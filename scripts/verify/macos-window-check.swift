// scripts/verify/macos-window-check.swift — assert the packaged app has an
// on-screen window WITHOUT needing Automation/Accessibility TCC.
//
// Uses CGWindowListCopyWindowInfo (no TCC required to read other apps' window
// metadata). Matches by process name ("macos-emitter") or the bundle's display
// name prefix ("Wavelink …"). Exit 0 = ≥1 on-screen window, 2 = none,
// 3 = could not query. Built on the fly by macos-launch-smoke.sh (swiftc is a
// packaging prerequisite).
import CoreGraphics
import Foundation

let args = CommandLine.arguments
let processName = args.count > 1 ? args[1] : "macos-emitter"
let displayPrefix = args.count > 2 ? args[2] : "Wavelink"

let opts = CGWindowListOption(arrayLiteral: .optionOnScreenOnly)
guard let infos = CGWindowListCopyWindowInfo(opts, kCGNullWindowID) as? [[String: Any]] else {
    print("could not query window list")
    exit(3)
}
var found = false
for w in infos {
    // Content windows live at layer 0; menu bars/Dock/windows of other apps are
    // higher layers and must not count as "the app opened a window".
    guard ((w[kCGWindowLayer as String] as? Int) ?? 1) == 0 else { continue }
    guard let owner = w[kCGWindowOwnerName as String] as? String else { continue }
    if owner == processName || owner.hasPrefix(displayPrefix) {
        found = true
        print("window: \"\(owner)\" bounds=\(w[kCGWindowBounds as String] ?? "")")
    }
}
print(found ? "PASS: on-screen window present" : "FAIL: no on-screen window")
exit(found ? 0 : 2)
