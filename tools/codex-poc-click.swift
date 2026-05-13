// Codex-style AX-click proof-of-concept.
//
// Tests the hypothesis we reverse-engineered out of `SkyComputerUseService`:
// pure AX dispatch (`AXUIElementCopyElementAtPosition` → `AXUIElementPerformAction`)
// against a backgrounded app's AX tree should NOT cause the target to come
// to the foreground, NOT steal keyboard focus, NOT raise its windows — across
// a *sequence* of clicks, not just one.
//
// Optional behaviour mirrors two extra things Codex does:
//
//   --toggle-eui   Before the first click, snapshot `AXEnhancedUserInterface`
//                  and force it off if it was on; restore at the end. Codex
//                  does this so coord-based hit-tests resolve to the right
//                  element on Catalyst / Electron targets that re-flow their
//                  AX layout when EUI is on (e.g. when VoiceOver is active).
//
//   --activate-first    Before *all* clicks, call
//                  `[NSRunningApplication activateWithOptions: 0]` — the
//                  polite default activation Codex's `Activate` IPC action
//                  uses. Lets you see the "session starts, target activates
//                  once, then stays active for all subsequent clicks"
//                  pattern in action. WITHOUT this flag, clicks should land
//                  on the backgrounded target with frontmost UNCHANGED.
//
// Usage:
//   swift tools/codex-poc-click.swift <pid> <x,y> [<x,y> ...] \
//       [--delay <seconds>] [--toggle-eui] [--activate-first]
//
// Example (backgrounded Calc, clicks at 7 → 8 → +):
//   swift tools/codex-poc-click.swift 68816 1200,500 1240,500 1280,500
//
// Outputs per click: frontmost before/after, target.isActive before/after,
// the AX role of the hit element, and the AXPress result code.

import Foundation
import AppKit
import ApplicationServices

// MARK: - argument parsing
guard CommandLine.arguments.count >= 3 else {
    print("""
    usage: \(CommandLine.arguments[0]) <pid> <x,y> [<x,y> ...] \\
        [--delay <seconds>] [--toggle-eui] [--activate-first]
    """)
    exit(1)
}
guard let pid = pid_t(CommandLine.arguments[1]) else {
    fputs("invalid pid: \(CommandLine.arguments[1])\n", stderr); exit(1)
}

var clicks: [(Float, Float)] = []
var delay: TimeInterval = 0.5
var toggleEUI = false
var activateFirst = false

var i = 2
while i < CommandLine.arguments.count {
    let arg = CommandLine.arguments[i]
    switch arg {
    case "--delay":
        i += 1
        guard i < CommandLine.arguments.count, let v = Double(CommandLine.arguments[i]) else {
            fputs("--delay needs a number\n", stderr); exit(1)
        }
        delay = v
    case "--toggle-eui":
        toggleEUI = true
    case "--activate-first":
        activateFirst = true
    default:
        let parts = arg.split(separator: ",")
        guard parts.count == 2, let x = Float(parts[0]), let y = Float(parts[1]) else {
            fputs("expected 'x,y' (got '\(arg)')\n", stderr); exit(1)
        }
        clicks.append((x, y))
    }
    i += 1
}

guard !clicks.isEmpty else {
    fputs("no clicks specified\n", stderr); exit(1)
}

// MARK: - AX preflight
guard AXIsProcessTrusted() else {
    fputs("""
    AXIsProcessTrusted() == false.

    Grant Accessibility permission to whatever is running this script
    (probably Terminal / iTerm / kitty). System Settings → Privacy &
    Security → Accessibility.
    """, stderr)
    exit(1)
}

let appRoot = AXUIElementCreateApplication(pid)
let target = NSRunningApplication(processIdentifier: pid)
let targetName = target?.localizedName ?? "<nil>"

print("target: pid=\(pid) name=\(targetName)")
print("initial frontmost: \(NSWorkspace.shared.frontmostApplication?.localizedName ?? "<nil>")")
print("initial target.isActive: \(target?.isActive ?? false)")
print("")

// MARK: - helpers
func axString(_ elem: AXUIElement, _ key: String) -> String? {
    var v: CFTypeRef?
    let r = AXUIElementCopyAttributeValue(elem, key as CFString, &v)
    guard r == .success, let s = v as? String else { return nil }
    return s
}

func axBool(_ elem: AXUIElement, _ key: String) -> Bool? {
    var v: CFTypeRef?
    let r = AXUIElementCopyAttributeValue(elem, key as CFString, &v)
    guard r == .success else { return nil }
    return (v as? Bool)
}

func axParent(_ elem: AXUIElement) -> AXUIElement? {
    var v: CFTypeRef?
    let r = AXUIElementCopyAttributeValue(elem, "AXParent" as CFString, &v)
    guard r == .success, let p = v else { return nil }
    return (p as! AXUIElement)
}

