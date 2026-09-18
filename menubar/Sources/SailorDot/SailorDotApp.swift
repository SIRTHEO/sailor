import SwiftUI

/// How often the dot asks again. Reading the store costs nothing, but a dot
/// that spawned a process every second would be its own load on the machine.
let askAgainEvery: Duration = .seconds(60)

/// How far back the spending is summed in the menu.
let hoursShown = 24

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
            Image(systemName: Standing.mark(accounts.reading.worst))
        }
        .menuBarExtraStyle(.window)
        .onChange(of: accounts.reading.readAt, initial: true) { _, _ in }
    }
}

struct Menu_: View {
    @ObservedObject var accounts: Accounts

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Account, ultime \(hoursShown)h")
                .font(.headline)
            if accounts.reading.accounts.isEmpty {
                Text("Sailor non ha ancora risposto")
                    .foregroundStyle(.secondary)
            }
            ForEach(accounts.reading.accounts) { account in
                Row(account: account)
            }
            Divider()
            Button("Aggiorna adesso") {
                Task { accounts.reading = await askSailor(hours: hoursShown) }
            }
            Button("Esci") { NSApplication.shared.terminate(nil) }
        }
        .padding(12)
        .frame(width: 380)
        .task { accounts.watch() }
    }
}

struct Row: View {
    let account: Account

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: Standing.mark(account.standing))
                .foregroundStyle(account.standing == "ready" ? .green : .orange)
            VStack(alignment: .leading, spacing: 1) {
                Text("\(account.cli) · \(account.profile ?? "dal terminale")")
                    .fontWeight(account.active ? .semibold : .regular)
                Text("\(Standing.said(account.standing)) · $\(account.spent) · \(account.calls) chiamate · \(ago(account.lastCallAgoS))")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
        }
    }
}
