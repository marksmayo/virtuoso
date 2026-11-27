import SwiftUI
import UIKit

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
                    NavigationLink("Loading") { LoadingScreen() }
                    NavigationLink("Wizard") { KeyboardScreen(closesOnTouch: true) }
                    NavigationLink("Stubborn") { KeyboardScreen(closesOnTouch: false) }
                    NavigationLink("Middle") { MiddleButtonScreen(closesOnTouch: false) }
                    NavigationLink("Middle touch") { MiddleButtonScreen(closesOnTouch: true) }
                    NavigationLink("Code") { CodeScreen() }
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

/// A screen that says it started at once, then keeps loading rows for two seconds: a settle
/// held until it goes quiet waits the rows out; one for "Started" need not.
struct LoadingScreen: View {
    @State private var started = false
    @State private var rows: [Int] = []

    var body: some View {
        List {
            Button("Start") {
                started = true
                Task { @MainActor in
                    for i in 1...14 {
                        try? await Task.sleep(nanoseconds: 150_000_000)
                        rows.append(i)
                    }
                }
            }
            if started { Text("Started") }
            ForEach(rows, id: \.self) { Text("Loaded \($0)") }
        }
        .navigationTitle("Loading")
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

/// A field over a button that fills the screen's middle: a touch there to close the keyboard
/// would press it (the count says). Its keyboard closes with a drag, or with `closesOnTouch` only
/// with a touch beside the field that barely moves (React Native's default): that screen does not
/// scroll, and a drag and Return leave the keyboard up.
struct MiddleButtonScreen: View {
    let closesOnTouch: Bool
    @State private var text = ""
    @State private var pressed = 0
    @State private var submitted = 0
    @FocusState private var focused: Bool

    private var content: some View {
        VStack(spacing: 16) {
            Group {
                // Touch: one line, and Return submits (as a React Native field's onSubmitEditing).
                if closesOnTouch {
                    TextField("Middle field", text: $text).onSubmit { submitted += 1 }
                } else {
                    TextField("Middle field", text: $text, axis: .vertical)
                }
            }
            .focused($focused)
            .accessibilityIdentifier("middle-field")
            Text("Pressed \(pressed)")
            if closesOnTouch { Text("Submitted \(submitted)") }
            if closesOnTouch {
                // As a React Native Pressable with no role: a view that takes taps, an Other with an
                // id in the accessibility tree, not a Button.
                Text("Middle button")
                    .frame(maxWidth: .infinity, minHeight: 260)
                    .background(Color.gray.opacity(0.2))
                    .contentShape(Rectangle())
                    .onTapGesture { pressed += 1 }
                    .accessibilityElement(children: .ignore)
                    .accessibilityIdentifier("middle-button")
            } else {
                Button { pressed += 1 } label: {
                    Text("Middle button").frame(maxWidth: .infinity, minHeight: 420)
                }
                .buttonStyle(.bordered)
                .accessibilityIdentifier("middle-button")
            }
            if closesOnTouch { Spacer() }
        }
        .padding(24)
    }

    var body: some View {
        Group {
            if closesOnTouch {
                content
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(
                        Color.clear
                            .contentShape(Rectangle())
                            .gesture(DragGesture(minimumDistance: 0).onEnded { v in
                                if abs(v.translation.width) < 40 && abs(v.translation.height) < 40 { focused = false }
                            })
                    )
            } else {
                ScrollView { content }.scrollDismissesKeyboard(.interactively)
            }
        }
        .ignoresSafeArea(.keyboard)
        .navigationTitle(closesOnTouch ? "Middle touch" : "Middle")
    }
}

/// A code field as React Native's TextInput reads to XCTest while empty: no value (and no
/// placeholder), the digits typed shown below it.
struct CodeScreen: View {
    @State private var code = ""

    var body: some View {
        Form {
            NoValueField(text: $code).frame(height: 44)
            Text(code.isEmpty ? "No code" : "Code: \(code)")
        }
        .navigationTitle("Code")
    }
}

struct NoValueField: UIViewRepresentable {
    @Binding var text: String

    /// A text field whose accessibility value is nothing while it is empty.
    final class Field: UITextField {
        override var accessibilityValue: String? {
            get { (text ?? "").isEmpty ? nil : text }
            set {}
        }
    }

    final class Coordinator: NSObject {
        let text: Binding<String>
        init(text: Binding<String>) { self.text = text }
        @objc func changed(_ f: UITextField) { text.wrappedValue = f.text ?? "" }
    }

    func makeCoordinator() -> Coordinator { Coordinator(text: $text) }

    func makeUIView(context: Context) -> UITextField {
        let f = Field()
        f.borderStyle = .roundedRect
        f.autocorrectionType = .no
        f.accessibilityIdentifier = "code-field"
        f.addTarget(context.coordinator, action: #selector(Coordinator.changed(_:)), for: .editingChanged)
        return f
    }

    func updateUIView(_ f: UITextField, context: Context) {
        if f.text != text { f.text = text }
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
