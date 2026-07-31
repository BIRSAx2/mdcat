// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

// Prints "<window id>\t<owner pid>" for the largest on-screen window owned by a Ghostty
// process, excluding the given PID (the live session's own window, if any). Used by
// scripts/screenshot-ghostty to find the window it just opened without touching any other
// Ghostty window. Ghostty also spawns a small invisible helper window alongside the real
// terminal window, hence picking the largest by area rather than the first match.

import CoreGraphics
import Foundation

let excludePid = CommandLine.arguments.count > 1 ? Int(CommandLine.arguments[1]) ?? -1 : -1

guard
    let list = CGWindowListCopyWindowInfo([.excludeDesktopElements], kCGNullWindowID)
        as? [[String: AnyObject]]
else {
    exit(1)
}

var best: (windowId: Int, pid: Int, area: Int)?
for window in list {
    let owner = window[kCGWindowOwnerName as String] as? String ?? ""
    guard owner.lowercased().contains("ghostty") else { continue }
    let pid = window[kCGWindowOwnerPID as String] as? Int ?? -1
    guard pid != excludePid else { continue }
    let windowId = window[kCGWindowNumber as String] as? Int ?? -1
    let bounds = window[kCGWindowBounds as String] as? [String: Int] ?? [:]
    let area = (bounds["Width"] ?? 0) * (bounds["Height"] ?? 0)
    if best == nil || area > best!.area {
        best = (windowId, pid, area)
    }
}

if let best {
    print("\(best.windowId)\t\(best.pid)")
}
