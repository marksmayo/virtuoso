import SwiftUI

// What the iOS device tests drive: a form, a long list and a deep link, the surfaces the
// Android tests find in Settings and the bench apps.
@main
struct FixtureApp: App {
    @State private var link = ""

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                List {
                    NavigationLink("Form") { FormScreen() }
                    NavigationLink("Long list") { LongList() }
                    if !link.isEmpty { Text("Opened \(link)") }
                }
                .navigationTitle("Fixture")
            }
            .onOpenURL { link = $0.absoluteString }
        }
    }
}

struct FormScreen: View {
    @State private var name = ""
    @State private var secret = ""
    @State private var subscribed = false
    @State private var greeting = ""

    var body: some View {
        Form {
            TextField("Name", text: $name).accessibilityIdentifier("name-field")
            SecureField("Password", text: $secret).accessibilityIdentifier("password-field")
            Toggle("Subscribe", isOn: $subscribed).accessibilityIdentifier("subscribe-toggle")
            Button("Submit") { greeting = "Hello, \(name)" }.accessibilityIdentifier("submit-button")
            if !greeting.isEmpty { Text(greeting).accessibilityIdentifier("greeting") }
            Text("Name length: \(name.count)")
        }
        .navigationTitle("Form")
    }
}

struct LongList: View {
    var body: some View {
        List(1...200, id: \.self) { i in
            Text("Row \(i)").accessibilityIdentifier("row-\(i)")
        }
        .navigationTitle("Long list")
    }
}
