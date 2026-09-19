import SwiftUI

/// How often the dot asks again. **ASKING NOW COSTS A ROUND TRIP PER ACCOUNT**,
/// to the provider and to the keychain: the windows it reads are five hours and
/// seven days wide, so a minute's precision would buy nothing and cost traffic.
let askAgainEvery: Duration = .seconds(180)

/// How far back the spending is summed in the menu.
let hoursShown = 24

/// Where an allowance stops being worth a glance and starts being a warning.
let gettingFull = 80.0

@MainActor
final class Accounts: ObservableObject {
    @Published var reading: Reading = .quiet

    func watch() {
        Task {
            while !Task.isCancelled {
                reading = await askSailor(hours: hoursShown)
                try? await Task.sleep(for: askAgainEvery)
            }
        }
    }
}

@main
struct SailorDotApp: App {
    @StateObject private var accounts = Accounts()

    var body: some Scene {
        MenuBarExtra {
            Menu_(accounts: accounts)
        } label: {
            // **A GLYPH ALONE SAYS NOTHING ABOUT HOW MUCH IS LEFT**, which is
            // the thing being watched: the fullest window of an account in use
            // rides beside it, and only once it is worth looking at.
            Image(systemName: Standing.mark(accounts.reading.worst))
            if let pressing = accounts.reading.pressing {
                Text("\(Int(pressing.usedPercent))%")
            }
        }
        .menuBarExtraStyle(.window)
        .onChange(of: accounts.reading.readAt, initial: true) { _, _ in }
    }
}

struct Menu_: View {
    @ObservedObject var accounts: Accounts

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Account e quota")
                .font(.headline)
            if accounts.reading.accounts.isEmpty {
                Text("Sailor non ha ancora risposto")
                    .foregroundStyle(.secondary)
            }
            ForEach(accounts.reading.accounts.sorted { $0.urgency < $1.urgency }) { account in
                Row(account: account)
            }
            Divider()
            Text("La spesa è solo quella dei flussi di Sailor: le sessioni di terminale non ci passano. Le percentuali sono la quota vera, chiesta al fornitore.")
                .font(.caption2)
                .foregroundStyle(.secondary)
            HStack {
                Button("Aggiorna adesso") {
                    Task { accounts.reading = await askSailor(hours: hoursShown) }
                }
                Spacer()
                Button("Esci") { NSApplication.shared.terminate(nil) }
            }
        }
        .padding(12)
        .frame(width: 420)
        .task { accounts.watch() }
    }
}

struct Row: View {
    let account: Account

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Image(systemName: Standing.mark(account.standing))
                    .foregroundStyle(colour)
                Text("\(account.cli) · \(account.profile ?? "dal terminale")")
                    .fontWeight(account.active ? .semibold : .regular)
                Spacer()
                Text(Standing.said(account.standing))
                    .font(.caption)
                    .foregroundStyle(colour)
            }
            ForEach(account.windows ?? []) { window in
                WindowBar(window: window)
            }
            if let refused = account.quotaSaid {
                Text(refused)
                    .font(.caption2)
                    .foregroundStyle(.orange)
                    .lineLimit(2)
            }
            Text("$\(account.spent) · \(account.calls) chiamate · \(ago(account.lastCallAgoS))")
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
        .padding(.leading, 2)
    }

    private var colour: Color {
        switch account.standing {
        case "ready": return .green
        case "ran_out": return .orange
        case "shut": return .red
        default: return .secondary
        }
    }
}

struct WindowBar: View {
    let window: Window

    var body: some View {
        HStack(spacing: 6) {
            Text(window.said)
                .font(.caption)
                .frame(width: 62, alignment: .leading)
            ProgressView(value: min(window.usedPercent, 100), total: 100)
                .tint(window.usedPercent >= 100 ? .red : window.usedPercent >= gettingFull ? .orange : .green)
                .frame(height: 4)
            Text("\(Int(window.usedPercent))%")
                .font(.caption.monospacedDigit())
                .frame(width: 36, alignment: .trailing)
            Text(whenItComesBack(window.resetsAt).map { "→ \($0)" } ?? "")
                .font(.caption2)
                .foregroundStyle(.secondary)
                .frame(width: 78, alignment: .leading)
        }
    }
}
