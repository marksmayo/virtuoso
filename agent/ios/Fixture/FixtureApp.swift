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
                    NavigationLink("Location") { LocationScreen() }
                    NavigationLink("Counter") { CounterScreen() }
                    NavigationLink("Wizard") { KeyboardScreen(closesOnTouch: true) }
                    NavigationLink("Stubborn") { KeyboardScreen(closesOnTouch: false) }
                    AppearanceText()
                    if !link.isEmpty { Text("Opened \(link)") }
                    // A launch argument `-greeting X` is a user default, as on any iOS app.
                    if let g = UserDefaults.standard.string(forKey: "greeting") { Text("Greeting: \(g)") }
                }
                .navigationTitle("Fixture")
            }
            .onOpenURL { link = $0.absoluteString }
        }
    }
}

/// The appearance the app draws in: it redraws when the simulator's changes.
struct AppearanceText: View {
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Text("Appearance: \(scheme == .dark ? "dark" : "light")")
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

/// A number that changes on every frame, forever: a screen that never holds still.
struct CounterScreen: View {
    @State private var frame = 0
    private let tick = Timer.publish(every: 1.0 / 60, on: .main, in: .common).autoconnect()

    var body: some View {
        VStack(spacing: 16) {
            Text("Frame \(frame)").accessibilityIdentifier("frame-counter")
            Text("Still text")
        }
        .onReceive(tick) { _ in frame += 1 }
        .navigationTitle("Counter")
    }
}

/// An email field as React Native's signup wizards have it: no Done key, Return adds a line
/// and leaves the keyboard up, a drag does not close it. With `closesOnTouch`, a touch beside
/// the field that barely moves closes it (React Native's keyboardShouldPersistTaps="never");
/// without, nothing does.
struct KeyboardScreen: View {
    let closesOnTouch: Bool
    @State private var email = ""
    @FocusState private var focused: Bool

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                TextField("Email", text: $email, axis: .vertical)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .focused($focused)
                    .accessibilityIdentifier("wizard-email")
                Color.clear
                    .frame(height: 600)
                    .contentShape(Rectangle())
                    .gesture(DragGesture(minimumDistance: 0).onEnded { v in
                        if closesOnTouch && abs(v.translation.width) < 40 && abs(v.translation.height) < 40 { focused = false }
                    })
            }
            .padding(24)
        }
        .scrollDismissesKeyboard(.never)
        .navigationTitle(closesOnTouch ? "Wizard" : "Stubborn")
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