func axActions(_ elem: AXUIElement) -> [String] {
    var arr: CFArray?
    let r = AXUIElementCopyActionNames(elem, &arr)
    guard r == .success, let a = arr as? [String] else { return [] }
    return a
}

// One-line description of an element: role + actions
func describe(_ elem: AXUIElement) -> String {
    let role = axString(elem, "AXRole") ?? "?"
    let sub = axString(elem, "AXSubrole") ?? "-"
    let actions = axActions(elem)
    return "role=\(role) subrole=\(sub) actions=\(actions)"
}

func writeEUI(_ on: Bool) -> AXError {
    let v: CFBoolean = on ? kCFBooleanTrue : kCFBooleanFalse
    return AXUIElementSetAttributeValue(
        appRoot,
        "AXEnhancedUserInterface" as CFString,
        v)
}

// MARK: - optional activation (session-start equivalent)
if activateFirst {
    let r = target?.activate(options: []) ?? false
    print("[activate-first] activate(options: []) -> \(r)")
    // Spin the runloop briefly so the activation notification arrives
    // before we start clicking.
    RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.3))
    print("[activate-first] frontmost after: \(NSWorkspace.shared.frontmostApplication?.localizedName ?? "<nil>")")
    print("[activate-first] target.isActive after: \(target?.isActive ?? false)")
    print("")
}

// MARK: - optional EUI toggle
let priorEUI = toggleEUI ? axBool(appRoot, "AXEnhancedUserInterface") : nil
if toggleEUI {
    print("[eui] prior AXEnhancedUserInterface = \(String(describing: priorEUI))")
    if priorEUI == true {
        let r = writeEUI(false)
        print("[eui] set AXEnhancedUserInterface=false -> \(r.rawValue)")
    }
    print("")
}

// MARK: - click loop
print("--- \(clicks.count) clicks, \(delay)s between ---")
for (idx, (x, y)) in clicks.enumerated() {
    let frontBefore = NSWorkspace.shared.frontmostApplication?.localizedName ?? "<nil>"
    let targetActiveBefore = target?.isActive ?? false

    // 1. AX hit-test on target's app root (not systemWide — that would
    //    return the user's frontend window's element instead).
    var elem: AXUIElement?
    let hitResult = AXUIElementCopyElementAtPosition(appRoot, x, y, &elem)

    // 2. Describe the element and its ancestor chain so we can see
    //    *what* AX exposed at this point — and whether `AXPress` is
    //    even an advertised action.
    var chain: [String] = []
    if hitResult == .success, let e = elem {
        chain.append(describe(e))
        var cursor: AXUIElement? = axParent(e)
        var depth = 0
        while let c = cursor, depth < 4 {
            chain.append(describe(c))
            cursor = axParent(c)
            depth += 1
        }
    }

    // 3. AXPress directly. No activation, no AX-attribute writes,
    //    no synthetic events.
    let pressResult: AXError
    if hitResult == .success, let e = elem {
        pressResult = AXUIElementPerformAction(e, kAXPressAction as CFString)
    } else {
        pressResult = AXError.noValue
    }

    // Brief settle so activation reflexes have a chance to fire (and
    // be detected) before we sample frontmost / isActive again.
    Thread.sleep(forTimeInterval: 0.1)

    let frontAfter = NSWorkspace.shared.frontmostApplication?.localizedName ?? "<nil>"
    let targetActiveAfter = target?.isActive ?? false

    let stolenFocus = (frontBefore != frontAfter)
    let activated = (!targetActiveBefore && targetActiveAfter)

    print("\nclick[\(idx)] @ (\(x), \(y))")
    print("  hit:    \(hitResult == .success ? "ok" : "err=\(hitResult.rawValue)")")
    for (i, line) in chain.enumerated() {
        let prefix = (i == 0) ? "    " : String(repeating: "    ", count: i + 1) + "↑ "
        print("\(prefix)\(line)")
    }
    print("  press:  \(pressResult == .success ? "ok" : "err=\(pressResult.rawValue)")")
    print("  front:  \(frontBefore) → \(frontAfter)\(stolenFocus ? "   ⚠️ FOCUS CHANGED" : "")")
    print("  active: \(targetActiveBefore) → \(targetActiveAfter)\(activated ? "   ⚠️ TARGET ACTIVATED" : "")")

    if idx < clicks.count - 1 {
        Thread.sleep(forTimeInterval: delay)
    }
}

// MARK: - restore EUI
if toggleEUI && priorEUI == true {
    let r = writeEUI(true)
    print("\n[eui] restored AXEnhancedUserInterface=true -> \(r.rawValue)")
}

// MARK: - summary
print("\nfinal frontmost: \(NSWorkspace.shared.frontmostApplication?.localizedName ?? "<nil>")")
print("final target.isActive: \(target?.isActive ?? false)")
