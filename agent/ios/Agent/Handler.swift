import XCTest
import ObjectiveC

// The requests: what is on screen, touches, keys and text, apps, screenshots, settling.
// Coordinates are in points, the screen's own units on iOS.
final class Handler {
    /// The app the host launched last: what `screen` reads while it is in front.
    var app: XCUIApplication
    var appId = "com.apple.springboard"
    let springboard = XCUIApplication(bundleIdentifier: "com.apple.springboard")

    init() { app = springboard }

    func handle(_ r: [String: Any]) -> ([String: Any], Data?) {
        do {
            return try dispatch(r)
        } catch {
            return (["ok": false, "error": "\(error)"], nil)
        }
    }

    /// The last action was a swipe: its content stops at the lift, and what animates after
    /// (a scroll indicator fading) is not waited for.
    private var lastWasSwipe = false

    private func dispatch(_ r: [String: Any]) throws -> ([String: Any], Data?) {
        let ok: [String: Any] = ["ok": true]
        let op = r["op"] as? String ?? ""
        if !["ping", "info", "screen", "screenshot", "settle"].contains(op) { lastWasSwipe = op == "swipe" || op == "drag" }
        switch op {
        case "ping":
            return (ok, nil)
        case "info":
            let b = UIScreen.main.bounds
            return (["ok": true, "width": Int(b.width), "height": Int(b.height), "scale": UIScreen.main.scale], nil)
        case "screen":
            return (try screen(of: r["app"] as? String), nil)
        case "tap":
            try tap(r, count: 1)
            return (ok, nil)
        case "taps":
            try tap(r, count: max(int(r, "count"), 1))
            return (ok, nil)
        case "press":
            point(r, "x", "y").press(forDuration: Double(int(r, "ms")) / 1000)
            return (ok, nil)
        case "swipe":
            swipe(r)
            return (ok, nil)
        case "drag":
            drag(r)
            return (ok, nil)
        case "type":
            try type(r["text"] as? String ?? "")
            return (ok, nil)
        case "erase":
            try keys(String(repeating: XCUIKeyboardKey.delete.rawValue, count: int(r, "count")))
            return (ok, nil)
        case "key":
            try key(r["name"] as? String ?? "")
            return (ok, nil)
        case "launch":
            launch(r)
            return (ok, nil)
        case "launched":
            // Started by the host (`simctl launch`, back once the process starts): the app the
            // screen is read from, once it is in the foreground.
            // The process simctl started, by its pid: under the bundle id, XCTest can still hold
            // the one it stopped, and reading that is "Lost connection to the application".
            let id = r["app"] as? String ?? ""
            let pid = Int32(int(r, "pid"))
            let a = (pid > 0 ? application(pid: pid) : nil) ?? XCUIApplication(bundleIdentifier: id)
            let up = a.wait(for: .runningForeground, timeout: Double(max(int(r, "timeout"), 1000)) / 1000)
            app = a
            appId = id
            return (["ok": true, "foreground": up], nil)
        case "terminate":
            XCUIApplication(bundleIdentifier: r["app"] as? String ?? "").terminate()
            return (ok, nil)
        case "screenshot":
            let png = XCUIScreen.main.screenshot().pngRepresentation
            return (["ok": true, "blob": png.count], png)
        case "settle":
            return (settle(quiet: int(r, "quiet"), noChange: int(r, "nochange"), timeout: int(r, "timeout")), nil)
        case "openURL":
            try openURL(r)
            return (ok, nil)
        case "notifications":
            // Down from the top edge, left of centre (the right is the control centre).
            let start = springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0)).withOffset(CGVector(dx: 0, dy: 2))
            start.press(forDuration: 0.1, thenDragTo: springboard.coordinate(withNormalizedOffset: CGVector(dx: 0.3, dy: 0.75)))
            coverSheet = true
            return (ok, nil)
        case "hideKeyboard":
            hideKeyboard()
            return (ok, nil)
        case "orientation":
            try orientation(r["value"] as? String ?? "")
            return (ok, nil)
        case let op:
            throw AgentError("unknown op \(op)")
        }
    }

    private func int(_ r: [String: Any], _ k: String) -> Int {
        (r[k] as? NSNumber)?.intValue ?? 0
    }

    /// The app in front: the one launched last while it runs in the foreground, else the home
    /// screen.
    /// The cover sheet (the notification centre, the lock screen) is SpringBoard's, over an app
    /// that the accessibility client still has active: watched for from when it was opened.
    var coverSheet = false

    private func front() -> XCUIApplication {
        if coverSheet {
            let w = springboard.windows["SBCoverSheetWindow"]
            if w.exists && w.isHittable { return springboard }
            coverSheet = false
        }
        if let a = activeApp() { return a }
        return app != springboard && app.state == .runningForeground ? app : springboard
    }

    /// The app in front, as the accessibility client has it: XCUIApplication.state can still
    /// say foreground well after the home screen covered an app, and a backgrounded app's
    /// tree takes XCTest 30 s a try to give up on. A system prompt makes SpringBoard active.
    private func activeApp() -> XCUIApplication? {
        guard let active = activePid() else { return nil }
        if pid(of: app) == active { return app }
        if pid(of: springboard) == active { return springboard }
        // Another app (one a link opened): by its process.
        guard let other = application(pid: active) else { return nil }
        app = other
        appId = (other.responds(to: NSSelectorFromString("bundleID")) ? other.value(forKey: "bundleID") as? String : nil) ?? ""
        return other
    }

    /// The process in front, as WebDriverAgent finds it: the accessibility client's active
    /// applications (XCUIDevice's accessibility interface since Xcode 15; XCAXClient_iOS before).
    private func activePid() -> Int32? {
        let active = NSSelectorFromString("activeApplications")
        guard let client = axClient(), client.responds(to: active),
              let apps = client.perform(active)?.takeUnretainedValue() as? [NSObject] else { return nil }
        let pidKey = NSSelectorFromString("processIdentifier")
        for a in apps where a.responds(to: pidKey) {
            if let pid = (a.value(forKey: "processIdentifier") as? NSNumber)?.int32Value { return pid }
        }
        return nil
    }

    /// An app by its process (XCUIApplication's applicationWithPID:), not by its bundle id.
    private func application(pid: Int32) -> XCUIApplication? {
        let cls: AnyClass = XCUIApplication.self, sel = NSSelectorFromString("applicationWithPID:")
        guard let m = class_getClassMethod(cls, sel) else { return nil }
        typealias WithPid = @convention(c) (AnyClass, Selector, Int32) -> XCUIApplication?
        return unsafeBitCast(method_getImplementation(m), to: WithPid.self)(cls, sel, pid)
    }

    private func axClient() -> NSObject? {
        let device = XCUIDevice.shared as NSObject, interface = NSSelectorFromString("accessibilityInterface")
        if device.responds(to: interface), let c = device.perform(interface)?.takeUnretainedValue() as? NSObject { return c }
        let shared = NSSelectorFromString("sharedClient")
        if let cls = NSClassFromString("XCAXClient_iOS") as? NSObject.Type, cls.responds(to: shared),
           let c = cls.perform(shared)?.takeUnretainedValue() as? NSObject { return c }
        return nil
    }

    private func pid(of a: XCUIApplication) -> Int32? {
        guard a.responds(to: NSSelectorFromString("processID")) else { return nil }
        return (a.value(forKey: "processID") as? NSNumber)?.int32Value
    }

    private func point(_ r: [String: Any], _ kx: String, _ ky: String) -> XCUICoordinate {
        // A system gesture (from the screen's edge) is SpringBoard's to take.
        let base = (r["system"] as? Bool ?? false) ? springboard : front()
        return base.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
            .withOffset(CGVector(dx: int(r, kx), dy: int(r, ky)))
    }

    /// A tap held 10 ms: React Native hands a longer touch to a scroll view around it when that
    /// scrolls meanwhile (a carousel paging itself), and the press is cancelled. Repeated taps
    /// are one gesture, held 100 ms and at least 200 ms apart: React Native drops a press that
    /// comes ~150 ms after the last, and XCTest's own double tap lands there as one tap.
    /// Synthesized through testmanagerd in screen points, so only while the device is upright;
    /// XCTest's taps otherwise.
    private func tap(_ r: [String: Any], count: Int) throws {
        let p = CGPoint(x: int(r, "x"), y: int(r, "y"))
        let upright = [.portrait, .unknown, .faceUp, .faceDown].contains(XCUIDevice.shared.orientation)
        let system = r["system"] as? Bool ?? false
        guard upright, !system, Touch.available else {
            let c = point(r, "x", "y")
            for n in 0..<count {
                if n > 0 { Thread.sleep(forTimeInterval: Double(int(r, "ms")) / 1000) }
                c.tap()
            }
            return
        }
        // A modal still presenting takes no touch, and one can begin after the settle (a popup
        // that comes a while after): when the screen the host read differs from the one the
        // last settle ended on, the app's animations are waited out first, briefly. One that
        // just ended (a carousel's page turn) still has its scroll events on their way to React
        // Native, and one arriving while the touch is down cancels the press: after a wait,
        // they are let through first.
        if lastSignature == nil || lastSignature != settledSignature {
            let q = Date()
            Quiescence.wait(front(), animations: true, timeout: 0.3)
            if Date().timeIntervalSince(q) > 0.03 { Thread.sleep(forTimeInterval: 0.15) }
        }
        if count == 1 {
            // UIKit's controls (a switch, a system alert's buttons) need the touch to span a
            // display frame: 12 ms never toggled a UISwitch, 17 ms and more always did.
            let uikit = front() == springboard || switchFrames.contains { $0.contains(p) }
            try Touch.taps(at: p, count: 1, interval: 0, hold: uikit ? 0.03 : 0.01)
        } else if Double(int(r, "ms")) >= 300 {
            // Taps far enough apart for no double-tap window: each its own, from its start.
            let interval = Double(int(r, "ms")) / 1000
            for n in 0..<count {
                let start = Date()
                try Touch.taps(at: p, count: 1, interval: 0, hold: 0.01)
                if n < count - 1 { Thread.sleep(forTimeInterval: max(interval - Date().timeIntervalSince(start), 0)) }
            }
        } else {
            let interval = max(Double(int(r, "ms")) / 1000, 0.2)
            try Touch.taps(at: p, count: count, interval: interval, hold: min(0.1, interval / 2))
        }
    }

    /// Keys (deletes, Return) as key events into what has focus: XCTest's typeText refuses when
    /// the app it is called on has no element with keyboard focus (Settings' search field, in
    /// iOS 26's toolbar).
    private func keys(_ text: String) throws {
        guard Touch.available else { return front().typeText(text) }
        try Touch.type(text, perSecond: 100)
    }

    /// Text through the keyboard of the focused field: the first character alone, then the rest
    /// at 100 a second once the field shows it (iOS drops keys while the keyboard reacts to the
    /// first; a field that formats as it goes, a phone number's, drops them sent all at once).
    /// A field that does not show its value gets the rest at 30 a second after half a second.
    private func type(_ text: String) throws {
        guard let first = text.first else { return }
        guard Touch.available else { return front().typeText(text) }
        let field = front().descendants(matching: .any).matching(NSPredicate(format: "hasKeyboardFocus == true")).firstMatch
        let before = field.exists ? field.value as? String : nil
        try Touch.type(String(first), perSecond: 30)
        let rest = String(text.dropFirst())
        guard !rest.isEmpty else { return }
        guard let before else {
            Thread.sleep(forTimeInterval: 0.5)
            return try Touch.type(rest, perSecond: 30)
        }
        let deadline = Date(timeIntervalSinceNow: 0.5)
        while Date() < deadline, (field.value as? String) == before {
            Thread.sleep(forTimeInterval: 0.02)
        }
        try Touch.type(rest, perSecond: 100)
    }

    private func swipe(_ r: [String: Any]) {
        let from = point(r, "x1", "y1"), to = point(r, "x2", "y2")
        let dx = Double(int(r, "x2") - int(r, "x1")), dy = Double(int(r, "y2") - int(r, "y1"))
        let seconds = max(Double(int(r, "ms")) / 1000, 0.05)
        let fling = r["fling"] as? Bool ?? false
        let upright = [.portrait, .unknown, .faceUp, .faceDown].contains(XCUIDevice.shared.orientation)
        let a = CGPoint(x: int(r, "x1"), y: int(r, "y1")), b = CGPoint(x: int(r, "x2"), y: int(r, "y2"))
        if upright, !(r["system"] as? Bool ?? false), Touch.available {
            if !fling, (try? Touch.drag(from: a, to: b)) == true { return }
            if fling, (try? Touch.swipe(from: a, to: b, duration: seconds)) != nil { return }
        }
        // A drag that is not a fling stays a drag: much faster, and a list takes it as a flick
        // (or not as a scroll at all). A still or cancelled lift holds first, so nothing coasts.
        // At 1500 pt/s a list still coasted 70% further than the finger went, held or not.
        let speed = min(max((dx * dx + dy * dy).squareRoot() / seconds, 100), fling ? 20000 : 600)
        let hold = fling ? 0 : 0.3
        from.press(forDuration: 0.05, thenDragTo: to, withVelocity: XCUIGestureVelocity(CGFloat(speed)), thenHoldForDuration: hold)
    }

    /// Presses `hold` ms so the item lifts, moves to the end over `move` ms, and lets go.
    private func drag(_ r: [String: Any]) {
        let from = point(r, "x1", "y1"), to = point(r, "x2", "y2")
        let dx = Double(int(r, "x2") - int(r, "x1")), dy = Double(int(r, "y2") - int(r, "y1"))
        let speed = max((dx * dx + dy * dy).squareRoot() / max(Double(int(r, "move")) / 1000, 0.05), 50)
        from.press(forDuration: Double(int(r, "hold")) / 1000, thenDragTo: to, withVelocity: XCUIGestureVelocity(CGFloat(speed)), thenHoldForDuration: 0.2)
    }

    private func key(_ name: String) throws {
        switch name {
        case "home": XCUIDevice.shared.press(.home)
        case "enter": try keys("\n")
        case "backspace": try keys(XCUIKeyboardKey.delete.rawValue)
        case "tab": try keys("\t")
        case "space": try keys(" ")
        case "lock":
            XCUIDevice.shared.perform(NSSelectorFromString("pressLockButton"))
            coverSheet = true
        default: throw AgentError("the \(name) key is not on iOS")
        }
    }

    private func launch(_ r: [String: Any]) {
        let id = r["app"] as? String ?? ""
        let a = XCUIApplication(bundleIdentifier: id)
        a.launchArguments = r["args"] as? [String] ?? []
        a.launchEnvironment = r["env"] as? [String: String] ?? [:]
        // launch() stops a running app first; activate() brings it back as it was.
        if r["activate"] as? Bool ?? false { a.activate() } else { a.launch() }
        app = a
        appId = id
    }

    /// iOS has no call to put the keyboard away: a drag down from just above it (a list or
    /// scroll view that dismisses interactively takes it), then the keyboard's own key.
    private func hideKeyboard() {
        let a = front()
        let kb = a.keyboards.firstMatch
        guard kb.exists else { return }
        let f = kb.frame
        let origin = a.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        origin.withOffset(CGVector(dx: f.midX, dy: f.minY - 30))
            .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: f.midX, dy: f.maxY - 5)))
        if gone(kb) { return }
        for name in ["Hide keyboard", "Done", "done", "Return", "return"] {
            let key = kb.buttons[name]
            if key.exists {
                key.tap()
                if gone(kb) { return }
            }
        }
    }

    /// A link straight into the app that handles it, with no "Open in ...?" prompt (iOS 16.4).
    private func openURL(_ r: [String: Any]) throws {
        guard let url = URL(string: r["url"] as? String ?? "") else { throw AgentError("not a URL") }
        guard #available(iOS 16.4, *) else { throw AgentError("opening links needs iOS 16.4") }
        if let id = r["app"] as? String {
            let a = XCUIApplication(bundleIdentifier: id)
            a.open(url)
            app = a
            appId = id
        } else {
            XCUIDevice.shared.system.open(url)
        }
    }

    private func gone(_ e: XCUIElement) -> Bool {
        for _ in 0..<10 {
            if !e.exists { return true }
            Thread.sleep(forTimeInterval: 0.1)
        }
        return false
    }

    private func orientation(_ v: String) throws {
        let o: UIDeviceOrientation
        switch v {
        case "portrait": o = .portrait
        case "landscapeLeft": o = .landscapeLeft
        case "landscapeRight": o = .landscapeRight
        case "upsideDown": o = .portraitUpsideDown
        default: throw AgentError("unknown orientation \(v)")
        }
        XCUIDevice.shared.orientation = o
    }

    // MARK: the screen

    /// The app in front as the Android agent sends a screen: windows top first, then every
    /// node in pre-order with its depth. A system alert over the app (a permission prompt) is
    /// the top window; the keyboard is a window of its own (type 2, an input method's).
    /// The switches of the last screen read: a tap on one is held long enough for UIKit.
    private var switchFrames: [CGRect] = []

    private func screen(of id: String?) throws -> [String: Any] {
        switchFrames = []
        if let id {
            let a = id == "com.apple.springboard" ? springboard : XCUIApplication(bundleIdentifier: id)
            var nodes: [[String: Any]] = []
            flatten(try a.snapshot(), depth: 0, window: 0, keyboard: 1, into: &nodes)
            return ["ok": true, "windows": [["pkg": id, "type": 1], ["pkg": id, "type": 2]], "nodes": nodes]
        }
        let target = front()
        var windows: [[String: Any]] = []
        var nodes: [[String: Any]] = []
        // Without the accessibility client's word on what is in front, a system alert is
        // looked for over the app (with it, an alert makes SpringBoard the front app).
        if target != springboard, activePid() == nil, springboard.alerts.firstMatch.exists {
            let w = windows.count
            windows.append(["pkg": "com.apple.springboard", "type": 1])
            flatten(try springboard.alerts.firstMatch.snapshot(), depth: 0, window: w, keyboard: w, into: &nodes)
        }
        // An app just relaunched can still be listed by the process that was stopped: reading
        // it is "Lost connection to the application". The app in front is looked up again and
        // read again, a few times over a second.
        var app = target, taken: XCUIElementSnapshot? = nil, tries = 0
        let issue = VirtuosoAgent.issue
        while taken == nil {
            do {
                taken = try app.snapshot()
            } catch {
                tries += 1
                if tries >= 4 { throw error }
                Thread.sleep(forTimeInterval: 0.25)
                app = front()
            }
        }
        VirtuosoAgent.issue = issue
        let w = windows.count
        windows.append(["pkg": bundleId(app), "type": 1])
        windows.append(["pkg": bundleId(app), "type": 2])
        let snapshot = taken!
        flatten(snapshot, depth: 0, window: w, keyboard: w + 1, into: &nodes)
        // What the host now sees: what the next settle starts from.
        _ = signature(of: snapshot)
        return ["ok": true, "windows": windows, "nodes": nodes]
    }

    private func bundleId(_ a: XCUIApplication) -> String {
        a == springboard ? "com.apple.springboard" : appId
    }

    /// `clip`: what the scroll views around the node show. iOS gives a node scrolled under a
    /// header, or out of its list, its whole frame: as on Android, a node shows only what its
    /// scroll views do, and one they hide is not visible.
    private func flatten(_ s: XCUIElementSnapshot, depth: Int, window: Int, keyboard: Int, clip: CGRect = .infinite, into nodes: inout [[String: Any]]) {
        let inKeyboard = window == keyboard || s.elementType == .keyboard
        let w = inKeyboard ? keyboard : window
        nodes.append(node(s, depth: depth, window: w, clip: clip))
        let inner = Handler.scrolling.contains(s.elementType) ? clip.intersection(s.frame) : clip
        for c in s.children {
            flatten(c, depth: depth + 1, window: inKeyboard ? keyboard : window, keyboard: keyboard, clip: inner, into: &nodes)
        }
    }

    // Flags as the host reads them (screen.rs).
    static let clickable = 1, enabled = 2, focused = 4, scrollable = 8, checked = 16, visible = 32
    static let selected = 64, editable = 128, checkable = 256, longClickable = 512, password = 1024

    private static let textInputs: Set<XCUIElement.ElementType> = [.textField, .secureTextField, .searchField, .textView]
    private static let tappable: Set<XCUIElement.ElementType> = [
        .button, .cell, .link, .switch, .toggle, .textField, .secureTextField, .searchField, .textView,
        .tab, .segmentedControl, .menuItem, .checkBox, .radioButton, .slider, .stepper, .picker, .pickerWheel,
        .key, .icon, .tabBar, .image,
    ]
    private static let scrolling: Set<XCUIElement.ElementType> = [.scrollView, .table, .collectionView, .webView]

    private func node(_ s: XCUIElementSnapshot, depth: Int, window: Int, clip: CGRect) -> [String: Any] {
        // What shows of it; all of it where nothing clips it.
        let shown = clip.isInfinite ? s.frame : s.frame.intersection(clip)
        let f = shown.isNull || shown.width <= 0 || shown.height <= 0 ? CGRect(origin: s.frame.origin, size: .zero) : shown
        let type = s.elementType
        let value = (s.value as? String) ?? (s.value as? NSNumber)?.stringValue
        let placeholder = s.placeholderValue ?? ""
        var text = s.label, desc = ""
        if type == .application {
            // The app's name, not text on screen: Android's root node has none either, and a
            // flow's tapOn would take the whole screen for an element named like the app.
            text = ""
        } else if Handler.textInputs.contains(type) {
            // A field's text is what is typed in it; its label describes it.
            text = (value != nil && value != placeholder) ? value! : ""
            desc = s.label
        } else if text.isEmpty, type == .staticText {
            text = value ?? ""
        }
        var flags = 0
        if f.width > 0, f.height > 0 { flags |= Handler.visible }
        if s.isEnabled { flags |= Handler.enabled }
        if s.isSelected { flags |= Handler.selected }
        if Handler.tappable.contains(type) { flags |= Handler.clickable | Handler.longClickable }
        if Handler.scrolling.contains(type) { flags |= Handler.scrollable }
        if Handler.textInputs.contains(type) {
            flags |= Handler.editable
            // The field typing goes to (XCElementSnapshot's hasKeyboardFocus).
            let o = s as AnyObject
            if o.responds(to: NSSelectorFromString("hasKeyboardFocus")), (o.value(forKey: "hasKeyboardFocus") as? Bool) == true {
                flags |= Handler.focused
            }
        }
        if type == .secureTextField { flags |= Handler.password }
        if type == .switch { switchFrames.append(f) }
        if [.switch, .toggle, .checkBox].contains(type) {
            flags |= Handler.checkable
            if value == "1" { flags |= Handler.checked }
        }
        // React Native spells a checkbox's or radio button's role and state into its value
        // ("checkbox, checked", "radio button, unchecked"): UIKit has no trait for them.
        let words = (value ?? "").components(separatedBy: ", ")
        if words.count >= 2, words[0] == "checkbox" || words[0] == "radio button" {
            flags |= Handler.checkable
            if words[1] == "checked" { flags |= Handler.checked }
        }
        var n: [String: Any] = [
            "d": depth, "w": window, "f": flags, "t": text, "c": desc, "i": s.identifier,
            "k": Handler.typeName(type),
            "b": [Int(f.minX.rounded()), Int(f.minY.rounded()), Int(f.maxX.rounded()), Int(f.maxY.rounded())],
        ]
        if !placeholder.isEmpty { n["h"] = placeholder }
        return n
    }

    static func typeName(_ t: XCUIElement.ElementType) -> String {
        switch t {
        case .application: return "Application"
        case .window: return "Window"
        case .button: return "Button"
        case .staticText: return "StaticText"
        case .textField: return "TextField"
        case .secureTextField: return "SecureTextField"
        case .searchField: return "SearchField"
        case .textView: return "TextView"
        case .image: return "Image"
        case .cell: return "Cell"
        case .table: return "Table"
        case .collectionView: return "CollectionView"
        case .scrollView: return "ScrollView"
        case .navigationBar: return "NavigationBar"
        case .tabBar: return "TabBar"
        case .switch: return "Switch"
        case .toggle: return "Toggle"
        case .link: return "Link"
        case .alert: return "Alert"
        case .keyboard: return "Keyboard"
        case .key: return "Key"
        case .webView: return "WebView"
        case .slider: return "Slider"
        case .activityIndicator: return "ActivityIndicator"
        case .other: return "Other"
        default: return "Element\(t.rawValue)"
        }
    }

    // MARK: settling

    /// Waits for the screen to stop changing: done once two reads in a row agree after a
    /// change, each after XCTest's idle wait (that wait is the quiet; `quiet` is unused), or
    /// `noChange` ms passed without one, or `timeout` ms in all.
    private func settle(quiet: Int, noChange: Int, timeout: Int) -> [String: Any] {
        let start = Date()
        // From the screen last seen (the host's read before the action, or the last settle's
        // end): a snapshot of its own would cost as much again.
        var last = lastSignature ?? signature(), changed = false
        // XCTest's idle wait first, each time: a modal presenting takes no touch while it
        // animates in, and a fade moves no frame. An animation that never ends would hold it:
        // once a wait takes its whole cap, the rest wait for the event loop only.
        var animations = !lastWasSwipe
        // Where the time went, for the host's trace: idle waits, and snapshots.
        var idleMs = 0.0, snapMs = 0.0, snaps = 0
        while true {
            let left = Double(timeout) / 1000 - Date().timeIntervalSince(start)
            let q = Date()
            // Capped at 0.6 s: longer than UIKit's transitions (~0.35-0.5 s); what animates
            // longer (Settings' screens, after a push) is left to the signatures.
            if Quiescence.wait(front(), animations: animations, timeout: max(min(0.6, left), 0.1)) { animations = false }
            idleMs += Date().timeIntervalSince(q) * 1000
            if Date().timeIntervalSince(q) < 0.05 { Thread.sleep(forTimeInterval: 0.05 - Date().timeIntervalSince(q)) }
            let sn = Date()
            let s = signature()
            snapMs += Date().timeIntervalSince(sn) * 1000
            snaps += 1
            let now = Date()
            var held = false
            if s != last {
                changed = true
                last = s
            } else {
                held = changed
            }
            let total = now.timeIntervalSince(start) * 1000
            if !changed && total >= Double(noChange) || held {
                // The screen it held still on, for the host's next read.
                settledSignature = last
                var reply: [String: Any] = ["ok": true, "changed": changed, "timedOut": false,
                                            "idleMs": Int(idleMs), "snapMs": Int(snapMs), "snaps": snaps]
                if let (snapshot, app) = lastSnapshot {
                    switchFrames = []
                    var nodes: [[String: Any]] = []
                    flatten(snapshot, depth: 0, window: 0, keyboard: 1, into: &nodes)
                    reply["windows"] = [["pkg": bundleId(app), "type": 1], ["pkg": bundleId(app), "type": 2]]
                    reply["nodes"] = nodes
                }
                return reply
            }
            if total >= Double(timeout) {
                return ["ok": true, "changed": changed, "timedOut": true]
            }
        }
    }

    /// An endless animation (a marquee, a looping Animated view, a number changing on every
    /// frame) changes a few elements on every read, and the screen would never hold still: an
    /// element seen changing (moving, or saying something else) for restlessSeconds while most
    /// of the screen holds still is restless, and what it says and where it is are left out. A
    /// pause of up to restlessPauseSeconds does not end its changing (a shimmer clipped by its
    /// block). A scroll or a transition changes most of the screen, and is waited for.
    private static let restlessSeconds = 0.5, restlessPauseSeconds = 0.6
    /// The last signature's elements (what they say and where), and for each element changing
    /// since, when it began and when it last changed.
    private var lastStates: [String: String] = [:]
    private var movingSince: [String: Date] = [:], movedAt: [String: Date] = [:]

    /// The last signature's snapshot, and the app it is of: the screen a settle ends on.
    private var lastSnapshot: (XCUIElementSnapshot, XCUIApplication)?

    /// The signature of the last screen seen (a read's, or a settle's): what a settle compares
    /// with first, without a snapshot of its own. And the one the last settle ended on.
    private var lastSignature: String?
    private var settledSignature: String?

    private func signature() -> String {
        let app = front()
        guard let s = try? app.snapshot() else { lastSnapshot = nil; return "" }
        lastSnapshot = (s, app)
        return signature(of: s)
    }

    private func signature(of s: XCUIElementSnapshot) -> String {
        let sig = digest(s)
        lastSignature = sig
        return sig
    }

    private func digest(_ s: XCUIElementSnapshot) -> String {
        // Each element: what it is (its kind and id, alike ones told apart by their order), and
        // its state (what it says, where it is).
        var marks: [(id: String, state: String)] = []
        var seen: [String: Int] = [:]
        func walk(_ s: XCUIElementSnapshot) {
            // A scroll bar fades for a second after every scroll: not a change to wait out.
            if s.label.hasSuffix("scroll bar") || s.label.contains("scroll bar,") { return }
            var id = "\(s.elementType.rawValue)|\(s.identifier)"
            let n = seen[id, default: 0]
            seen[id] = n + 1
            id += "#\(n)"
            let f = s.frame
            marks.append((id, "\(s.label)|\(s.value ?? "")|\(Int(f.minX)),\(Int(f.minY)),\(Int(f.width)),\(Int(f.height))"))
            s.children.forEach(walk)
        }
        walk(s)
        let now = Date()
        var states: [String: String] = [:], moving: [String: Date] = [:], moved: [String: Date] = [:]
        for m in marks {
            states[m.id] = m.state
            if let was = lastStates[m.id], was != m.state {
                moving[m.id] = movingSince[m.id] ?? now
                moved[m.id] = now
            } else if let since = movingSince[m.id], let at = movedAt[m.id], now.timeIntervalSince(at) < Handler.restlessPauseSeconds {
                moving[m.id] = since
                moved[m.id] = at
            }
        }
        lastStates = states
        movingSince = moving
        movedAt = moved
        let few = moving.count * 3 <= marks.count
        var out = ""
        for m in marks {
            out += m.id
            // Restless: changing for a while, and seen changing again since it began (an element
            // that changed once, between two settles, is not).
            if few, let since = moving[m.id], let at = moved[m.id], at > since, now.timeIntervalSince(since) >= Handler.restlessSeconds {
                out += ";"
                continue
            }
            out += "|\(m.state);"
        }
        return out
    }
}

