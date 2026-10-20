import SwiftUI

// The iPhone app in the phone's language (Resources/Phone/Localizable.xcstrings: English and Russian).
// Literals in Text("…") are looked up by SwiftUI already; these also look up the text that reaches a
// view as a String (a helper's parameter, a status line, a ternary with a variable). A String that is
// not in the table (a session's title, a file name) shows as it is.

/// `text` in the phone's language when the table has it, else unchanged.
func phoneLocalized(_ text: String) -> String {
    Bundle.main.localizedString(forKey: text, value: text, table: nil)
}

extension Text {
    /// Preferred over SwiftUI's generic `init<S: StringProtocol>(_:)` for a String, never over a literal.
    @_disfavoredOverload
    init(_ content: String) {
        self.init(verbatim: phoneLocalized(content))
    }
}

extension Label where Title == Text, Icon == Image {
    @_disfavoredOverload
    init(_ title: String, systemImage: String) {
        self.init { Text(verbatim: phoneLocalized(title)) } icon: { Image(systemName: systemImage) }
    }
}
