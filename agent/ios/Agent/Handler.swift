import XCTest

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

    private func dispatch(_ r: [String: Any]) throws -> ([String: Any], Data?) {
        let ok: [String: Any] = ["ok": true]
        switch r["op"] as? String ?? "" {
        case "ping":
            return (ok, nil)
        case "info":
            let b = UIScreen.main.bounds
            return (["ok": true, "width": Int(b.width), "height": Int(b.height), "scale": UIScreen.main.scale], nil)
        case "screen":
            return (try screen(), nil)
        case "tap":
            point(r, "x", "y").tap()
            return (ok, nil)
        case "press":
            point(r, "x", "y").press(forDuration: Double(int(r, "ms")) / 1000)
            return (ok, nil)
        case "swipe":
            swipe(r)
            return (ok, nil)
        case "type":
            front().typeText(r["text"] as? String ?? "")
            return (ok, nil)
        case "erase":
            front().typeText(String(repeating: XCUIKeyboardKey.delete.rawValue, count: int(r, "count")))
            return (ok, nil)
        case "key":
            try key(r["name"] as? String ?? "")
            return (ok, nil)
        case "launch":
            launch(r)
            return (ok, nil)
        case "terminate":
            XCUIApplication(bundleIdentifier: r["app"] as? String ?? "").terminate()
            return (ok, nil)
        case "screenshot":
            let png = XCUIScreen.main.screenshot().pngRepresentation
            return (["ok": true, "blob": png.count], png)
        case "settle":
            return (settle(quiet: int(r, "quiet"), noChange: int(r, "nochange"), timeout: int(r, "timeout")), nil)
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
    private func front() -> XCUIApplication {
        app.state == .runningForeground ? app : springboard
    }

    private func point(_ r: [String: Any], _ kx: String, _ ky: String) -> XCUICoordinate {
        front().coordinate(withNormalizedOffset: CGVector(dx: 0, dy: 0))
            .withOffset(CGVector(dx: int(r, kx), dy: int(r, ky)))
    }

    private func swipe(_ r: [String: Any]) {
        let from = point(r, "x1", "y1"), to = point(r, "x2", "y2")
        let dx = Double(int(r, "x2") - int(r, "x1")), dy = Double(int(r, "y2") - int(r, "y1"))
        let seconds = max(Double(int(r, "ms")) / 1000, 0.05)
        let speed = max((dx * dx + dy * dy).squareRoot() / seconds, 100)
        // A fling lifts at speed; a still or cancelled lift holds first, so nothing coasts.
        let hold = (r["fling"] as? Bool ?? false) ? 0 : 0.3
        from.press(forDuration: 0.05, thenDragTo: to, withVelocity: XCUIGestureVelocity(CGFloat(speed)), thenHoldForDuration: hold)
    }

    private func key(_ name: String) throws {
        switch name {
        case "home": XCUIDevice.shared.press(.home)
        case "enter": front().typeText("\n")
        case "backspace": front().typeText(XCUIKeyboardKey.delete.rawValue)
        case "tab": front().typeText("\t")
        case "space": front().typeText(" ")
        case "lock": XCUIDevice.shared.perform(NSSelectorFromString("pressLockButton"))
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
    private func screen() throws -> [String: Any] {
        let target = front()
        var windows: [[String: Any]] = []
        var nodes: [[String: Any]] = []
        if target != springboard, springboard.alerts.firstMatch.exists {
            let w = windows.count
            windows.append(["pkg": "com.apple.springboard", "type": 1])
            flatten(try springboard.alerts.firstMatch.snapshot(), depth: 0, window: w, keyboard: w, into: &nodes)
        }
        let w = windows.count
        windows.append(["pkg": bundleId(target), "type": 1])
        windows.append(["pkg": bundleId(target), "type": 2])
        flatten(try target.snapshot(), depth: 0, window: w, keyboard: w + 1, into: &nodes)
        return ["ok": true, "windows": windows, "nodes": nodes]
    }

    private func bundleId(_ a: XCUIApplication) -> String {
        a == springboard ? "com.apple.springboard" : appId
    }

    private func flatten(_ s: XCUIElementSnapshot, depth: Int, window: Int, keyboard: Int, into nodes: inout [[String: Any]]) {
        let inKeyboard = window == keyboard || s.elementType == .keyboard
        let w = inKeyboard ? keyboard : window
        nodes.append(node(s, depth: depth, window: w))
        for c in s.children {
            flatten(c, depth: depth + 1, window: inKeyboard ? keyboard : window, keyboard: keyboard, into: &nodes)
        }
    }

    // Flags as the host reads them (screen.rs).
    static let clickable = 1, enabled = 2, scrollable = 8, checked = 16, visible = 32
    static let selected = 64, editable = 128, checkable = 256, longClickable = 512, password = 1024

    private static let textInputs: Set<XCUIElement.ElementType> = [.textField, .secureTextField, .searchField, .textView]
    private static let tappable: Set<XCUIElement.ElementType> = [
        .button, .cell, .link, .switch, .toggle, .textField, .secureTextField, .searchField, .textView,
        .tab, .segmentedControl, .menuItem, .checkBox, .radioButton, .slider, .stepper, .picker, .pickerWheel,
        .key, .icon, .tabBar, .image,
    ]
    private static let scrolling: Set<XCUIElement.ElementType> = [.scrollView, .table, .collectionView, .webView]

    private func node(_ s: XCUIElementSnapshot, depth: Int, window: Int) -> [String: Any] {
        let f = s.frame
        let type = s.elementType
        let value = (s.value as? String) ?? (s.value as? NSNumber)?.stringValue
        let placeholder = s.placeholderValue ?? ""
        var text = s.label, desc = ""
        if Handler.textInputs.contains(type) {
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
        if Handler.textInputs.contains(type) { flags |= Handler.editable }
        if type == .secureTextField { flags |= Handler.password }
        if [.switch, .toggle, .checkBox].contains(type) {
            flags |= Handler.checkable
            if value == "1" { flags |= Handler.checked }
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
        case .other: return "Other"
        default: return "Element\(t.rawValue)"
        }
    }

    // MARK: settling

    /// Waits for the screen to stop changing: done once it has held still for `quiet` ms
    /// after a change, or `noChange` ms passed without one, or `timeout` ms in all.
    private func settle(quiet: Int, noChange: Int, timeout: Int) -> [String: Any] {
        let start = Date()
        var last = signature(), lastChange = start, changed = false
        while true {
            Thread.sleep(forTimeInterval: 0.05)
            let now = Date(), s = signature()
            if s != last {
                changed = true
                lastChange = now
                last = s
            }
            let total = now.timeIntervalSince(start) * 1000, still = now.timeIntervalSince(lastChange) * 1000
            if !changed && total >= Double(noChange) || changed && still >= Double(quiet) {
                return ["ok": true, "changed": changed, "timedOut": false]
            }
            if total >= Double(timeout) {
                return ["ok": true, "changed": changed, "timedOut": true]
            }
        }
    }

    private func signature() -> String {
        guard let s = try? front().snapshot() else { return "" }
        var out = ""
        func walk(_ s: XCUIElementSnapshot) {
            let f = s.frame
            out += "\(s.elementType.rawValue)|\(s.label)|\(s.value ?? "")|\(Int(f.minX)),\(Int(f.minY)),\(Int(f.width)),\(Int(f.height));"
            s.children.forEach(walk)
        }
        walk(s)
        return out
    }
}