/// Touches played by testmanagerd as XCTest's own are (XCPointerEventPath in an
/// XCSynthesizedEventRecord, as WebDriverAgent sends them), with their timing ours. Points are
/// the screen's, upright.
enum Touch {
    static var available: Bool {
        NSClassFromString("XCPointerEventPath") != nil && NSClassFromString("XCSynthesizedEventRecord") != nil
            && NSClassFromString("XCTRunnerDaemonSession") != nil
    }

    /// `count` taps at a point, one every `interval` seconds, each held `hold`: one record with
    /// a finger per tap (a second press on one path does not land as a tap). Returns once
    /// testmanagerd has played them.
    static func taps(at p: CGPoint, count: Int, interval: TimeInterval, hold: TimeInterval) throws {
        var paths: [NSObject] = []
        for k in 0..<max(count, 1) {
            let offset = Double(k) * interval
            let path = try touchPath(at: p, offset: offset)
            try call(path, "liftUpAtOffset:", offset + hold)
            paths.append(path)
        }
        try synthesize(try record(with: paths))
    }

    /// The flow format's swipe: down at start, resting 100 ms, then gliding to the end over
    /// `duration` and lifting there still moving (testmanagerd plays a move until the next
    /// event), so content flings on at the glide's speed, UIKit and React Native alike.
    static func swipe(from start: CGPoint, to end: CGPoint, duration: TimeInterval) throws {
        let path = try touchPath(at: start, offset: 0)
        let move = NSSelectorFromString("moveToPoint:atOffset:")
        guard path.responds(to: move) else { throw AgentError("XCPointerEventPath.moveToPoint:atOffset: is missing") }
        typealias Move = @convention(c) (NSObject, Selector, CGPoint, TimeInterval) -> Void
        unsafeBitCast(path.method(for: move), to: Move.self)(path, move, end, 0.1)
        try call(path, "liftUpAtOffset:", 0.1 + duration)
        try synthesize(try record(with: [path]))
    }

