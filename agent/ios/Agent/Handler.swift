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

    private static let reads: Set<String> = ["ping", "info", "screen", "screenshot", "settle"]

    private func dispatch(_ r: [String: Any]) throws -> ([String: Any], Data?) {
        let ok: [String: Any] = ["ok": true]
        let op = r["op"] as? String ?? ""
        let action = !Handler.reads.contains(op)
        if action { lastWasSwipe = op == "swipe" || op == "drag" }
        switch op {
        case "ping":
            return (ok, nil)
        case "info":
            let b = UIScreen.main.bounds
            return (["ok": true, "width": Int(b.width), "height": Int(b.height), "scale": UIScreen.main.scale], nil)
        case "screen":
            if let id = r["app"] as? String { return (try screen(of: id), nil) }
            let read = try readFront(full: r["full"] as? Bool ?? false)
            // The wait's screen is still the one it has: no tree to send.
            if let was = seq(r, "ifNot"), was == read.seq {
                return (["ok": true, "unchanged": true, "seq": NSNumber(value: was)], nil)
            }
            return (raw(read, extra: ""), nil)
        case "tap":
            // The lookup's screen, by its hash: still it, the tap goes in this request; else the
            // screen as it is now goes back, untapped.
            if let want = seq(r, "ifSeq") {
                let read = try readFront(full: false)
                if read.seq != want { return (raw(read, extra: ",\"tapped\":false"), nil) }
            }
            try tap(r, count: 1)
            return (ok, nil)
        case "taps":
            try tap(r, count: max(int(r, "count"), 1))
            return (ok, nil)
        case "press":
            try press(r)
            return (ok, nil)
        case "clipboard":
            // In the simulator's pasteboard, as `simctl pbcopy` puts it (that took ~5 s).
            UIPasteboard.general.string = r["text"] as? String ?? ""
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
            // Its state every 50 ms: XCTest's wait for a state checks it more coarsely.
            let deadline = Date(timeIntervalSinceNow: Double(max(int(r, "timeout"), 1000)) / 1000)
            var up = a.state == .runningForeground
            while !up && Date() < deadline {
                Thread.sleep(forTimeInterval: 0.05)
                up = a.state == .runningForeground
            }
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
            return (settle(noChange: int(r, "nochange"), timeout: int(r, "timeout")), nil)
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

    private func seq(_ r: [String: Any], _ k: String) -> UInt64? {
        (r[k] as? NSNumber)?.uint64Value
    }

    /// The app in front: the one launched last while it runs in the foreground, else the home
    /// screen.
    /// The cover sheet (the notification centre, the lock screen) is SpringBoard's, over an app
    /// that the accessibility client still has active: watched for from when it was opened.
    var coverSheet = false
    /// The last front() had the accessibility client's word on what is in front.
    private var frontKnown = false

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
        frontKnown = false
        guard let active = activePid() else { return nil }
        frontKnown = true
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
        guard let client = Handler.axClient(), client.responds(to: active),
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

    static func axClient() -> NSObject? {
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

    /// The device's orientation, as the orientation op last set it (read once before that).
    private static var orientation: UIDeviceOrientation?

    /// Upright: touches go through testmanagerd in screen points.
    private var upright: Bool {
        let o = Handler.orientation ?? XCUIDevice.shared.orientation
        Handler.orientation = o
        return [.portrait, .unknown, .faceUp, .faceDown].contains(o)
    }

    /// A tap held 10 ms: React Native hands a longer touch to a scroll view around it when that
    /// scrolls meanwhile (a carousel paging itself), and the press is cancelled. Repeated taps
    /// are one gesture, held 100 ms and at least 200 ms apart: React Native drops a press that
    /// comes ~150 ms after the last, and XCTest's own double tap lands there as one tap.
    /// Synthesized through testmanagerd in screen points, so only while the device is upright;
    /// XCTest's taps otherwise.
    private func tap(_ r: [String: Any], count: Int) throws {
        let p = CGPoint(x: int(r, "x"), y: int(r, "y"))
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
        let f = front()
        restBeforeTouch(f)
        if count == 1 {
            // UIKit's controls (a switch, a system alert's buttons) need the touch to span a
            // display frame: 12 ms never toggled a UISwitch, 17 ms and more always did.
            let uikit = f == springboard || switchFrames.contains { $0.contains(p) }
            try Touch.taps(at: p, count: 1, interval: 0, hold: uikit ? 0.02 : 0.01, wait: false)
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
            try Touch.taps(at: p, count: count, interval: interval, hold: min(0.1, interval / 2), wait: false)
        }
    }

    /// A screen read since the last settle may still be presenting: its animations waited out,
    /// briefly, and scroll events still on their way let through (see tap).
    private func restBeforeTouch(_ f: XCUIApplication) {
        if lastSignature == nil || lastSignature != settledSignature {
            let q = Date()
            Quiescence.wait(f, animations: true, timeout: 0.3)
            if Date().timeIntervalSince(q) > 0.03 { Thread.sleep(forTimeInterval: 0.15) }
        }
    }

    /// A touch held `ms`, as a tap is made: back 100 ms after the lift (XCTest's own press
    /// waited for the app to idle first, and for testmanagerd's answer).
    private func press(_ r: [String: Any]) throws {
        let seconds = Double(int(r, "ms")) / 1000
        guard upright, !(r["system"] as? Bool ?? false), Touch.available else {
            return point(r, "x", "y").press(forDuration: seconds)
        }
        restBeforeTouch(front())
        try Touch.taps(at: CGPoint(x: int(r, "x"), y: int(r, "y")), count: 1, interval: 0, hold: seconds, wait: false)
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
    /// Back once the keys are sent: testmanagerd's answer (~250 ms after the last) is collected
    /// by the settle that follows, or before the next touch or keys.
    private func type(_ text: String) throws {
        guard let first = text.first else { return }
        guard Touch.available else { return front().typeText(text) }
        let field = front().descendants(matching: .any).matching(NSPredicate(format: "hasKeyboardFocus == true")).firstMatch
        // What the field says now, from one snapshot of it.
        let snapshot = try? field.snapshot()
        let before = snapshot.flatMap { $0.value as? String }
        try Touch.type(String(first), perSecond: 30, wait: false)
        let rest = String(text.dropFirst())
        guard !rest.isEmpty else { return }
        guard let before else {
            Thread.sleep(forTimeInterval: 0.5)
            return try Touch.type(rest, perSecond: 30, wait: false)
        }
        // Watched through a snapshot of the field's value alone (field.value looks the field
        // up again in the whole app each time).
        let says: () -> String? = {
            if let snapshot, let v = Snap.value(of: snapshot) { return v }
            return field.value as? String
        }
        let deadline = Date(timeIntervalSinceNow: 0.5)
        while Date() < deadline, says() == before {
            Thread.sleep(forTimeInterval: 0.02)
        }
        try Touch.type(rest, perSecond: 100, wait: false)
    }

    /// XCElementSnapshot's hasKeyboardFocus.
    private static func hasFocus(_ s: XCUIElementSnapshot) -> Bool {
        let o = s as AnyObject
        return o.responds(to: Handler.focusSelector) && (o.value(forKey: "hasKeyboardFocus") as? Bool) == true
    }
    private static let focusSelector = NSSelectorFromString("hasKeyboardFocus")

    private func swipe(_ r: [String: Any]) {
        let from = point(r, "x1", "y1"), to = point(r, "x2", "y2")
        let dx = Double(int(r, "x2") - int(r, "x1")), dy = Double(int(r, "y2") - int(r, "y1"))
        let seconds = max(Double(int(r, "ms")) / 1000, 0.05)
        let fling = r["fling"] as? Bool ?? false
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
        guard let f = (try? kb.snapshot())?.frame else { return }
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
        let deadline = Date(timeIntervalSinceNow: 1)
        while Date() < deadline {
            if !e.exists { return true }
            Thread.sleep(forTimeInterval: 0.03)
        }
        return !e.exists
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
        Handler.orientation = o
    }

    // MARK: the screen

    /// The app in front as the Android agent sends a screen: windows top first, then every
    /// node in pre-order with its depth. A system alert over the app (a permission prompt) is
    /// the top window; the keyboard is a window of its own (type 2, an input method's).
    /// The switches of the last screen read: a tap on one is held long enough for UIKit.
    private var switchFrames: [CGRect] = []

    /// A read of the app in front: its windows and nodes as JSON, the read's hash (a wait or a
    /// tap names it to ask whether the screen is still this one), and whether the snapshot
    /// fetched only the attributes a screen uses.
    struct Read {
        let parts: [UInt8]
        let seq: UInt64
        let trimmed: Bool
    }

    /// One app's whole tree, in front or not (the home screen's, a system prompt's).
    private func screen(of id: String) throws -> [String: Any] {
        switchFrames = []
        let a = id == "com.apple.springboard" ? springboard : XCUIApplication(bundleIdentifier: id)
        let w = Walk(marking: false)
        walk(try a.snapshot(), depth: 0, window: 0, keyboard: 1, into: w)
        let (parts, seq) = encoded(windows: [(id, 1), (id, 2)], w)
        return raw(Read(parts: parts, seq: seq, trimmed: false), extra: "")
    }

    private func readFront(full: Bool) throws -> Read {
        switchFrames = []
        let target = front()
        var windows: [(String, Int)] = []
        let w = Walk(marking: false)
        // Without the accessibility client's word on what is in front, a system alert is
        // looked for over the app (with it, an alert makes SpringBoard the front app).
        if target != springboard, !frontKnown, springboard.alerts.firstMatch.exists {
            windows.append(("com.apple.springboard", 1))
            walk(try springboard.alerts.firstMatch.snapshot(), depth: 0, window: 0, keyboard: 0, into: w)
        }
        // An app just relaunched can still be listed by the process that was stopped: reading
        // it is "Lost connection to the application". The app in front is looked up again and
        // read again, a few times over a second.
        var app = target, taken: (XCUIElementSnapshot, Bool)? = nil, tries = 0
        let issue = VirtuosoAgent.issue
        while taken == nil {
            do {
                taken = try Snap.take(app, pid: pid(of: app), full: full)
            } catch {
                tries += 1
                if tries >= 4 { throw error }
                Thread.sleep(forTimeInterval: 0.25)
                app = front()
            }
        }
        VirtuosoAgent.issue = issue
        let k = windows.count
        windows.append((bundleId(app), 1))
        windows.append((bundleId(app), 2))
        let (snapshot, trimmed) = taken!
        w.marking = true
        walk(snapshot, depth: 0, window: k, keyboard: k + 1, into: w)
        // What the host now sees: what the next settle starts from.
        lastSignature = digest(w.marks)
        lastSnapshot = (snapshot, app)
        let (parts, seq) = encoded(windows: windows, w)
        return Read(parts: parts, seq: seq, trimmed: trimmed)
    }

    private func bundleId(_ a: XCUIApplication) -> String {
        a == springboard ? "com.apple.springboard" : appId
    }

    /// `"windows":[...],"nodes":[...]`, and its hash: the read's seq (53 bits, a JSON number
    /// both ends read exactly).
    private func encoded(windows: [(String, Int)], _ w: Walk) -> ([UInt8], UInt64) {
        var out = JSONOut()
        out.raw("\"windows\":[")
        for (k, win) in windows.enumerated() {
            if k > 0 { out.raw(",") }
            out.raw("{\"pkg\":")
            out.string(win.0)
            out.raw(",\"type\":")
            out.int(win.1)
            out.raw("}")
        }
        out.raw("],\"nodes\":[")
        out.bytes.append(contentsOf: w.json.bytes)
        out.raw("]")
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        for b in out.bytes {
            h = (h ^ UInt64(b)) &* 0x100_0000_01b3
        }
        return (out.bytes, h & 0x1F_FFFF_FFFF_FFFF)
    }

    /// A reply written as it goes out: the read's JSON with its seq, and `extra` (`,"k":v`...).
    private func raw(_ read: Read, extra: String) -> [String: Any] {
        var body = Array("{\"ok\":true,".utf8)
        body.append(contentsOf: read.parts)
        body.append(contentsOf: Array(",\"seq\":\(read.seq),\"trimmed\":\(read.trimmed)\(extra)}".utf8))
        return ["ok": true, "raw": Data(body)]
    }

    /// One walk of a snapshot: its nodes as JSON, and (while marking) what the settle signature
    /// is made of.
    final class Walk {
        var json = JSONOut()
        var nodes = 0
        var marking: Bool
        var marks: [Mark] = []
        var seen: [Kind: Int] = [:]
        init(marking: Bool) { self.marking = marking }
    }

    /// `clip`: what the scroll views around the node show. iOS gives a node scrolled under a
    /// header, or out of its list, its whole frame: as on Android, a node shows only what its
    /// scroll views do, and one they hide is not visible.
    private func walk(_ s: XCUIElementSnapshot, depth: Int, window: Int, keyboard: Int, clip: CGRect = .infinite, inBar: Bool = false, into w: Walk) {
        let inKeyboard = window == keyboard || s.elementType == .keyboard
        node(s, depth: depth, window: inKeyboard ? keyboard : window, clip: clip, into: w)
        // A scroll bar fades for a second after every scroll: not a change to wait out.
        let bar = inBar || (w.marking && (s.label.hasSuffix("scroll bar") || s.label.contains("scroll bar,")))
        if w.marking, !bar { mark(s, into: w) }
        let inner = Handler.scrolling.contains(s.elementType) ? clip.intersection(s.frame) : clip
        for c in s.children {
            walk(c, depth: depth + 1, window: inKeyboard ? keyboard : window, keyboard: keyboard, clip: inner, inBar: bar, into: w)
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
    private static let checkables: Set<XCUIElement.ElementType> = [.switch, .toggle, .checkBox]

    /// A node as JSON: `{"d":depth,"w":window,"f":flags,"k":kind,"b":[l,t,r,b]}`, with its
    /// text (`t`), description (`c`), id (`i`) and placeholder (`h`) only when it has them.
    private func node(_ s: XCUIElementSnapshot, depth: Int, window: Int, clip: CGRect, into w: Walk) {
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
            if Handler.hasFocus(s) { flags |= Handler.focused }
        }
        if type == .secureTextField { flags |= Handler.password }
        if type == .switch { switchFrames.append(f) }
        if Handler.checkables.contains(type) {
            flags |= Handler.checkable
            if value == "1" { flags |= Handler.checked }
        }
        // React Native spells a checkbox's or radio button's role and state into its value
        // ("checkbox, checked", "radio button, unchecked"): UIKit has no trait for them.
        if let v = value, v.hasPrefix("checkbox, ") || v.hasPrefix("radio button, ") {
            let words = v.components(separatedBy: ", ")
            flags |= Handler.checkable
            if words[1] == "checked" { flags |= Handler.checked }
        }
        Handler.write(&w.json, comma: w.nodes > 0, depth: depth, window: window, flags: flags, kind: Handler.typeName(type), frame: f,
                      text: text, desc: desc, id: s.identifier, placeholder: placeholder)
        w.nodes += 1
    }

    /// A node as JSON, appended in place.
    private static func write(_ out: inout JSONOut, comma: Bool, depth: Int, window: Int, flags: Int, kind: String, frame f: CGRect,
                              text: String, desc: String, id: String, placeholder: String) {
        if comma { out.raw(",") }
        out.raw("{\"d\":")
        out.int(depth)
        out.raw(",\"w\":")
        out.int(window)
        out.raw(",\"f\":")
        out.int(flags)
        out.raw(",\"k\":")
        out.string(kind)
        out.raw(",\"b\":[")
        out.int(Int(f.minX.rounded()))
        out.raw(",")
        out.int(Int(f.minY.rounded()))
        out.raw(",")
        out.int(Int(f.maxX.rounded()))
        out.raw(",")
        out.int(Int(f.maxY.rounded()))
        out.raw("]")
        for (key, v) in [("t", text), ("c", desc), ("i", id), ("h", placeholder)] where !v.isEmpty {
            out.raw(",\"\(key)\":")
            out.string(v)
        }
        out.raw("}")
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
    /// change, each after XCTest's idle wait (that wait is the quiet), or `noChange` ms passed
    /// without one, or `timeout` ms in all. A touch sent without waiting for testmanagerd's
    /// answer has it before the settle ends.
    private func settle(noChange: Int, timeout: Int) -> [String: Any] {
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
        // When the screen last read differently: it holds still once it has read the same for
        // heldSeconds since. Snapshots of only the attributes a screen uses take tens of ms, and
        // two that close agree in the middle of a transition that moves nothing the
        // accessibility tree reports (a React Native screen sliding in takes no touch yet).
        var changedAt = Date()
        while true {
            let left = Double(timeout) / 1000 - Date().timeIntervalSince(start)
            let q = Date()
            let app = front()
            // Capped at 0.6 s: longer than UIKit's transitions (~0.35-0.5 s); what animates
            // longer (Settings' screens, after a push) is left to the signatures.
            if Quiescence.wait(app, animations: animations, timeout: max(min(0.6, left), 0.1)) { animations = false }
            let waited = Date().timeIntervalSince(q)
            idleMs += waited * 1000
            if waited < 0.05 { Thread.sleep(forTimeInterval: 0.05 - waited) }
            // A touch sent without waiting is played a little after it is sent: only a snapshot
            // taken once testmanagerd has confirmed it can end the settle (two taken before it
            // played would agree on the screen before the touch).
            let afterTouch = Touch.confirmed()
            let sn = Date()
            // The app in front as the idle wait found it, when that was a moment ago.
            let s = signature(of: waited < 0.02 ? app : nil)
            snapMs += Date().timeIntervalSince(sn) * 1000
            snaps += 1
            let now = Date()
            var held = false
            if s != last {
                changed = true
                last = s
                changedAt = now
            } else {
                held = changed && now.timeIntervalSince(changedAt) >= Handler.heldSeconds
            }
            let total = now.timeIntervalSince(start) * 1000
            if afterTouch && (!changed && total >= Double(noChange) || held) {
                try? Touch.awaitPending()
                // The screen it held still on, for the host's next read.
                settledSignature = last
                let extra = ",\"changed\":\(changed),\"timedOut\":false,\"idleMs\":\(Int(idleMs)),\"snapMs\":\(Int(snapMs)),\"snaps\":\(snaps)"
                if let (snapshot, app) = lastSnapshot {
                    switchFrames = []
                    let w = Walk(marking: false)
                    walk(snapshot, depth: 0, window: 0, keyboard: 1, into: w)
                    let (parts, seq) = encoded(windows: [(bundleId(app), 1), (bundleId(app), 2)], w)
                    return raw(Read(parts: parts, seq: seq, trimmed: lastTrimmed), extra: extra)
                }
                return ["ok": true, "changed": changed, "timedOut": false, "idleMs": Int(idleMs), "snapMs": Int(snapMs), "snaps": snaps]
            }
            if total >= Double(timeout) {
                try? Touch.awaitPending()
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
    /// How long a screen that changed must read the same to hold still.
    private static let heldSeconds = 0.2

    /// An element by what it is: its kind and id, alike ones told apart by their order.
    struct Kind: Hashable {
        let type: UInt
        let id: String
    }
    struct MarkID: Hashable {
        let kind: Kind
        let n: Int
    }
    /// An element and its state (what it says, where it is), hashed.
    struct Mark {
        let id: MarkID
        let state: Int
    }

    /// The last signature's elements' states, and for each element changing since, when it
    /// began and when it last changed.
    private var lastStates: [MarkID: Int] = [:]
    private var movingSince: [MarkID: Date] = [:], movedAt: [MarkID: Date] = [:]

    /// The last snapshot (a read's or a signature's), and the app it is of: the screen a
    /// settle ends on. And whether it fetched only the attributes a screen uses.
    private var lastSnapshot: (XCUIElementSnapshot, XCUIApplication)?
    private var lastTrimmed = false

    /// The signature of the last screen seen (a read's, or a settle's): what a settle compares
    /// with first, without a snapshot of its own. And the one the last settle ended on.
    private var lastSignature: Int?
    private var settledSignature: Int?

    /// A snapshot of the app in front (`app`, when the caller just found it) and its signature;
    /// `nil` when it could not be read.
    private func signature(of given: XCUIApplication? = nil) -> Int? {
        let app = given ?? front()
        guard let (s, trimmed) = try? Snap.take(app, pid: pid(of: app), full: false) else {
            lastSnapshot = nil
            return nil
        }
        lastSnapshot = (s, app)
        lastTrimmed = trimmed
        let w = Walk(marking: true)
        collect(s, inBar: false, into: w)
        let sig = digest(w.marks)
        lastSignature = sig
        return sig
    }

    /// The marks alone (no JSON), as `walk` makes them.
    private func collect(_ s: XCUIElementSnapshot, inBar: Bool, into w: Walk) {
        let bar = inBar || s.label.hasSuffix("scroll bar") || s.label.contains("scroll bar,")
        if !bar { mark(s, into: w) }
        for c in s.children {
            collect(c, inBar: bar, into: w)
        }
    }

    private func mark(_ s: XCUIElementSnapshot, into w: Walk) {
        let kind = Kind(type: s.elementType.rawValue, id: s.identifier)
        let n = w.seen[kind, default: 0]
        w.seen[kind] = n + 1
        let f = s.frame
        var h = Hasher()
        h.combine(s.label)
        h.combine("\(s.value ?? "")")
        h.combine(Int(f.minX))
        h.combine(Int(f.minY))
        h.combine(Int(f.width))
        h.combine(Int(f.height))
        w.marks.append(Mark(id: MarkID(kind: kind, n: n), state: h.finalize()))
    }

    private func digest(_ marks: [Mark]) -> Int {
        let now = Date()
        var states: [MarkID: Int] = [:], moving: [MarkID: Date] = [:], moved: [MarkID: Date] = [:]
        states.reserveCapacity(marks.count)
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
        var h = Hasher()
        for m in marks {
            h.combine(m.id)
            // Restless: changing for a while, and seen changing again since it began (an element
            // that changed once, between two settles, is not).
            if few, let since = moving[m.id], let at = moved[m.id], at > since, now.timeIntervalSince(since) >= Handler.restlessSeconds {
                h.combine(Int?.none)
                continue
            }
            h.combine(Int?.some(m.state))
        }
        return h.finalize()
    }
}

/// JSON written byte by byte: no dictionary per node, no JSONSerialization.
struct JSONOut {
    var bytes: [UInt8] = []
    private static let hex = Array("0123456789abcdef".utf8)

    mutating func raw(_ s: String) {
        bytes.append(contentsOf: s.utf8)
    }

    mutating func int(_ n: Int) {
        bytes.append(contentsOf: String(n).utf8)
    }

    mutating func string(_ s: String) {
        bytes.append(0x22)
        for b in s.utf8 {
            switch b {
            case 0x22: bytes.append(contentsOf: [0x5C, 0x22])
            case 0x5C: bytes.append(contentsOf: [0x5C, 0x5C])
            case 0x0A: bytes.append(contentsOf: [0x5C, 0x6E])
            case 0x0D: bytes.append(contentsOf: [0x5C, 0x72])
            case 0x09: bytes.append(contentsOf: [0x5C, 0x74])
            case 0..<0x20: bytes.append(contentsOf: [0x5C, 0x75, 0x30, 0x30, JSONOut.hex[Int(b >> 4)], JSONOut.hex[Int(b & 15)]])
            default: bytes.append(b)
            }
        }
        bytes.append(0x22)
    }
}

/// Snapshots of an app: only the attributes a screen and the settle signature use, through the
/// accessibility client WebDriverAgent asks (XCTest's own snapshot fetches every attribute of
/// every element), else XCTest's.
enum Snap {
    /// XCElementSnapshot's key paths the agent reads.
    private static let keyPaths: Set<String> = [
        "elementType", "identifier", "label", "value", "placeholderValue", "frame", "enabled", "selected", "hasKeyboardFocus",
    ]
    /// Reads that failed in a row: past a few, this Xcode does not have it, and XCTest's is used.
    private static var failures = 0

    /// The accessibility attributes for those key paths (XCElementSnapshot's mapping).
    private static let attributes = axAttributes(Array(keyPaths))
    /// The one for a field's value.
    private static let valueAttributes = axAttributes(["value"])

    private static func axAttributes(_ keyPaths: [String]) -> NSArray? {
        guard let cls = NSClassFromString("XCElementSnapshot") else { return nil }
        // Its mapping takes the key paths as an array, and gives an array or a set back.
        let paths = keyPaths as NSArray
        func array(_ o: AnyObject?) -> NSArray? { (o as? NSArray) ?? (o as? NSSet).map { $0.allObjects as NSArray } }
        let two = NSSelectorFromString("axAttributesForElementSnapshotKeyPaths:isMacOS:")
        if let m = class_getClassMethod(cls, two) {
            typealias Map = @convention(c) (AnyClass, Selector, NSArray, Bool) -> AnyObject?
            if let a = array(unsafeBitCast(method_getImplementation(m), to: Map.self)(cls, two, paths, false)) { return a }
        }
        let one = NSSelectorFromString("snapshotAttributesForElementSnapshotKeyPaths:")
        if let m = class_getClassMethod(cls, one) {
            typealias Map = @convention(c) (AnyClass, Selector, NSArray) -> AnyObject?
            if let a = array(unsafeBitCast(method_getImplementation(m), to: Map.self)(cls, one, paths)) { return a }
        }
        return nil
    }

    /// What an element (a field, from a snapshot of it) says now, from a snapshot of its value
    /// alone: `nil` when that cannot be had.
    static func value(of element: XCUIElementSnapshot) -> String?? {
        guard let valueAttributes, let ax = (element as AnyObject).value(forKey: "accessibilityElement") as? NSObject,
              let s = request(ax, attributes: valueAttributes)
        else { return nil }
        return .some(s.value as? String)
    }

    /// The app's snapshot, and whether it fetched only those attributes. `full`: XCTest's.
    static func take(_ app: XCUIApplication, pid: Int32?, full: Bool) throws -> (XCUIElementSnapshot, Bool) {
        if !full, failures < 3, let pid {
            if let s = trimmed(pid: pid) {
                failures = 0
                return (s, true)
            }
            failures += 1
        }
        return (try app.snapshot(), false)
    }

    private static func trimmed(pid: Int32) -> XCUIElementSnapshot? {
        guard let attributes, let cls = NSClassFromString("XCAccessibilityElement") else { return nil }
        let withPid = NSSelectorFromString("elementWithProcessIdentifier:")
        guard let make = class_getClassMethod(cls, withPid) else { return nil }
        typealias Make = @convention(c) (AnyClass, Selector, Int32) -> NSObject?
        guard let element = unsafeBitCast(method_getImplementation(make), to: Make.self)(cls, withPid, pid) else { return nil }
        return request(element, attributes: attributes)
    }

    /// The accessibility client's snapshot of an element, with only those attributes.
    private static func request(_ element: NSObject, attributes: NSArray) -> XCUIElementSnapshot? {
        guard let client = Handler.axClient() else { return nil }
        let request = NSSelectorFromString("requestSnapshotForElement:attributes:parameters:error:")
        guard client.responds(to: request) else { return nil }
        var parameters: NSDictionary = [:]
        let defaults = NSSelectorFromString("defaultParameters")
        if client.responds(to: defaults), let d = client.perform(defaults)?.takeUnretainedValue() as? NSDictionary { parameters = d }
        typealias Request = @convention(c) (NSObject, Selector, NSObject, NSArray, NSDictionary, AutoreleasingUnsafeMutablePointer<NSError?>?) -> NSObject?
        var error: NSError?
        guard let result = unsafeBitCast(client.method(for: request), to: Request.self)(client, request, element, attributes, parameters, &error) else { return nil }
        // A result object around the snapshot (newer Xcodes), or the snapshot itself.
        for key in ["rootElementSnapshot", "snapshot"] where result.responds(to: NSSelectorFromString(key)) {
            if let s = result.value(forKey: key) as? XCUIElementSnapshot { return s }
        }
        return result as? XCUIElementSnapshot
    }
}

/// Touches played by testmanagerd as XCTest's own are (XCPointerEventPath in an
/// XCSynthesizedEventRecord, as WebDriverAgent sends them), with their timing ours. Points are
/// the screen's, upright.
enum Touch {
    static let available: Bool =
        NSClassFromString("XCPointerEventPath") != nil && NSClassFromString("XCSynthesizedEventRecord") != nil
            && NSClassFromString("XCTRunnerDaemonSession") != nil

    /// `count` taps at a point, one every `interval` seconds, each held `hold`: one record with
    /// a finger per tap (a second press on one path does not land as a tap). Returns once
    /// testmanagerd has played them; or, `wait` false, 100 ms after the last lift (its answer,
    /// ~250 ms after that, is collected before the next touch and before a settle ends).
    static func taps(at p: CGPoint, count: Int, interval: TimeInterval, hold: TimeInterval, wait: Bool = true) throws {
        var paths: [NSObject] = []
        for k in 0..<max(count, 1) {
            let offset = Double(k) * interval
            let path = try touchPath(at: p, offset: offset)
            try call(path, "liftUpAtOffset:", offset + hold)
            paths.append(path)
        }
        let sent = Date()
        try synthesize(try record(with: paths), wait: wait)
        if !wait { rest(until: Double(max(count, 1) - 1) * interval + hold + 0.1, from: sent) }
    }

    /// Sleeps until `seconds` after `start`.
    private static func rest(until seconds: TimeInterval, from start: Date) {
        Thread.sleep(forTimeInterval: max(seconds - Date().timeIntervalSince(start), 0))
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
        // Waited for: an edge swipe back sent without waiting once did not take.
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
        // Back 20 ms after the lift, the content still (the tail is too slow to fling):
        // testmanagerd confirms ~250 ms after that, and its answer is collected before the next
        // touch and before a settle ends.
        let sent = Date()
        try synthesize(try record(with: [path]), wait: false)
        rest(until: kickS + fastS + tailS + 0.02, from: sent)
        return true
    }

    /// Text typed as key events into what has focus, `perSecond` characters a second.
    static func type(_ text: String, perSecond: Int, wait: Bool = true) throws {
        guard let cls = NSClassFromString("XCPointerEventPath") as? NSObject.Type else { throw AgentError("XCPointerEventPath is missing") }
        let alloc = cls.perform(NSSelectorFromString("alloc")).takeUnretainedValue() as! NSObject
        let initText = NSSelectorFromString("initForTextInput")
        guard alloc.responds(to: initText) else { throw AgentError("XCPointerEventPath.initForTextInput is missing") }
        let path = alloc.perform(initText).takeUnretainedValue() as! NSObject
        let sel = NSSelectorFromString("typeText:atOffset:typingSpeed:shouldRedact:")
        guard path.responds(to: sel) else { throw AgentError("XCPointerEventPath.typeText:atOffset:typingSpeed:shouldRedact: is missing") }
        typealias TypeText = @convention(c) (NSObject, Selector, NSString, TimeInterval, UInt64, Bool) -> Void
        unsafeBitCast(path.method(for: sel), to: TypeText.self)(path, sel, text as NSString, 0, UInt64(max(perSecond, 1)), false)
        try synthesize(try record(with: [path]), wait: wait)
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

    /// The touch last sent without waiting has been confirmed (its answer comes on the run
    /// loop, turned here once), or there is none.
    static func confirmed() -> Bool {
        guard let p = pending else { return true }
        if !p.done { RunLoop.current.run(mode: .default, before: Date()) }
        return p.done
    }

    static func awaitPending() throws {
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
