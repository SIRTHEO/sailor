import Foundation

/// What `sailor accounts --json` answers. The dot never reads the store: one
/// place makes the sums, and a second would give a second figure.
struct Reading: Decodable {
    let worst: String
    let readAt: Int
    let accounts: [Account]

    enum CodingKeys: String, CodingKey {
        case worst
        case readAt = "read_at"
        case accounts
    }

    static let quiet = Reading(worst: "unknown", readAt: 0, accounts: [])

    /// The window worth putting in the menu bar: the fullest one belonging to
    /// an account actually in use, and only once it is worth a glance.
    var pressing: Window? {
        accounts
            .filter { $0.active }
            .compactMap { $0.fullest }
            .filter { $0.usedPercent >= gettingFull }
            .max { one, two in one.usedPercent < two.usedPercent }
    }
}

struct Account: Decodable, Identifiable {
    let cli: String
    let profile: String?
    let active: Bool
    let standing: String
    let said: String
    let calls: Int
    let spent: String
    let lastCallAgoS: Int?
    let ranOutAgoS: Int?
    /// The allowance itself, asked of the provider. `nil` where nobody asked.
    let windows: [Window]?
    /// Why the allowance could not be read, in the provider's own words.
    let quotaSaid: String?

    var id: String { "\(cli)/\(profile ?? "-")" }

    /// The window nearest its end: the one figure that speaks for the account.
    var fullest: Window? {
        windows?.max { one, two in one.usedPercent < two.usedPercent }
    }

    /// **SEVEREST FIRST, AND ACTIVE BEFORE SHELVED.** The account in use is the
    /// one a glance is about; a shelved dead one belongs further down.
    var urgency: Int {
        let byStanding = ["shut": 0, "ran_out": 1, "unknown": 2, "ready": 3][standing] ?? 2
        return byStanding * 2 + (active ? 0 : 1)
    }

    enum CodingKeys: String, CodingKey {
        case cli, profile, active, standing, said, calls, spent, windows
        case lastCallAgoS = "last_call_ago_s"
        case ranOutAgoS = "ran_out_ago_s"
        case quotaSaid = "quota_said"
    }
}

/// One window of an allowance, as `sailor accounts --quota` reports it.
struct Window: Decodable, Identifiable {
    let unit: String
    let usedPercent: Double
    let resetsAt: String?

    var id: String { unit }

    /// The window's name as a person says it, and the raw name when this
    /// version does not know it: the provider adds windows without asking.
    var said: String {
        switch unit {
        case "five_hour": return "5 ore"
        case "seven_day": return "7 giorni"
        case "primary_window": return "finestra"
        default: return unit
        }
    }

    enum CodingKeys: String, CodingKey {
        case unit
        case usedPercent = "used_percent"
        case resetsAt = "resets_at"
    }
}

/// **A TIME NOBODY CAN READ IS NOT AN ANSWER.** The provider sends whichever
/// ISO shape it likes, with fractional seconds or without, so both are tried
/// and an unparsable one is shown as it arrived rather than dropped.
func whenItComesBack(_ text: String?) -> String? {
    guard let text else { return nil }
    let whole = ISO8601DateFormatter()
    whole.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
    let plain = ISO8601DateFormatter()
    plain.formatOptions = [.withInternetDateTime]
    guard let at = whole.date(from: text) ?? plain.date(from: text) else { return text }
    let said = DateFormatter()
    said.locale = Locale(identifier: "it_IT")
    said.dateFormat = Calendar.current.isDateInToday(at) ? "HH:mm" : "d MMM HH:mm"
    return said.string(from: at)
}

enum Standing {
    /// The glyph for a standing, so the menu bar says it without a word.
    static func mark(_ standing: String) -> String {
        switch standing {
        case "ready": return "circle.fill"
        case "ran_out": return "circle.lefthalf.filled"
        case "shut": return "circle"
        default: return "circle.dotted"
        }
    }

    static func said(_ standing: String) -> String {
        switch standing {
        case "ready": return "pronto"
        case "ran_out": return "ESAURITO"
        case "shut": return "CHIUSO"
        default: return "non si sa"
        }
    }
}

/// How long ago, in the shortest words that are still true.
func ago(_ seconds: Int?) -> String {
    guard let seconds, seconds >= 0 else { return "mai" }
    if seconds < 90 { return "adesso" }
    if seconds < 5_400 { return "\(seconds / 60)m fa" }
    if seconds < 172_800 { return "\(seconds / 3_600)h fa" }
    return "\(seconds / 86_400)g fa"
}