    /// The drag that does not fling: content stops when the touch lifts. Momentum comes from the
    /// speed of the touch's last moves (a touch that rests sends no events, so resting before the
    /// lift does not help), so the drag ends slowly: kick points in kickMs, the rest but the
    /// last tail points in fastMs, those in tailMs (200 points/s: UIKit flung once in three at
    /// 250, React Native at 333). 350 points scroll 250-340 in 390 ms. Returns after the lift,
    /// once testmanagerd has played it; false when this Xcode lacks explicit move events.
    static let kick = 12.0, kickMs = 40.0, fastMs = 150.0, tail = 40.0, tailMs = 200.0

    static func drag(from start: CGPoint, to end: CGPoint) throws -> Bool {
        let makeSel = NSSelectorFromString("moveEventWithStartPoint:destination:offset:duration:")
        let addSel = NSSelectorFromString("_addPointerEvent:")
        guard let eventClass = NSClassFromString("XCPointerEvent"), let make = class_getClassMethod(eventClass, makeSel) else { return false }
        let dx = end.x - start.x, dy = end.y - start.y, length = max(hypot(dx, dy), 1)
        let k = min(kick, length / 10), t = min(tail, length / 4)
        func along(_ d: Double) -> CGPoint { CGPoint(x: start.x + dx * d / length, y: start.y + dy * d / length) }
        let kickS = kickMs / 1000, fastS = fastMs / 1000, tailS = tailMs / 1000
        let points: [(point: CGPoint, offset: TimeInterval, duration: TimeInterval)] = [
            (start, 0, 0), (along(k), 0, kickS), (along(length - t), kickS, fastS), (end, kickS + fastS, tailS),
        ]
        let path = try touchPath(at: start, offset: 0)
        guard path.responds(to: addSel) else { return false }
        typealias Make = @convention(c) (AnyClass, Selector, CGPoint, CGPoint, TimeInterval, TimeInterval) -> NSObject
        typealias Add = @convention(c) (NSObject, Selector, NSObject) -> Void
        let makeMove = unsafeBitCast(method_getImplementation(make), to: Make.self)
        let add = unsafeBitCast(path.method(for: addSel), to: Add.self)
        for (a, b) in zip(points, points.dropFirst()) {
            add(path, addSel, makeMove(eventClass, makeSel, a.point, b.point, b.offset, b.duration))
        }
        // liftUpAtOffset: lifts where the path's last moveToPoint went (here the start): the lift
        // comes from a path at the end point.
        let liftPath = try touchPath(at: end, offset: 0)
        try call(liftPath, "liftUpAtOffset:", kickS + fastS + tailS)
        let eventsSel = NSSelectorFromString("pointerEvents")
        guard liftPath.responds(to: eventsSel),
              let lift = (liftPath.perform(eventsSel)?.takeUnretainedValue() as? [NSObject])?.last else { return false }
        add(path, addSel, lift)
        // Back 100 ms after the lift, the content still: testmanagerd confirms ~250 ms after
        // that, and its answer is collected before the next touch.
        let sent = Date()
        try synthesize(try record(with: [path]), wait: false)
        Thread.sleep(forTimeInterval: max(kickS + fastS + tailS + 0.1 - Date().timeIntervalSince(sent), 0))
        return true
    }

