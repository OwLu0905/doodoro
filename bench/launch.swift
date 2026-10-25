// Launches an executable and waits for its first window wider than 100 points to come on screen.
// Prints "<pid> <milliseconds>", and leaves the app running.
//
// `launch --responsible <pid>...` prints each process with the one macOS holds responsible for it
// instead: an XPC service's host app, or the terminal both were started from.

import CoreGraphics
import Foundation

let args = CommandLine.arguments
if args.count > 1 && args[1] == "--responsible" {
    typealias Responsible = @convention(c) (Int32) -> Int32
    let symbol = dlsym(UnsafeMutableRawPointer(bitPattern: -2), "responsibility_get_pid_responsible_for_pid")!
    let responsible = unsafeBitCast(symbol, to: Responsible.self)
    for pid in args.dropFirst(2) { print(pid, responsible(Int32(pid) ?? 0)) }
    exit(0)
}
guard args.count > 1 else {
    FileHandle.standardError.write("usage: launch <executable>\n".data(using: .utf8)!)
    exit(2)
}
let app = Process()
app.executableURL = URL(fileURLWithPath: args[1])
app.environment = ProcessInfo.processInfo.environment
app.standardOutput = FileHandle.nullDevice
app.standardError = FileHandle.nullDevice

let start = DispatchTime.now()
try app.run()
let pid = app.processIdentifier

func visible() -> Bool {
    guard let list = CGWindowListCopyWindowInfo(.optionOnScreenOnly, kCGNullWindowID) as? [[String: Any]] else { return false }
    return list.contains { w in
        guard (w[kCGWindowOwnerPID as String] as? Int32) == pid,
              let b = w[kCGWindowBounds as String] as? [String: Double] else { return false }
        return (b["Width"] ?? 0) > 100
    }
}

while !visible() {
    if DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds > 20_000_000_000 {
        print("\(pid) timeout")
        exit(1)
    }
    usleep(2000)
}
let ms = Double(DispatchTime.now().uptimeNanoseconds - start.uptimeNanoseconds) / 1e6
print("\(pid) \(String(format: "%.0f", ms))")
