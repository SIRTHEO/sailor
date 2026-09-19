import Foundation

/// Where the binary in service lives. The dot asks the same `sailor` a person
/// asks: a dot reading a different build would show a different truth.
let sailorInService = FileManager.default.homeDirectoryForCurrentUser
    .appending(path: ".config/sailor/bin/sailor")

/// Run `sailor accounts --json` and hand back what it said.
///
/// A dot that threw on a missing binary would vanish from the menu bar with no
/// word; every failure here becomes a reading that says «not known» instead.
func askSailor(hours: Int) async -> Reading {
    guard FileManager.default.isExecutableFile(atPath: sailorInService.path) else {
        return .quiet
    }
    let task = Process()
    task.executableURL = sailorInService
    task.arguments = ["accounts", "--json", "--quota", "--hours", String(hours)]
    let pipe = Pipe()
    task.standardOutput = pipe
    task.standardError = FileHandle.nullDevice
    do {
        try task.run()
    } catch {
        return .quiet
    }
    let said = pipe.fileHandleForReading.readDataToEndOfFile()
    task.waitUntilExit()
    guard task.terminationStatus == 0 else { return .quiet }
    return (try? JSONDecoder().decode(Reading.self, from: said)) ?? .quiet
}