    /// Text typed as key events into what has focus, `perSecond` characters a second.
    static func type(_ text: String, perSecond: Int) throws {
        guard let cls = NSClassFromString("XCPointerEventPath") as? NSObject.Type else { throw AgentError("XCPointerEventPath is missing") }
        let alloc = cls.perform(NSSelectorFromString("alloc")).takeUnretainedValue() as! NSObject
        let initText = NSSelectorFromString("initForTextInput")
        guard alloc.responds(to: initText) else { throw AgentError("XCPointerEventPath.initForTextInput is missing") }
        let path = alloc.perform(initText).takeUnretainedValue() as! NSObject
        let sel = NSSelectorFromString("typeText:atOffset:typingSpeed:shouldRedact:")
        guard path.responds(to: sel) else { throw AgentError("XCPointerEventPath.typeText:atOffset:typingSpeed:shouldRedact: is missing") }
        typealias TypeText = @convention(c) (NSObject, Selector, NSString, TimeInterval, UInt64, Bool) -> Void
        unsafeBitCast(path.method(for: sel), to: TypeText.self)(path, sel, text as NSString, 0, UInt64(max(perSecond, 1)), false)
        try synthesize(try record(with: [path]))
    }

    private static func touchPath(at p: CGPoint, offset: TimeInterval) throws -> NSObject {
        guard let cls = NSClassFromString("XCPointerEventPath") as? NSObject.Type else { throw AgentError("XCPointerEventPath is missing") }
        let sel = NSSelectorFromString("initForTouchAtPoint:offset:")
        let alloc = cls.perform(NSSelectorFromString("alloc")).takeUnretainedValue() as! NSObject
        guard alloc.responds(to: sel) else { throw AgentError("XCPointerEventPath.initForTouchAtPoint:offset: is missing") }
        typealias Init = @convention(c) (NSObject, Selector, CGPoint, TimeInterval) -> NSObject
        return unsafeBitCast(alloc.method(for: sel), to: Init.self)(alloc, sel, p, offset)
    }

