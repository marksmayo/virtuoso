import XCTest
import Darwin
import ObjectiveC

// The virtuoso agent on an iOS simulator: one UI test that serves the host until told to quit.
//
// Wire format, as the Android agent's: u32 big-endian length, then that many bytes of UTF-8
// JSON. The host's first frame is {"token": ...}; a connection with another token is dropped.
// Every reply carries `ok`, and `error` when it is false; a reply with `blob: n` is followed by
// a frame of n raw bytes. The simulator shares the Mac's network, so the host connects to
// 127.0.0.1:VIRTUOSO_PORT (xcodebuild passes TEST_RUNNER_VIRTUOSO_PORT in as VIRTUOSO_PORT).
final class VirtuosoAgent: XCTestCase {
    /// What XCTest reported during the current request: an action that cannot happen (typing
    /// with nothing focused) is a test failure in XCTest, an error reply here.
    static var issue: String?

    override func setUp() {
        continueAfterFailure = true
    }

    override func record(_ issue: XCTIssue) {
        VirtuosoAgent.issue = issue.compactDescription
        // In the agent's log too: an exception that ends the serving test is only an issue.
        print("virtuoso agent issue: \(issue.compactDescription)")
    }

    func testServe() throws {
        let env = ProcessInfo.processInfo.environment
        let port = UInt16(env["VIRTUOSO_PORT"] ?? "") ?? 22087
        let token = env["VIRTUOSO_TOKEN"] ?? ""
        Quiescence.off()
        let listener = try Listener(port: port)
        let handler = Handler()
        serving: while true {
            let conn = try listener.accept()
            defer { conn.close() }
            guard let hello = try? conn.readJSON(), hello["token"] as? String == token else { continue }
            while let req = try? conn.readJSON() {
                if req["op"] as? String == "quit" { break serving }
                VirtuosoAgent.issue = nil
                var (reply, blob) = handler.handle(req)
                if let issue = VirtuosoAgent.issue, reply["ok"] as? Bool == true {
                    reply = ["ok": false, "error": issue]
                    blob = nil
                }
                do {
                    // A screen's reply comes written and framed already (Handler's JSONOut).
                    if let framed = reply["framed"] as? [UInt8] {
                        try conn.writeAll(framed)
                    } else {
                        try conn.writeFrame(JSONSerialization.data(withJSONObject: reply))
                    }
                    if let blob { try conn.writeFrame(blob) }
                } catch {
                    break
                }
            }
        }
    }
}

/// XCTest waits for an app to go idle before each query and action; the home screen, with its
/// animated widgets, can take a minute to. virtuoso waits for the screen to settle itself, so
/// the wait is off, as WebDriverAgent turns it off: XCUIApplicationProcess's quiescence wait
/// does nothing.
enum Quiescence {
    /// The waits as XCTest had them, for settle to call itself (wait).
    private static var original: (one: IMP?, two: IMP?) = (nil, nil)

    static func off() {
        guard let cls = NSClassFromString("XCUIApplicationProcess") else { return }
        let one: @convention(block) (AnyObject, Bool) -> Void = { _, _ in }
        let two: @convention(block) (AnyObject, Bool, Bool) -> Void = { _, _, _ in }
        if let m = class_getInstanceMethod(cls, NSSelectorFromString("waitForQuiescenceIncludingAnimationsIdle:")) {
            original.one = method_setImplementation(m, imp_implementationWithBlock(one))
        }
        if let m = class_getInstanceMethod(cls, NSSelectorFromString("waitForQuiescenceIncludingAnimationsIdle:isPreEvent:")) {
            original.two = method_setImplementation(m, imp_implementationWithBlock(two))
        }
    }

