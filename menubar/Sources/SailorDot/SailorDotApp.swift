import SwiftUI

/// How often the dot asks again. **ASKING NOW COSTS A ROUND TRIP PER ACCOUNT**,
/// to the provider and to the keychain: the windows it reads are five hours and
/// seven days wide, so a minute's precision would buy nothing and cost traffic.
let askAgainEvery: Duration = .seconds(180)

/// How far back the spending and the work are summed in the menu.
let hoursShown = 24

/// Where an allowance stops being worth a glance and starts being a warning.
let gettingFull = 80.0

@MainActor
final class Accounts: ObservableObject {
    @Published var reading: Reading = .quiet
    @Published var asking = false

    func watch() {
        Task {
            while !Task.isCancelled {
                await askNow()
                try? await Task.sleep(for: askAgainEvery)
            }
        }
    }

    func askNow() async {
        asking = true
        reading = await askSailor(hours: hoursShown)
        asking = false
    }
}

@main
struct SailorDotApp: App {
    @StateObject private var accounts = Accounts()

    var body: some Scene {
        MenuBarExtra {
            Panel(accounts: accounts)
        } label: {
            // **THE BAR SAYS WHOSE IT IS, THEN HOW IT STANDS.** A bare circle
            // among thirty other bare circles is one nobody finds; the boat is
            // Sailor's, and the mark beside it carries the standing.
            Image(systemName: "sailboat.fill")
            Image(systemName: Standing.mark(accounts.reading.worst))
            if let pressing = accounts.reading.pressing {
                Text("\(Int(pressing.usedPercent))%")
            }
        }
        .menuBarExtraStyle(.window)
        .onChange(of: accounts.reading.readAt, initial: true) { _, _ in }
    }
}

/// **THREE LAYERS, IN THE ORDER A GLANCE WANTS THEM.** What is being worked in
/// now, what is asking to be fixed, and everything else folded away: a list
/// that gives fourteen equal rows is a list nobody reads twice.
struct Panel: View {
    @ObservedObject var accounts: Accounts
    @State private var showTheRest = false
    @State private var copied: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Header(reading: accounts.reading)
            if accounts.reading.accounts.isEmpty {
                Text("Sailor non ha ancora risposto")
                    .foregroundStyle(.secondary)
            }
            if !accounts.reading.atWork.isEmpty {
                Section_("Al lavoro adesso") {
                    ForEach(accounts.reading.atWork) { account in
                        Row(account: account, copied: $copied)
                    }
                }
            }
            if !accounts.reading.needingAHand.isEmpty {
                Section_("Da sistemare") {
                    ForEach(accounts.reading.needingAHand) { account in
                        Row(account: account, copied: $copied)
                    }
                }
            }
            if !accounts.reading.theRest.isEmpty {
                DisclosureGroup(isExpanded: $showTheRest) {
                    VStack(alignment: .leading, spacing: 8) {
                        ForEach(accounts.reading.theRest) { account in
                            Row(account: account, copied: $copied)
                        }
                    }
                    .padding(.top, 6)
                } label: {
                    Text("Fermi (\(accounts.reading.theRest.count))")
                        .font(.caption.weight(.semibold))
                        .foregroundStyle(.secondary)
                }
            }
            Divider()
            Text("Il lavoro è letto dai diari dei motori, sessioni di terminale comprese; la spesa in dollari è solo quella dei flussi di Sailor. Le percentuali sono la quota vera, chiesta al fornitore.")
                .font(.caption2)
                .foregroundStyle(.secondary)
            HStack {
                Button(accounts.asking ? "Sto chiedendo…" : "Aggiorna adesso") {
                    Task { await accounts.askNow() }
                }
                .disabled(accounts.asking)
                Spacer()
                Button("Esci") { NSApplication.shared.terminate(nil) }
            }
        }
        .padding(14)
        .frame(width: 460)
        .task { accounts.watch() }
    }
}

/// The one line the first look is about: how much was worked, and by how many.
struct Header: View {
    let reading: Reading

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 6) {
                Image(systemName: Standing.mark(reading.worst))
                    .foregroundStyle(Standing.colour(reading.worst))
                Text("Account e quota")
                    .font(.headline)
                Spacer()
                Text("ultime \(hoursShown)h")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            Text(reading.inOneLine)
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }
}

struct Section_<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    init(_ title: String, @ViewBuilder content: () -> Content) {
        self.title = title
        self.content = content()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title.uppercased())
                .font(.caption2.weight(.semibold))
                .foregroundStyle(.secondary)
            content
        }
    }
}

struct Row: View {
    let account: Account
    @Binding var copied: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Image(systemName: Standing.mark(account.standing))
                    .foregroundStyle(Standing.colour(account.standing))
                Text(account.profile ?? "dal terminale")
                    .fontWeight(account.atWork ? .semibold : .regular)
                Text(account.cli)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(Standing.said(account.standing))
                    .font(.caption)
                    .foregroundStyle(Standing.colour(account.standing))
            }
            ForEach(account.windows ?? []) { window in
                WindowBar(window: window)
            }
            if let worked = account.worked, worked.calls > 0 {
                Text(said(worked))
                    .font(.caption.monospacedDigit())
            }
            if let refused = account.quotaSaid {
                Text(refused)
                    .font(.caption2)
                    .foregroundStyle(.orange)
                    .lineLimit(2)
            }
            if let repair = account.repair {
                HStack(spacing: 6) {
                    Button(copied == repair ? "Copiato" : "Copia il comando") {
                        NSPasteboard.general.clearContents()
                        NSPasteboard.general.setString(repair, forType: .string)
                        copied = repair
                    }
                    .controlSize(.small)
                    Text(repair)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
        }
        .padding(.leading, 2)
    }

    /// What the account did, in the order a person asks it: how much, how many
    /// calls, and what it would cost at list price.
    private func said(_ worked: Worked) -> String {
        let weight = worked.atListPrice.map { " · $\($0) a listino" } ?? ""
        return "\(inShort(worked.tokens)) gettoni · \(worked.calls) chiamate in \(worked.sessions) sessioni\(weight)"
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