    private static func record(with paths: [NSObject]) throws -> NSObject {
        guard let cls = NSClassFromString("XCSynthesizedEventRecord") as? NSObject.Type else { throw AgentError("XCSynthesizedEventRecord is missing") }
        let sel = NSSelectorFromString("initWithName:interfaceOrientation:")
        let alloc = cls.perform(NSSelectorFromString("alloc")).takeUnretainedValue() as! NSObject
        guard alloc.responds(to: sel) else { throw AgentError("XCSynthesizedEventRecord.initWithName:interfaceOrientation: is missing") }
        typealias Init = @convention(c) (NSObject, Selector, NSString, Int) -> NSObject
        let record = unsafeBitCast(alloc.method(for: sel), to: Init.self)(alloc, sel, "Tap", UIInterfaceOrientation.portrait.rawValue)
        let add = NSSelectorFromString("addPointerEventPath:")
        guard record.responds(to: add) else { throw AgentError("XCSynthesizedEventRecord.addPointerEventPath: is missing") }
        typealias Add = @convention(c) (NSObject, Selector, NSObject) -> Void
        for path in paths { unsafeBitCast(record.method(for: add), to: Add.self)(record, add, path) }
        return record
    }

    /// A touch sent without waiting for testmanagerd's answer: awaited before the next one
    /// (events overlapping in time could mix).
    private final class Pending { var done = false; var error: Error? }
    private static var pending: Pending?

