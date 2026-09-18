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

    var id: String { "\(cli)/\(profile ?? "-")" }

    enum CodingKeys: String, CodingKey {
        case cli, profile, active, standing, said, calls, spent
        case lastCallAgoS = "last_call_ago_s"
        case ranOutAgoS = "ran_out_ago_s"
    }
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
