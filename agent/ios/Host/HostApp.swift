import SwiftUI

// The agent's test host: never shown in a run, the agent drives the app under test.
@main
struct HostApp: App {
    var body: some Scene {
        WindowGroup { Text("virtuoso agent") }
    }
}