    private static func awaitPending() throws {
        guard let p = pending else { return }
        pending = nil
        let deadline = Date(timeIntervalSinceNow: 10)
        while !p.done && Date() < deadline {
            RunLoop.current.run(mode: .default, before: Date(timeIntervalSinceNow: 0.005))
        }
        if let e = p.error { throw e }
    }

    /// testmanagerd plays the record; its answer comes on the run loop, which keeps turning
    /// while it is awaited (requests are served on the main thread).
    private static func synthesize(_ record: NSObject, wait: Bool = true) throws {
        try awaitPending()
        guard let cls = NSClassFromString("XCTRunnerDaemonSession"),
              let session = (cls as AnyObject).perform(NSSelectorFromString("sharedSession"))?.takeUnretainedValue() as? NSObject,
              let proxy = session.perform(NSSelectorFromString("daemonProxy"))?.takeUnretainedValue() as? NSObject
        else { throw AgentError("XCTRunnerDaemonSession.sharedSession.daemonProxy is missing") }
        let sel = NSSelectorFromString("_XCT_synthesizeEvent:completion:")
        guard proxy.responds(to: sel) else { throw AgentError("_XCT_synthesizeEvent:completion: is missing") }
        typealias Call = @convention(c) (NSObject, Selector, NSObject, @escaping (Error?) -> Void) -> Void
        let p = Pending()
        unsafeBitCast(proxy.method(for: sel), to: Call.self)(proxy, sel, record) { error in
            p.error = error
            p.done = true
        }
        pending = p
        if wait { try awaitPending() }
    }

    /// target.selector(offset), for XCPointerEventPath's offset setters.
    private static func call(_ target: NSObject, _ selector: String, _ offset: TimeInterval) throws {
        let sel = NSSelectorFromString(selector)
        guard target.responds(to: sel) else { throw AgentError("XCPointerEventPath.\(selector) is missing") }
        typealias Call = @convention(c) (NSObject, Selector, TimeInterval) -> Void
        unsafeBitCast(target.method(for: sel), to: Call.self)(target, sel, offset)
    }
}
