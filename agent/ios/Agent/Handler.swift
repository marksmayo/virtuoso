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

    private static let reads: Set<String> = ["ping", "info", "screen", "screenshot", "settle", "front"]

    /// Small replies that never change, written and framed once (Agent.swift sends `framed` as
    /// it is, no JSONSerialization); `ok` stays beside it for the check of an issue the request
    /// raised.
    private static func fixed(_ json: String) -> [String: Any] {
        let body = Array(json.utf8)
        let n = UInt32(body.count)
        let framed: [UInt8] = [UInt8(n >> 24), UInt8((n >> 16) & 0xFF), UInt8((n >> 8) & 0xFF), UInt8(n & 0xFF)] + body
        return ["ok": true, "framed": framed]
    }
    private static let okReply = fixed("{\"ok\":true}")
    private static let busyReply = fixed("{\"ok\":true,\"busy\":true}")
    private static let notBusyReply = fixed("{\"ok\":true,\"busy\":false}")

    private func dispatch(_ r: [String: Any]) throws -> ([String: Any], Data?) {
        let ok = Handler.okReply
        let op = r["op"] as? String ?? ""
        let action = !Handler.reads.contains(op)
        Cost.reset()
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
                // The app in front, as the read found it a moment ago.
                try tap(r, count: 1, in: read.app)
                return (touchedBusy ? Handler.busyReply : Handler.notBusyReply, nil)
            }
            try tap(r, count: 1)
            return (touchedBusy ? Handler.busyReply : Handler.notBusyReply, nil)
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
            // Its state every 10 ms: XCTest's wait for a state checks it more coarsely.
            let deadline = Date(timeIntervalSinceNow: Double(max(int(r, "timeout"), 1000)) / 1000)
            var up = a.state == .runningForeground
            while !up && Date() < deadline {
                Thread.sleep(forTimeInterval: 0.01)
                up = autoreleasepool(invoking: { a.state == .runningForeground })
            }
            app = a
            appId = id
            return (["ok": true, "foreground": up], nil)
        case "terminate":
            XCUIApplication(bundleIdentifier: r["app"] as? String ?? "").terminate()
            return (ok, nil)
        case "front":
            // The app in front by its bundle id, no snapshot: what openLink waits for.
            return (["ok": true, "app": bundleId(front())], nil)
        case "screenshot":
            let png = XCUIScreen.main.screenshot().pngRepresentation
            return (["ok": true, "blob": png.count], png)
        case "settle":
            return (settle(noChange: int(r, "nochange"), timeout: int(r, "timeout"), ifNot: seq(r, "ifNot")), nil)
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

    /// The app in front (findFront), its cost counted for the host's trace.
    private func front() -> XCUIApplication {
        let t = Date()
        defer { Cost.front += Date().timeIntervalSince(t) }
        return findFront()
    }

    private func findFront() -> XCUIApplication {
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
        // Another app (one a link opened): by its process. The app held is not in front
        // either way: when the other cannot be had, the home screen is read, not the app going
        // to the background (its tree can take XCTest 30 s a try, or the accessibility client
        // a minute, to give up on).
        guard let other = application(pid: active) else { return springboard }
        app = other
        appId = (other.responds(to: NSSelectorFromString("bundleID")) ? other.value(forKey: "bundleID") as? String : nil) ?? ""
        return other
    }

    /// The process in front, as WebDriverAgent finds it: the accessibility client's active
    /// applications (XCUIDevice's accessibility interface since Xcode 15; XCAXClient_iOS before).
    private static let activeSel = NSSelectorFromString("activeApplications")
    private static let processIdentifierSel = NSSelectorFromString("processIdentifier")
    private static let processIDSel = NSSelectorFromString("processID")
    /// A pid_t getter, called directly (no key-value boxing).
    private typealias PidGetter = @convention(c) (NSObject, Selector) -> Int32

    private func activePid() -> Int32? {
        let active = Handler.activeSel
        guard let client = Handler.axClient(), client.responds(to: active),
              let apps = client.perform(active)?.takeUnretainedValue() as? [NSObject] else { return nil }
        let pidKey = Handler.processIdentifierSel
        for a in apps where a.responds(to: pidKey) {
            return unsafeBitCast(a.method(for: pidKey), to: PidGetter.self)(a, pidKey)
        }
        return nil
    }

    /// XCUIApplication's class methods making an app from its process, as WebDriverAgent tries
    /// them (Xcode 16's XCUIApplication has neither).
    private static let withPidSels = ["appWithPID:", "applicationWithPID:"].map(NSSelectorFromString)

    /// An app by its process; else by the bundle id of the .app its process runs from.
    private func application(pid: Int32) -> XCUIApplication? {
        let cls: AnyClass = XCUIApplication.self
        typealias WithPid = @convention(c) (AnyClass, Selector, Int32) -> XCUIApplication?
        for sel in Handler.withPidSels {
            if let m = class_getClassMethod(cls, sel), let a = unsafeBitCast(method_getImplementation(m), to: WithPid.self)(cls, sel, pid) {
                return a
            }
        }
        return Handler.bundleID(pid: pid).map { XCUIApplication(bundleIdentifier: $0) }
    }

    /// proc_pidpath: not in the iOS SDK's headers, but in the simulator's libSystem.
    private typealias PidPath = @convention(c) (Int32, UnsafeMutableRawPointer?, UInt32) -> Int32
    private static let pidPath: PidPath? = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "proc_pidpath").map { unsafeBitCast($0, to: PidPath.self) }

    /// A process's bundle id: the one of the .app its executable is in.
    private static func bundleID(pid: Int32) -> String? {
        guard let pidPath else { return nil }
        var buf = [CChar](repeating: 0, count: 4096)
        let n = buf.withUnsafeMutableBytes { pidPath(pid, $0.baseAddress, UInt32($0.count)) }
        guard n > 0 else { return nil }
        var url = URL(fileURLWithPath: buf.withUnsafeBufferPointer { String(cString: $0.baseAddress!) })
        while url.pathComponents.count > 1 && url.pathExtension != "app" { url.deleteLastPathComponent() }
        guard url.pathExtension == "app" else { return nil }
        return Bundle(url: url)?.bundleIdentifier
    }

    private static let interfaceSel = NSSelectorFromString("accessibilityInterface")
    private static let sharedClientSel = NSSelectorFromString("sharedClient")
    private static let clientClass = NSClassFromString("XCAXClient_iOS") as? NSObject.Type

    static func axClient() -> NSObject? {
        let device = XCUIDevice.shared as NSObject, interface = interfaceSel
        if device.responds(to: interface), let c = device.perform(interface)?.takeUnretainedValue() as? NSObject { return c }
        let shared = sharedClientSel
        if let cls = clientClass, cls.responds(to: shared),
           let c = cls.perform(shared)?.takeUnretainedValue() as? NSObject { return c }
        return nil
    }

    private func pid(of a: XCUIApplication) -> Int32? {
        let sel = Handler.processIDSel
        guard a.responds(to: sel) else { return nil }
        return unsafeBitCast(a.method(for: sel), to: PidGetter.self)(a, sel)
    }

    private func point(_ r: [String: Any], _ kx: String, _ ky: String) -> XCUICoordinate {
        origin(r).withOffset(CGVector(dx: int(r, kx), dy: int(r, ky)))
    }

    /// A gesture's start and end (x1, y1 to x2, y2), from one look at the app in front.
    private func points(_ r: [String: Any]) -> (XCUICoordinate, XCUICoordinate) {
        let o = origin(r)
        return (o.withOffset(CGVector(dx: int(r, "x1"), dy: int(r, "y1"))), o.withOffset(CGVector(dx: int(r, "x2"), dy: int(r, "y2"))))
    }

    /// The top left of the app a request's touch goes to.
    private func origin(_ r: [String: Any]) -> XCUICoordinate {
        // A system gesture (from the screen's edge) is SpringBoard's to take.
        let base = (r["system"] as? Bool ?? false) ? springboard : front()
        return base.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
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
    private func tap(_ r: [String: Any], count: Int, in found: XCUIApplication? = nil) throws {
        let p = CGPoint(x: int(r, "x"), y: int(r, "y"))
        let system = r["system"] as? Bool ?? false
        touchedBusy = false
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
        // A single tap needs the app in front (SpringBoard's touches are held longer); repeated
        // taps only when they wait for it to idle.
        let f = found ?? (count == 1 ? front() : nil)
        touchedBusy = restBeforeTouch(f ?? front())
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
    /// The last settle ending while the app still animated (an endless spinner holds XCTest's
    /// idle wait to its cap, so a popup presenting meanwhile reads the same twice) counts as
    /// such a read too. Whether the animations were still going at the cap. `f`, the app in
    /// front, is looked up only when there is a wait (the screen is usually the settled one).
    @discardableResult
    private func restBeforeTouch(_ f: @autoclosure () -> XCUIApplication) -> Bool {
        guard lastSignature == nil || lastSignature != settledSignature || settledBusy else { return false }
        let q = Date()
        let busy = Quiescence.wait(f(), animations: true, timeout: 0.3)
        if Date().timeIntervalSince(q) > 0.03 { Thread.sleep(forTimeInterval: 0.15) }
        return busy
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
        // The field with focus, first in document order as XCTest's query finds it, from a
        // snapshot of only the attributes a screen uses (XCTest's query snapshots the whole
        // app with every attribute); the query where that cannot be had.
        let app = front()
        var query: XCUIElement { app.descendants(matching: .any).matching(NSPredicate(format: "hasKeyboardFocus == true")).firstMatch }
        let snapshot: XCUIElementSnapshot?
        if let (s, _) = try? Snap.take(app, pid: pid(of: app), full: false) {
            snapshot = Handler.firstFocused(in: s)
        } else {
            snapshot = try? query.snapshot()
        }
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
        let ax = snapshot.flatMap { Snap.element(of: $0) }
        let says: () -> String? = {
            if let ax, let v = Snap.value(of: ax) { return v }
            return query.value as? String
        }
        let deadline = Date(timeIntervalSinceNow: 0.5)
        while Date() < deadline, autoreleasepool(invoking: says) == before {
            Thread.sleep(forTimeInterval: 0.01)
        }
        try Touch.type(rest, perSecond: 100, wait: false)
    }

    /// The first element in document order with keyboard focus.
    private static func firstFocused(in s: XCUIElementSnapshot) -> XCUIElementSnapshot? {
        if hasFocus(s) { return s }
        for c in s.children {
            if let f = firstFocused(in: c) { return f }
        }
        return nil
    }

    /// XCElementSnapshot's hasKeyboardFocus.
    private static func hasFocus(_ s: XCUIElementSnapshot) -> Bool {
        guard let o = s as AnyObject as? NSObject, o.responds(to: focusSelector) else { return false }
        typealias Getter = @convention(c) (NSObject, Selector) -> Bool
        return unsafeBitCast(o.method(for: focusSelector), to: Getter.self)(o, focusSelector)
    }
    private static let focusSelector = NSSelectorFromString("hasKeyboardFocus")

    private func swipe(_ r: [String: Any]) {
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
        // XCTest's coordinates: for this fallback alone.
        let (from, to) = points(r)
        from.press(forDuration: 0.05, thenDragTo: to, withVelocity: XCUIGestureVelocity(CGFloat(speed)), thenHoldForDuration: hold)
    }

    /// Presses `hold` ms so the item lifts, moves to the end over `move` ms, and lets go.
    private func drag(_ r: [String: Any]) {
        let (from, to) = points(r)
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
    /// scroll view that dismisses interactively takes it), then the keyboard's own key, then a
    /// short swipe up and one left mid-screen (a touch beside the field closes React Native's,
    /// keyboardShouldPersistTaps="never", its default). Still up, it is left (Settings' search
    /// field keeps its keyboard whatever is tried, and a flow goes on fine).
    private func hideKeyboard() {
        let a = front()
        let kb = a.keyboards.firstMatch
        // The keyboard as the last look at the app found it: from a snapshot of only the
        // attributes a screen uses where that can be had (XCTest's query snapshots the whole
        // app with every attribute), XCTest's where it found none.
        var shown = keyboard(in: a).keyboard
        guard let f = (shown ?? (try? kb.snapshot()))?.frame else { return }
        let origin = a.coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
        origin.withOffset(CGVector(dx: f.midX, dy: f.minY - 30))
            .press(forDuration: 0.05, thenDragTo: origin.withOffset(CGVector(dx: f.midX, dy: f.maxY - 5)))
        if gone(a, kb, shown: &shown) { return }
        // The keys it has, from one snapshot of it (an `exists` query each was a snapshot each),
        // read again after a key that did not close it: the one gone() last took, else XCTest's.
        var present = shown.map(Handler.buttonNames(under:)) ?? Handler.buttonNames(in: kb)
        for name in ["Hide keyboard", "Done", "done", "Return", "return"] where present.contains(name) {
            kb.buttons[name].tap()
            if gone(a, kb, shown: &shown) { return }
            present = shown.map(Handler.buttonNames(under:)) ?? Handler.buttonNames(in: kb)
        }
        for to in [CGVector(dx: 0.5, dy: 0.47), CGVector(dx: 0.47, dy: 0.5)] {
            a.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))
                .press(forDuration: 0.05, thenDragTo: a.coordinate(withNormalizedOffset: to))
            if gone(a, kb, shown: &shown) { return }
        }
    }

    /// The first keyboard in `a`'s tree in document order (what `keyboards.firstMatch` finds),
    /// from a snapshot of only the attributes a screen uses (`trimmed`), else XCTest's; `read`
    /// false when neither can be had.
    private func keyboard(in a: XCUIApplication) -> (read: Bool, trimmed: Bool, keyboard: XCUIElementSnapshot?) {
        guard let (s, trimmed) = try? Snap.take(a, pid: pid(of: a), full: false) else { return (false, false, nil) }
        return (true, trimmed, Handler.firstKeyboard(in: s))
    }

    private static func firstKeyboard(in s: XCUIElementSnapshot) -> XCUIElementSnapshot? {
        if s.elementType == .keyboard { return s }
        for c in s.children {
            if let k = firstKeyboard(in: c) { return k }
        }
        return nil
    }

    /// The identifiers and labels of the buttons in `e`, as `buttons[name]` matches them.
    private static func buttonNames(in e: XCUIElement) -> Set<String> {
        guard let s = try? e.snapshot() else { return [] }
        return buttonNames(under: s)
    }

    /// The identifiers and labels of the buttons in a snapshot of a keyboard.
    private static func buttonNames(under s: XCUIElementSnapshot) -> Set<String> {
        var names = Set<String>()
        func visit(_ n: XCUIElementSnapshot) {
            if n.elementType == .button {
                names.insert(n.identifier)
                names.insert(n.label)
            }
            n.children.forEach(visit)
        }
        visit(s)
        return names
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

    /// The keyboard `e` (`a`'s first) is gone within a second: looked for every 10 ms in
    /// snapshots of only the attributes a screen uses (`exists` is a snapshot of the whole app
    /// with every attribute), else every 30 ms (in XCTest's snapshot, or with `exists`).
    /// `shown`: the keyboard as the last look found it (`nil` when it was `exists`).
    private func gone(_ a: XCUIApplication, _ e: XCUIElement, shown: inout XCUIElementSnapshot?) -> Bool {
        let deadline = Date(timeIntervalSinceNow: 1)
        while true {
            let look = autoreleasepool(invoking: { keyboard(in: a) })
            if look.read {
                shown = look.keyboard
                if look.keyboard == nil { return true }
            } else {
                shown = nil
                if !autoreleasepool(invoking: { e.exists }) { return true }
            }
            if Date() >= deadline { return false }
            Thread.sleep(forTimeInterval: look.trimmed ? 0.01 : 0.03)
        }
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
        /// The reply so far: room for its frame's length, `{"ok":true,`, the windows, the nodes.
        var bytes: [UInt8]
        let seq: UInt64
        let trimmed: Bool
        /// The app read.
        let app: XCUIApplication
    }

    /// One app's whole tree, in front or not (the home screen's, a system prompt's).
    private func screen(of id: String) throws -> [String: Any] {
        switchFrames.removeAll(keepingCapacity: true)
        let a = id == "com.apple.springboard" ? springboard : XCUIApplication(bundleIdentifier: id)
        var w = startRead(windows: [(id, 1), (id, 2)])
        walk(try a.snapshot(), depth: 0, window: 0, keyboard: 1, into: &w)
        return raw(finish(w, trimmed: false, app: a), extra: "")
    }

    private func readFront(full: Bool) throws -> Read {
        switchFrames.removeAll(keepingCapacity: true)
        let target = front()
        var windows: [(String, Int)] = []
        // Without the accessibility client's word on what is in front, a system alert is
        // looked for over the app (with it, an alert makes SpringBoard the front app).
        var alert: XCUIElementSnapshot? = nil
        if !frontKnown, target != springboard, springboard.alerts.firstMatch.exists {
            windows.append(("com.apple.springboard", 1))
            alert = try springboard.alerts.firstMatch.snapshot()
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
        let id = bundleId(app)
        windows.append((id, 1))
        windows.append((id, 2))
        let (snapshot, trimmed) = taken!
        var w = startRead(windows: windows)
        let walked = Date()
        if let alert { walk(alert, depth: 0, window: 0, keyboard: 0, into: &w) }
        w.mark()
        walk(snapshot, depth: 0, window: k, keyboard: k + 1, into: &w)
        Cost.walk += Date().timeIntervalSince(walked)
        // What the host now sees: what the next settle starts from.
        lastSignature = digest(w.marks)
        // Not kept: a settle ends on a snapshot of its own (signature), never on this one.
        return finish(w, trimmed: trimmed, app: app)
    }

    private func bundleId(_ a: XCUIApplication) -> String {
        a == springboard ? "com.apple.springboard" : appId
    }

    /// A reply's start: room for its frame's length, then `{"ok":true,`.
    private static let replyHead: [UInt8] = [0, 0, 0, 0] + Array("{\"ok\":true,".utf8)
    /// The size of the last read's reply: the next one's buffer starts that big.
    private static var lastReplySize = 1 << 16

    /// A walk writing a read's reply as it goes: its start, `"windows":[...]`, `"nodes":[`.
    private func startRead(windows: [(String, Int)]) -> Walk {
        var w = Walk(marking: false)
        w.json.bytes.reserveCapacity(Handler.lastReplySize)
        w.json.bytes.append(contentsOf: Handler.replyHead)
        w.hashFrom = w.json.bytes.count
        w.json.raw("\"windows\":[")
        for (k, win) in windows.enumerated() {
            if k > 0 { w.json.raw(",") }
            w.json.raw("{\"pkg\":")
            w.json.string(win.0)
            w.json.raw(",\"type\":")
            w.json.int(win.1)
            w.json.raw("}")
        }
        w.json.raw("],\"nodes\":[")
        return w
    }

    /// The read the walk wrote, closed, with its hash: the read's seq (53 bits, a JSON number
    /// both ends read exactly) over the windows and nodes.
    private func finish(_ w: consuming Walk, trimmed: Bool, app: XCUIApplication) -> Read {
        w.json.raw("]")
        Handler.lastReplySize = w.json.bytes.count + 512
        return Read(bytes: w.json.bytes, seq: Handler.seqHash(w.json.bytes, from: w.hashFrom), trimmed: trimmed, app: app)
    }

    /// FNV-1a over 8 bytes at a time, mixed at the end, its low 53 bits.
    static func seqHash(_ bytes: [UInt8], from start: Int) -> UInt64 {
        var h: UInt64 = 0xcbf2_9ce4_8422_2325
        bytes.withUnsafeBytes { p in
            var i = start
            while i + 8 <= p.count {
                h = (h ^ p.loadUnaligned(fromByteOffset: i, as: UInt64.self)) &* 0x100_0000_01b3
                i += 8
            }
            while i < p.count {
                h = (h ^ UInt64(p[i])) &* 0x100_0000_01b3
                i += 1
            }
        }
        // Each step keeps every bit; the mix brings the high ones into the 53 kept.
        h ^= h >> 32
        h = h &* 0x9E37_79B9_7F4A_7C15
        h ^= h >> 29
        return h & 0x1F_FFFF_FFFF_FFFF
    }

    /// A reply written as it goes out: the read's JSON with its seq, and `extra` (`,"k":v`...),
    /// framed in place (its length in the room left for it).
    private func raw(_ read: consuming Read, extra: String) -> [String: Any] {
        var body = read.bytes
        body.append(contentsOf: ",\"seq\":\(read.seq),\"trimmed\":\(read.trimmed),\"trace\":\"\(Cost.text)\"\(extra)}".utf8)
        let n = UInt32(body.count - 4)
        body[0] = UInt8(n >> 24)
        body[1] = UInt8((n >> 16) & 0xFF)
        body[2] = UInt8((n >> 8) & 0xFF)
        body[3] = UInt8(n & 0xFF)
        return ["ok": true, "framed": body]
    }

    /// One walk of a snapshot: its nodes as JSON, and (while marking) what the settle signature
    /// is made of.
    struct Walk {
        var json = JSONOut()
        /// Where the read's hash starts (after the reply's head).
        var hashFrom = 0
        var nodes = 0
        var marking = false
        var marks: [Mark] = []
        var seen: [Kind: Int] = [:]
        /// Room for the marks only in a walk that makes them (a read's JSON alone needs none).
        init(marking: Bool) {
            if marking { mark() }
        }
        mutating func mark() {
            marking = true
            marks.reserveCapacity(Handler.lastMarks)
            seen.reserveCapacity(Handler.lastMarks)
        }
    }
    /// How many elements the last walk marked: the next one's room.
    static var lastMarks = 256

    /// The attributes a walk uses of a node, each read once (each read is an Objective-C call).
    struct Attrs {
        let type: XCUIElement.ElementType
        let label: String
        /// What it says as text: a String as it is, a number as its text; `nil` for anything else.
        let value: String?
        /// A value of any other kind, for the signature alone.
        let other: Any?
        let identifier: String
        let frame: CGRect
        init(_ s: XCUIElementSnapshot) {
            type = s.elementType
            label = s.label
            // Cast once here, not again by each reader.
            let v = s.value
            if let t = v as? String {
                value = t
                other = nil
            } else if let n = v as? NSNumber {
                value = n.stringValue
                other = nil
            } else {
                value = nil
                other = v
            }
            identifier = s.identifier
            frame = s.frame
        }
    }

    /// A label that ends or lists a scroll bar ("Vertical scroll bar, 2 pages"): at least that long.
    private static func isScrollBar(_ label: String) -> Bool {
        label.utf8.count >= 10 && (label.hasSuffix("scroll bar") || label.contains("scroll bar,"))
    }

    /// `clip`: what the scroll views around the node show. iOS gives a node scrolled under a
    /// header, or out of its list, its whole frame: as on Android, a node shows only what its
    /// scroll views do, and one they hide is not visible.
    private func walk(_ s: XCUIElementSnapshot, depth: Int, window: Int, keyboard: Int, clip: CGRect = .infinite, inBar: Bool = false, into w: inout Walk) {
        let a = Attrs(s)
        let inKeyboard = window == keyboard || a.type == .keyboard
        node(s, a, depth: depth, window: inKeyboard ? keyboard : window, clip: clip, into: &w)
        // A scroll bar fades for a second after every scroll: not a change to wait out.
        let bar = inBar || (w.marking && Handler.isScrollBar(a.label))
        if w.marking, !bar { mark(a, into: &w) }
        let inner = Handler.has(a.type, Handler.isScrolling) ? clip.intersection(a.frame) : clip
        for c in s.children {
            walk(c, depth: depth + 1, window: inKeyboard ? keyboard : window, keyboard: keyboard, clip: inner, inBar: bar, into: &w)
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

    /// Those sets as bits per element type, looked up once a node.
    private static let isTextInput: UInt8 = 1, isTappable: UInt8 = 2, isScrolling: UInt8 = 4, isCheckable: UInt8 = 8
    private static let kindBits: [UInt8] = {
        var bits = [UInt8](repeating: 0, count: 128)
        for (set, bit) in [(textInputs, isTextInput), (tappable, isTappable), (scrolling, isScrolling), (checkables, isCheckable)] {
            for t in set where Int(t.rawValue) < bits.count { bits[Int(t.rawValue)] |= bit }
        }
        return bits
    }()
    @inline(__always) private static func has(_ t: XCUIElement.ElementType, _ bit: UInt8) -> Bool {
        let i = Int(t.rawValue)
        return i < kindBits.count ? kindBits[i] & bit != 0 : false
    }

    /// A node as JSON: `{"d":depth,"w":window,"f":flags,"k":kind,"b":[l,t,r,b]}`, with its
    /// text (`t`), description (`c`), id (`i`) and placeholder (`h`) only when it has them.
    private func node(_ s: XCUIElementSnapshot, _ a: Attrs, depth: Int, window: Int, clip: CGRect, into w: inout Walk) {
        // What shows of it; all of it where nothing clips it.
        let shown = clip.isInfinite ? a.frame : a.frame.intersection(clip)
        let f = shown.isNull || shown.width <= 0 || shown.height <= 0 ? CGRect(origin: a.frame.origin, size: .zero) : shown
        let type = a.type
        let value = a.value
        let placeholder = s.placeholderValue ?? ""
        let input = Handler.has(type, Handler.isTextInput)
        var text = a.label, desc = ""
        if type == .application {
            // The app's name, not text on screen: Android's root node has none either, and a
            // flow's tapOn would take the whole screen for an element named like the app.
            text = ""
        } else if input {
            // A field's text is what is typed in it; its label describes it.
            text = (value != nil && value != placeholder) ? value! : ""
            desc = a.label
        } else if text.isEmpty, type == .staticText {
            text = value ?? ""
        }
        var flags = 0
        if f.width > 0, f.height > 0 { flags |= Handler.visible }
        if s.isEnabled { flags |= Handler.enabled }
        if s.isSelected { flags |= Handler.selected }
        if Handler.has(type, Handler.isTappable) { flags |= Handler.clickable | Handler.longClickable }
        if Handler.has(type, Handler.isScrolling) { flags |= Handler.scrollable }
        if input {
            flags |= Handler.editable
            // The field typing goes to (XCElementSnapshot's hasKeyboardFocus).
            if Handler.hasFocus(s) { flags |= Handler.focused }
        }
        if type == .secureTextField { flags |= Handler.password }
        if type == .switch { switchFrames.append(f) }
        if Handler.has(type, Handler.isCheckable) {
            flags |= Handler.checkable
            if value == "1" { flags |= Handler.checked }
        }
        // React Native spells a checkbox's or radio button's role and state into its value
        // ("checkbox, checked", "radio button, unchecked"): UIKit has no trait for them.
        if let v = value, let c = v.utf8.first, c == UInt8(ascii: "c") || c == UInt8(ascii: "r"),
           v.hasPrefix("checkbox, ") || v.hasPrefix("radio button, ") {
            let words = v.components(separatedBy: ", ")
            flags |= Handler.checkable
            if words[1] == "checked" { flags |= Handler.checked }
        }
        Handler.write(&w.json, comma: w.nodes > 0, depth: depth, window: window, flags: flags, kind: Handler.quotedTypeName(type), frame: f,
                      text: text, desc: desc, id: a.identifier, placeholder: placeholder)
        w.nodes += 1
    }

    /// A node as JSON, appended in place.
    private static func write(_ out: inout JSONOut, comma: Bool, depth: Int, window: Int, flags: Int, kind: [UInt8], frame f: CGRect,
                              text: String, desc: String, id: String, placeholder: String) {
        if comma { out.raw(",") }
        out.raw("{\"d\":")
        out.int(depth)
        out.raw(",\"w\":")
        out.int(window)
        out.raw(",\"f\":")
        out.int(flags)
        out.raw(",\"k\":")
        out.bytes.append(contentsOf: kind)
        out.raw(",\"b\":[")
        out.int(Int(f.minX.rounded()))
        out.raw(",")
        out.int(Int(f.minY.rounded()))
        out.raw(",")
        out.int(Int(f.maxX.rounded()))
        out.raw(",")
        out.int(Int(f.maxY.rounded()))
        out.raw("]")
        if !text.isEmpty {
            out.raw(",\"t\":")
            out.string(text)
        }
        if !desc.isEmpty {
            out.raw(",\"c\":")
            out.string(desc)
        }
        if !id.isEmpty {
            out.raw(",\"i\":")
            out.string(id)
        }
        if !placeholder.isEmpty {
            out.raw(",\"h\":")
            out.string(placeholder)
        }
        out.raw("}")
    }

    /// Element types' names as JSON strings, by raw value: every type XCTest has, written once
    /// (no hashing per node).
    private static let quotedNameTable: [[UInt8]] = (0..<UInt(128)).map { raw -> [UInt8] in
        var out = JSONOut()
        out.string(XCUIElement.ElementType(rawValue: raw).map(Handler.typeName) ?? "Element\(raw)")
        return out.bytes
    }
    /// An element type's name as a JSON string; past the table, written once per type.
    private static var quotedNames: [UInt: [UInt8]] = [:]
    private static func quotedTypeName(_ t: XCUIElement.ElementType) -> [UInt8] {
        let i = Int(t.rawValue)
        if i < quotedNameTable.count { return quotedNameTable[i] }
        if let q = quotedNames[t.rawValue] { return q }
        var out = JSONOut()
        out.string(typeName(t))
        quotedNames[t.rawValue] = out.bytes
        return out.bytes
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
    /// answer has it before the settle ends. `ifNot`: the host's screen (its read's hash); one
    /// that ends on that same screen sends no tree.
    private func settle(noChange: Int, timeout: Int, ifNot: UInt64?) -> [String: Any] {
        let start = Date()
        // From the screen last seen (the host's read before the action, or the last settle's
        // end): a snapshot of its own would cost as much again.
        var last = lastSignature ?? signature(), changed = false
        // XCTest's idle wait first, each time: a modal presenting takes no touch while it
        // animates in, and a fade moves no frame. An animation that never ends would hold it:
        // once a wait takes its whole cap, the rest wait for the event loop only.
        var animations = !lastWasSwipe
        // Whether an idle wait for the animations took its whole cap: they still went on.
        var busy = false
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
            // Each iteration's autoreleased garbage (XCTest's, the accessibility client's) goes
            // with it, not at the end of a settle that can take hundreds of snapshots.
            if autoreleasepool(invoking: { Quiescence.wait(app, animations: animations, timeout: max(min(0.6, left), 0.1)) }) {
                if animations { busy = true }
                animations = false
            }
            let waited = Date().timeIntervalSince(q)
            idleMs += waited * 1000
            if waited < 0.05 {
                // The next snapshot 50 ms after the wait began, or at the moment the screen will
                // have held still long enough (heldSeconds after a change, noChange ms without
                // one) when that comes sooner: only a snapshot ends the settle, and one landing
                // past that moment ends it up to a period late. Not before it: a snapshot taken
                // just before would not end the settle either.
                let due = changed ? changedAt.addingTimeInterval(Handler.heldSeconds) : start.addingTimeInterval(Double(noChange) / 1000)
                let untilDue = due.timeIntervalSinceNow
                Thread.sleep(forTimeInterval: untilDue > 0 ? min(0.05 - waited, untilDue) : 0.05 - waited)
            }
            // A touch sent without waiting is played a little after it is sent: only a snapshot
            // taken once testmanagerd has confirmed it can end the settle (two taken before it
            // played would agree on the screen before the touch).
            let afterTouch = Touch.confirmed()
            let sn = Date()
            // The app in front as the idle wait found it, when that was a moment ago.
            let s = autoreleasepool(invoking: { signature(of: waited < 0.02 ? app : nil) })
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
                settledBusy = busy
                let extra = ",\"changed\":\(changed),\"timedOut\":false,\"idleMs\":\(Int(idleMs)),\"snapMs\":\(Int(snapMs)),\"snaps\":\(snaps)"
                if let (snapshot, app) = lastSnapshot {
                    // Not held after this reply: nothing reads it before the next settle's own.
                    lastSnapshot = nil
                    switchFrames.removeAll(keepingCapacity: true)
                    let id = bundleId(app)
                    var w = startRead(windows: [(id, 1), (id, 2)])
                    walk(snapshot, depth: 0, window: 0, keyboard: 1, into: &w)
                    let read = finish(w, trimmed: lastTrimmed, app: app)
                    // The host holds this screen already: it keeps its own.
                    if let was = ifNot, was == read.seq {
                        return ["ok": true, "unchanged": true, "seq": NSNumber(value: was), "changed": changed, "timedOut": false, "idleMs": Int(idleMs), "snapMs": Int(snapMs), "snaps": snaps, "trace": Cost.text]
                    }
                    return raw(read, extra: extra)
                }
                return ["ok": true, "changed": changed, "timedOut": false, "idleMs": Int(idleMs), "snapMs": Int(snapMs), "snaps": snaps, "trace": Cost.text]
            }
            if total >= Double(timeout) {
                try? Touch.awaitPending()
                lastSnapshot = nil
                return ["ok": true, "changed": changed, "timedOut": true, "trace": Cost.text]
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
        /// Hashed once: a mark looks it up in `seen` and hashes it into its MarkID.
        private let h: Int
        init(type: UInt, id: String) {
            self.type = type
            self.id = id
            var hasher = Hasher()
            hasher.combine(type)
            hasher.combine(id)
            h = hasher.finalize()
        }
        static func == (a: Kind, b: Kind) -> Bool { a.h == b.h && a.type == b.type && a.id == b.id }
        func hash(into hasher: inout Hasher) { hasher.combine(h) }
    }
    struct MarkID: Hashable {
        let kind: Kind
        let n: Int
        /// Hashed once: a digest looks it up in several tables.
        private let h: Int
        init(kind: Kind, n: Int) {
            self.kind = kind
            self.n = n
            var hasher = Hasher()
            hasher.combine(kind)
            hasher.combine(n)
            h = hasher.finalize()
        }
        static func == (a: MarkID, b: MarkID) -> Bool { a.h == b.h && a.n == b.n && a.kind == b.kind }
        func hash(into hasher: inout Hasher) { hasher.combine(h) }
    }
    /// An element and its state (what it says, where it is), hashed.
    struct Mark {
        let id: MarkID
        let state: Int
    }

    /// The last signature's elements and their states, and for each element changing since,
    /// when it began and when it last changed.
    private var lastMarkList: [Mark] = []
    private var movingSince: [MarkID: Date] = [:], movedAt: [MarkID: Date] = [:]

    /// The last signature's snapshot, and the app it is of: the screen a settle ends on (let go
    /// once it has). And whether it fetched only the attributes a screen uses.
    private var lastSnapshot: (XCUIElementSnapshot, XCUIApplication)?
    private var lastTrimmed = false

    /// The signature of the last screen seen (a read's, or a settle's): what a settle compares
    /// with first, without a snapshot of its own. And the one the last settle ended on.
    private var lastSignature: Int?
    private var settledSignature: Int?
    /// The last settle ended while the app still animated.
    private var settledBusy = false
    /// The last tap went in while the app still animated (restBeforeTouch's wait hit its cap).
    private var touchedBusy = false

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
        var w = Walk(marking: true)
        let walked = Date()
        collect(s, inBar: false, into: &w)
        let sig = digest(w.marks)
        Cost.walk += Date().timeIntervalSince(walked)
        lastSignature = sig
        return sig
    }

    /// The marks alone (no JSON), as `walk` makes them.
    private func collect(_ s: XCUIElementSnapshot, inBar: Bool, into w: inout Walk) {
        let a = Attrs(s)
        let bar = inBar || Handler.isScrollBar(a.label)
        if !bar { mark(a, into: &w) }
        for c in s.children {
            collect(c, inBar: bar, into: &w)
        }
    }

    private func mark(_ a: Attrs, into w: inout Walk) {
        let kind = Kind(type: a.type.rawValue, id: a.identifier)
        let n = Handler.bump(&w.seen[kind, default: 0])
        let f = a.frame
        var h = Hasher()
        h.combine(a.label)
        // What it says, as text (a String as it is, a number as its text): signatures are
        // compared only with each other.
        if let v = a.value {
            h.combine(v)
        } else if let v = a.other {
            h.combine("\(v)")
        } else {
            h.combine("")
        }
        h.combine(Int(f.minX))
        h.combine(Int(f.minY))
        h.combine(Int(f.width))
        h.combine(Int(f.height))
        w.marks.append(Mark(id: MarkID(kind: kind, n: n), state: h.finalize()))
    }

    /// The count before, counted one more.
    @inline(__always) private static func bump(_ c: inout Int) -> Int {
        defer { c += 1 }
        return c
    }

    private func digest(_ marks: [Mark]) -> Int {
        let now = Date()
        Handler.lastMarks = max(marks.count, 16)
        var moving: [MarkID: Date] = [:], moved: [MarkID: Date] = [:]
        // The last marks' states by element: made only once a mark is not where it was.
        var lastStates: [MarkID: Int]? = nil
        // Nothing was changing (a still screen): no table to look in.
        let wasMoving = !movingSince.isEmpty
        for (i, m) in marks.enumerated() {
            // Where it was last time, as it usually is: no table lookup.
            let was: Int?
            if i < lastMarkList.count && lastMarkList[i].id == m.id {
                was = lastMarkList[i].state
            } else {
                if lastStates == nil {
                    lastStates = Dictionary(lastMarkList.map { ($0.id, $0.state) }, uniquingKeysWith: { _, last in last })
                }
                was = lastStates?[m.id]
            }
            if let was, was != m.state {
                moving[m.id] = (wasMoving ? movingSince[m.id] : nil) ?? now
                moved[m.id] = now
            } else if wasMoving, let since = movingSince[m.id], let at = movedAt[m.id], now.timeIntervalSince(at) < Handler.restlessPauseSeconds {
                moving[m.id] = since
                moved[m.id] = at
            }
        }
        lastMarkList = marks
        movingSince = moving
        movedAt = moved
        // Nothing changing: no table to look in for each mark.
        let few = !moving.isEmpty && moving.count * 3 <= marks.count
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

    /// A literal's bytes, appended at once (a short String's utf8 is appended byte by byte).
    mutating func raw(_ s: StaticString) {
        s.withUTF8Buffer { bytes.append(contentsOf: $0) }
    }

    mutating func int(_ n: Int) {
        if n < 0 { bytes.append(0x2D) }
        var m = n.magnitude
        if m < 10 {
            bytes.append(0x30 + UInt8(m))
            return
        }
        var digits: (UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8,
                     UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8, UInt8) =
            (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0)
        var k = 0
        withUnsafeMutableBytes(of: &digits) { d in
            while m > 0 {
                d[k] = 0x30 + UInt8(m % 10)
                m /= 10
                k += 1
            }
            while k > 0 {
                k -= 1
                bytes.append(d[k])
            }
        }
    }

    mutating func string(_ s: String) {
        bytes.append(0x22)
        var s = s
        s.withUTF8 { p in
            // Runs that need no escape go in whole.
            var run = 0
            for i in 0..<p.count {
                let b = p[i]
                if b >= 0x20 && b != 0x22 && b != 0x5C { continue }
                if i > run { bytes.append(contentsOf: UnsafeBufferPointer(rebasing: p[run..<i])) }
                escape(b)
                run = i + 1
            }
            if p.count > run { bytes.append(contentsOf: UnsafeBufferPointer(rebasing: p[run..<p.count])) }
        }
        bytes.append(0x22)
    }

    private mutating func escape(_ b: UInt8) {
        bytes.append(0x5C)
        switch b {
        case 0x22: bytes.append(0x22)
        case 0x5C: bytes.append(0x5C)
        case 0x0A: bytes.append(0x6E)
        case 0x0D: bytes.append(0x72)
        case 0x09: bytes.append(0x74)
        default:
            bytes.append(0x75)
            bytes.append(0x30)
            bytes.append(0x30)
            bytes.append(JSONOut.hex[Int(b >> 4)])
            bytes.append(JSONOut.hex[Int(b & 15)])
        }
    }
}

/// Where a request's time went, for the host's trace (VIRTUOSO_IOS_TRACE): finding the app in
/// front, its snapshots (how many fetched only the attributes a screen uses), and walking them.
enum Cost {
    static var front = 0.0, take = 0.0, walk = 0.0, takes = 0, trimmed = 0

    static func reset() {
        (front, take, walk, takes, trimmed) = (0, 0, 0, 0, 0)
    }

    static var text: String {
        "front \(Int(front * 1000)) ms, \(takes) snapshots \(Int(take * 1000)) ms (\(trimmed) trimmed), walks \(Int(walk * 1000)) ms"
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
    static func value(of ax: NSObject) -> String?? {
        guard let valueAttributes, let s = request(ax, attributes: valueAttributes) else { return nil }
        return .some(s.value as? String)
    }

    /// A snapshot's accessibility element: what a snapshot of it alone is asked of.
    static func element(of snapshot: XCUIElementSnapshot) -> NSObject? {
        (snapshot as AnyObject).value(forKey: "accessibilityElement") as? NSObject
    }

    /// The app's snapshot, and whether it fetched only those attributes. `full`: XCTest's.
    static func take(_ app: XCUIApplication, pid: Int32?, full: Bool) throws -> (XCUIElementSnapshot, Bool) {
        let t = Date()
        Cost.takes += 1
        defer { Cost.take += Date().timeIntervalSince(t) }
        if !full, failures < 3, let pid {
            if let s = trimmed(pid: pid) {
                failures = 0
                Cost.trimmed += 1
                return (s, true)
            }
            failures += 1
        }
        return (try app.snapshot(), false)
    }

    private typealias MakeElement = @convention(c) (AnyClass, Selector, Int32) -> NSObject?
    private static let withPidSel = NSSelectorFromString("elementWithProcessIdentifier:")
    /// XCAccessibilityElement and its elementWithProcessIdentifier:, found once.
    private static let elementMaker: (AnyClass, MakeElement)? = {
        guard let cls = NSClassFromString("XCAccessibilityElement"), let m = class_getClassMethod(cls, withPidSel) else { return nil }
        return (cls, unsafeBitCast(method_getImplementation(m), to: MakeElement.self))
    }()

    private static func trimmed(pid: Int32) -> XCUIElementSnapshot? {
        guard let attributes, let (cls, make) = elementMaker, let element = make(cls, withPidSel, pid) else { return nil }
        return request(element, attributes: attributes)
    }

    /// The accessibility client's snapshot of an element, with only those attributes.
    private static let requestSel = NSSelectorFromString("requestSnapshotForElement:attributes:parameters:error:")
    private static let defaultsSel = NSSelectorFromString("defaultParameters")
    private typealias Request = @convention(c) (NSObject, Selector, NSObject, NSArray, NSDictionary, AutoreleasingUnsafeMutablePointer<NSError?>?) -> NSObject?
    /// Where the snapshot is in the request's result: the getter that held it last (an Xcode's
    /// result is always of one class), tried first.
    private static var resultKeys = ["rootElementSnapshot", "snapshot"].map(NSSelectorFromString)

    /// The accessibility client, its request and its default parameters, found once (one client
    /// serves the whole run); found again after a request that gave nothing.
    private static var client: (object: NSObject, request: Request, parameters: NSDictionary)?

    private static func requester() -> (object: NSObject, request: Request, parameters: NSDictionary)? {
        if let c = client { return c }
        guard let object = Handler.axClient(), object.responds(to: requestSel) else { return nil }
        var parameters: NSDictionary = [:]
        if object.responds(to: defaultsSel), let d = object.perform(defaultsSel)?.takeUnretainedValue() as? NSDictionary { parameters = d }
        let found = (object: object, request: unsafeBitCast(object.method(for: requestSel), to: Request.self), parameters: parameters)
        client = found
        return found
    }

    private static func request(_ element: NSObject, attributes: NSArray) -> XCUIElementSnapshot? {
        guard let c = requester() else { return nil }
        var error: NSError?
        guard let result = c.request(c.object, requestSel, element, attributes, c.parameters, &error) else {
            client = nil
            return nil
        }
        // A result object around the snapshot (newer Xcodes), or the snapshot itself.
        for (k, key) in resultKeys.enumerated() where result.responds(to: key) {
            if let s = result.perform(key)?.takeUnretainedValue() as? XCUIElementSnapshot {
                if k > 0 { resultKeys.swapAt(0, k) }
                return s
            }
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
        guard let cls = pathClass else { throw AgentError("XCPointerEventPath is missing") }
        let alloc = cls.perform(allocSel).takeUnretainedValue() as! NSObject
        let initText = NSSelectorFromString("initForTextInput")
        guard alloc.responds(to: initText) else { throw AgentError("XCPointerEventPath.initForTextInput is missing") }
        let path = alloc.perform(initText).takeUnretainedValue() as! NSObject
        let sel = NSSelectorFromString("typeText:atOffset:typingSpeed:shouldRedact:")
        guard path.responds(to: sel) else { throw AgentError("XCPointerEventPath.typeText:atOffset:typingSpeed:shouldRedact: is missing") }
        typealias TypeText = @convention(c) (NSObject, Selector, NSString, TimeInterval, UInt64, Bool) -> Void
        unsafeBitCast(path.method(for: sel), to: TypeText.self)(path, sel, text as NSString, 0, UInt64(max(perSecond, 1)), false)
        try synthesize(try record(with: [path]), wait: wait)
    }

    private static let pathClass = NSClassFromString("XCPointerEventPath") as? NSObject.Type
    private static let recordClass = NSClassFromString("XCSynthesizedEventRecord") as? NSObject.Type
    private static let sessionClass: AnyClass? = NSClassFromString("XCTRunnerDaemonSession")
    private static let allocSel = NSSelectorFromString("alloc")
    private static let touchInitSel = NSSelectorFromString("initForTouchAtPoint:offset:")
    private static let recordInitSel = NSSelectorFromString("initWithName:interfaceOrientation:")
    private static let addPathSel = NSSelectorFromString("addPointerEventPath:")
    private static let sharedSessionSel = NSSelectorFromString("sharedSession")
    private static let daemonProxySel = NSSelectorFromString("daemonProxy")
    private static let synthesizeSel = NSSelectorFromString("_XCT_synthesizeEvent:completion:")

    private static func touchPath(at p: CGPoint, offset: TimeInterval) throws -> NSObject {
        guard let cls = pathClass else { throw AgentError("XCPointerEventPath is missing") }
        let sel = touchInitSel
        let alloc = cls.perform(allocSel).takeUnretainedValue() as! NSObject
        guard alloc.responds(to: sel) else { throw AgentError("XCPointerEventPath.initForTouchAtPoint:offset: is missing") }
        typealias Init = @convention(c) (NSObject, Selector, CGPoint, TimeInterval) -> NSObject
        return unsafeBitCast(alloc.method(for: sel), to: Init.self)(alloc, sel, p, offset)
    }

    private static func record(with paths: [NSObject]) throws -> NSObject {
        guard let cls = recordClass else { throw AgentError("XCSynthesizedEventRecord is missing") }
        let sel = recordInitSel
        let alloc = cls.perform(allocSel).takeUnretainedValue() as! NSObject
        guard alloc.responds(to: sel) else { throw AgentError("XCSynthesizedEventRecord.initWithName:interfaceOrientation: is missing") }
        typealias Init = @convention(c) (NSObject, Selector, NSString, Int) -> NSObject
        let record = unsafeBitCast(alloc.method(for: sel), to: Init.self)(alloc, sel, "Tap", UIInterfaceOrientation.portrait.rawValue)
        let add = addPathSel
        guard record.responds(to: add) else { throw AgentError("XCSynthesizedEventRecord.addPointerEventPath: is missing") }
        typealias Add = @convention(c) (NSObject, Selector, NSObject) -> Void
        for path in paths { unsafeBitCast(record.method(for: add), to: Add.self)(record, add, path) }
        return record
    }

    /// A touch sent without waiting for testmanagerd's answer: awaited before the next one
    /// (events overlapping in time could mix).
    /// testmanagerd's answer: signalled from whichever queue it comes on.
    private final class Pending {
        let answered = DispatchSemaphore(value: 0)
        private let lock = NSLock()
        private var isDone = false, failure: Error?
        func finish(_ error: Error?) {
            lock.lock()
            (isDone, failure) = (true, error)
            lock.unlock()
            answered.signal()
        }
        var done: Bool { lock.lock(); defer { lock.unlock() }; return isDone }
        var error: Error? { lock.lock(); defer { lock.unlock() }; return failure }
    }
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
        // Woken the moment it comes on another queue; the run loop turned between waits, for
        // an answer that comes on it.
        let deadline = Date(timeIntervalSinceNow: 10)
        while !p.done && Date() < deadline {
            if p.answered.wait(timeout: .now() + .milliseconds(1)) == .success { break }
            RunLoop.current.run(mode: .default, before: Date())
        }
        if let e = p.error { throw e }
    }

    /// testmanagerd plays the record; its answer comes on the run loop, which keeps turning
    /// while it is awaited (requests are served on the main thread).
    private static func synthesize(_ record: NSObject, wait: Bool = true) throws {
        try awaitPending()
        guard let cls = sessionClass,
              let session = (cls as AnyObject).perform(sharedSessionSel)?.takeUnretainedValue() as? NSObject,
              let proxy = session.perform(daemonProxySel)?.takeUnretainedValue() as? NSObject
        else { throw AgentError("XCTRunnerDaemonSession.sharedSession.daemonProxy is missing") }
        let sel = synthesizeSel
        guard proxy.responds(to: sel) else { throw AgentError("_XCT_synthesizeEvent:completion: is missing") }
        typealias Call = @convention(c) (NSObject, Selector, NSObject, @escaping (Error?) -> Void) -> Void
        let p = Pending()
        unsafeBitCast(proxy.method(for: sel), to: Call.self)(proxy, sel, record) { error in
            p.finish(error)
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