    private typealias SetTimeout = @convention(c) (Double) -> Void
    private typealias GetTimeout = @convention(c) () -> Double
    private static let setTimeout: SetTimeout? = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "_XCTSetApplicationStateTimeout").map { unsafeBitCast($0, to: SetTimeout.self) }
    private static let getTimeout: GetTimeout? = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "_XCTApplicationStateTimeout").map { unsafeBitCast($0, to: GetTimeout.self) }

    /// XCTest's own wait for the app to go idle (its event loop, then its animations), at most
    /// `timeout` seconds: a modal presenting takes no touch while it animates in. Whether it
    /// waited the whole time (an animation that never ends).
    @discardableResult
    static func wait(_ app: XCUIApplication, animations: Bool, timeout: Double) -> Bool {
        guard let process = process(of: app), let set = setTimeout, let get = getTimeout else { return false }
        let previous = get()
        set(timeout)
        // An app that does not go idle in time is an XCTest issue: not this request's error.
        let issue = VirtuosoAgent.issue
        defer {
            set(previous)
            VirtuosoAgent.issue = issue
        }
        let start = Date()
        if let two = original.two {
            typealias Wait = @convention(c) (NSObject, Selector, Bool, Bool) -> Void
            unsafeBitCast(two, to: Wait.self)(process, NSSelectorFromString("waitForQuiescenceIncludingAnimationsIdle:isPreEvent:"), animations, false)
        } else if let one = original.one {
            typealias Wait = @convention(c) (NSObject, Selector, Bool) -> Void
            unsafeBitCast(one, to: Wait.self)(process, NSSelectorFromString("waitForQuiescenceIncludingAnimationsIdle:"), animations)
        } else {
            return false
        }
        return Date().timeIntervalSince(start) >= timeout * 0.95
    }

    /// The ways to an app's process object, as selectors: the one that worked last tried first.
    private static var processPaths: [[Selector]] = [["currentProcess"], ["applicationProcess"], ["applicationImpl", "currentProcess"], ["applicationImpl", "applicationProcess"]]
        .map { $0.map(NSSelectorFromString) }

    private static func process(of app: XCUIApplication) -> NSObject? {
        for (k, path) in processPaths.enumerated() {
            var object: NSObject? = app
            for sel in path {
                guard let current = object, current.responds(to: sel) else { object = nil; break }
                object = current.perform(sel)?.takeUnretainedValue() as? NSObject
            }
            if let object {
                if k > 0 { processPaths.swapAt(0, k) }
                return object
            }
        }
        return nil
    }
}

struct AgentError: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}

final class Listener {
    let fd: Int32

    init(port: UInt16) throws {
        fd = socket(AF_INET, SOCK_STREAM, 0)
        guard fd >= 0 else { throw AgentError("socket: errno \(errno)") }
        var on: Int32 = 1
        setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &on, socklen_t(MemoryLayout<Int32>.size))
        var addr = sockaddr_in()
        addr.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        addr.sin_family = sa_family_t(AF_INET)
        addr.sin_port = port.bigEndian
        addr.sin_addr.s_addr = inet_addr("127.0.0.1")
        let bound = withUnsafePointer(to: &addr) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size)) }
        }
        guard bound == 0, listen(fd, 4) == 0 else { throw AgentError("listening on port \(port): errno \(errno)") }
    }

    func accept() throws -> Conn {
        let c = Darwin.accept(fd, nil, nil)
        guard c >= 0 else { throw AgentError("accept: errno \(errno)") }
        var on: Int32 = 1
        setsockopt(c, Int32(IPPROTO_TCP), TCP_NODELAY, &on, socklen_t(MemoryLayout<Int32>.size))
        return Conn(fd: c)
    }
}

final class Conn {
    let fd: Int32

    init(fd: Int32) { self.fd = fd }

    func readExactly(_ n: Int) throws -> Data {
        if n == 0 { return Data() }
        // Read straight into the Data returned: no copy from a buffer.
        var buf = Data(count: n)
        try buf.withUnsafeMutableBytes { (p: UnsafeMutableRawBufferPointer) in
            var got = 0
            while got < n {
                let r = read(fd, p.baseAddress! + got, n - got)
                if r <= 0 { throw AgentError("connection closed") }
                got += r
            }
        }
        return buf
    }

    func readJSON() throws -> [String: Any] {
        let h = [UInt8](try readExactly(4))
        let n = Int(h[0]) << 24 | Int(h[1]) << 16 | Int(h[2]) << 8 | Int(h[3])
        guard let obj = try JSONSerialization.jsonObject(with: readExactly(n)) as? [String: Any] else {
            throw AgentError("a request that is not a JSON object")
        }
        return obj
    }

    func writeFrame(_ body: Data) throws {
        var out = withUnsafeBytes(of: UInt32(body.count).bigEndian) { Data($0) }
        out.append(body)
        try out.withUnsafeBytes { try send($0) }
    }

    /// Bytes framed already (their length first).
    func writeAll(_ framed: [UInt8]) throws {
        try framed.withUnsafeBytes { try send($0) }
    }

    private func send(_ p: UnsafeRawBufferPointer) throws {
        var sent = 0
        while sent < p.count {
            let r = write(fd, p.baseAddress! + sent, p.count - sent)
            if r <= 0 { throw AgentError("write: errno \(errno)") }
            sent += r
        }
    }

    func close() { Darwin.close(fd) }
}
