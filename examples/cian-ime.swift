// cian-ime — read or set the macOS keyboard input source.
//
//   cian-ime                 → prints the current input source id
//   cian-ime <input-source>  → switches to it (exit 1 if there is no such one)
//
// This is the whole helper cian.ime{} needs on macOS. It exists so the setup
// costs nothing but a compile: macism and im-select do the same job, but both
// are a third-party install, and this is thirty lines of the system API they
// both call.

import Carbon
import Foundation

func currentSourceID() -> String {
    guard let src = TISCopyCurrentKeyboardInputSource()?.takeRetainedValue(),
          let raw = TISGetInputSourceProperty(src, kTISPropertyInputSourceID)
    else { return "" }
    return Unmanaged<CFString>.fromOpaque(raw).takeUnretainedValue() as String
}

let args = CommandLine.arguments
guard args.count > 1 else {
    print(currentSourceID())
    exit(0)
}

let wanted = args[1]
// Enabled sources first: selecting one that is merely installed fails, and the
// error is clearer if we say so ourselves.
func find(_ id: String, installedToo: Bool) -> TISInputSource? {
    let filter = [kTISPropertyInputSourceID as String: id] as CFDictionary
    let list = TISCreateInputSourceList(filter, installedToo)?.takeRetainedValue()
    return (list as? [TISInputSource])?.first
}

guard let source = find(wanted, installedToo: false) ?? find(wanted, installedToo: true) else {
    FileHandle.standardError.write(
        "cian-ime: no input source \"\(wanted)\" (current: \(currentSourceID()))\n"
            .data(using: .utf8)!)
    exit(1)
}

let status = TISSelectInputSource(source)
if status != noErr {
    FileHandle.standardError.write(
        "cian-ime: could not select \"\(wanted)\" (OSStatus \(status))\n".data(using: .utf8)!)
    exit(2)
}
