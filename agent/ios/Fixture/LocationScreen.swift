import CoreLocation
import SwiftUI

// Asks for the location: the system's permission prompt, over the app.
struct LocationScreen: View {
    @StateObject private var model = LocationModel()

    var body: some View {
        List {
            Button("Ask for location") { model.ask() }.accessibilityIdentifier("ask-location")
            Text("Location: \(model.status)").accessibilityIdentifier("location-status")
        }
        .navigationTitle("Location")
    }
}

final class LocationModel: NSObject, ObservableObject, CLLocationManagerDelegate {
    private let manager = CLLocationManager()
    @Published var status = "not asked"

    override init() {
        super.init()
        manager.delegate = self
    }

    func ask() { manager.requestWhenInUseAuthorization() }

    func locationManagerDidChangeAuthorization(_ m: CLLocationManager) {
        switch m.authorizationStatus {
        case .authorizedWhenInUse, .authorizedAlways: status = "allowed"
        case .denied, .restricted: status = "denied"
        default: status = "not asked"
        }
    }
}
