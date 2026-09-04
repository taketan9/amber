import SwiftUI

@main
struct CianApp: App {
    var body: some Scene {
        WindowGroup {
            ContentView()
                // Cyan, because that is what the app is called and what its
                // icon is. One accent through the whole app rather than a
                // colour per screen: the tint is how you tell what can be
                // touched, and a different answer on every screen is no
                // answer.
                .tint(Color("AccentColor"))
        }
    }
}
