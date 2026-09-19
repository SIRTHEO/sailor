import SwiftUI

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

    /// The accounts worked in during the window, heaviest first: the ones a
    /// glance is about.
    var atWork: [Account] {
        accounts.filter(\.atWork).sorted { $0.tokensWorked > $1.tokensWorked }
    }

    /// The ones asking for a hand: shut, run out, or unreadable — and not
    /// already shown above, where they are being worked in anyway.
    var needingAHand: [Account] {
        accounts
            .filter { !$0.atWork && $0.standing != "ready" }
            .sorted { $0.urgency < $1.urgency }
    }

    /// Signed in, nothing to fix, nothing done: folded away.
    var theRest: [Account] {
        accounts.filter { !$0.atWork && $0.standing == "ready" }
    }

    /// The whole reading in the one line a first look reads.
    var inOneLine: String {
        let working = atWork
        guard !working.isEmpty else { return "no account at work in this window" }
        let tokens = working.reduce(0) { $0 + $1.tokensWorked }
        let sessions = working.reduce(0) { $0 + ($1.worked?.sessions ?? 0) }
        return "\(working.count) account at work · \(sessions) session(s) · \(inShort(tokens)) tokens"
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
    /// What this account really did in its own home, terminals included.
    /// `nil` where nobody measured that engine: unknown, never nothing.
    let worked: Worked?
    /// The line that cures this row, where the engine declares one.
    let repair: String?

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
        case cli, profile, active, standing, said, calls, spent, windows, worked, repair
        case lastCallAgoS = "last_call_ago_s"
        case ranOutAgoS = "ran_out_ago_s"
        case quotaSaid = "quota_said"
    }

    /// Whether this account was worked in during the window: the one question
    /// the first look is about.
    var atWork: Bool { (worked?.calls ?? 0) > 0 }

    var tokensWorked: Int { worked?.tokens ?? 0 }
}

/// What an account really did, read off the engine's own records: the calls a
/// person made by hand pass through no ledger, and this is where they are.
struct Worked: Decodable {
    let calls: Int
    let sessions: Int
    let inputTokens: Int
    let outputTokens: Int
    let cacheReadTokens: Int
    let cacheWriteTokens: Int
    /// What the work weighs at list price; `nil` where a model has no price.
    let atListPrice: String?

    var tokens: Int { inputTokens + outputTokens + cacheReadTokens + cacheWriteTokens }

    enum CodingKeys: String, CodingKey {
        case calls, sessions
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
        case cacheReadTokens = "cache_read_tokens"
        case cacheWriteTokens = "cache_write_tokens"
        case atListPrice = "at_list_price"
    }
}

/// A token count as a person reads it at a glance.
func inShort(_ tokens: Int) -> String {
    if tokens < 10_000 { return "\(tokens)" }
    if tokens < 1_000_000 { return "\(tokens / 1_000)k" }
    return String(format: "%.1fM", Double(tokens) / 1_000_000)
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
        case "five_hour": return "5 hours"
        case "seven_day": return "7 days"
        case "primary_window": return "window"
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
    said.locale = Locale(identifier: "en_US_POSIX")
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

    /// One colour per standing, so the mark and the word never disagree.
    static func colour(_ standing: String) -> Color {
        switch standing {
        case "ready": return .green
        case "ran_out": return .orange
        case "shut": return .red
        default: return .secondary
        }
    }

    static func said(_ standing: String) -> String {
        switch standing {
        case "ready": return "ready"
        case "ran_out": return "RAN OUT"
        case "shut": return "SHUT"
        default: return "not known"
        }
    }
}

/// How long ago, in the shortest words that are still true.
func ago(_ seconds: Int?) -> String {
    guard let seconds, seconds >= 0 else { return "never" }
    if seconds < 90 { return "just now" }
    if seconds < 5_400 { return "\(seconds / 60)m ago" }
    if seconds < 172_800 { return "\(seconds / 3_600)h ago" }
    return "\(seconds / 86_400)d ago"
}
