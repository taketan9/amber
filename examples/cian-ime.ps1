# cian-ime.ps1 — read or set the Windows IME's open state.
#
#   powershell -NoProfile -File cian-ime.ps1          → prints "on" or "off"
#   powershell -NoProfile -File cian-ime.ps1 off      → turns the IME off
#   powershell -NoProfile -File cian-ime.ps1 on       → turns it back on
#
# The Windows half of `cian.ime{}`, and the twin of `cian-ime.swift`. It
# exists for the same reason: the setup should cost a compile or nothing at
# all. `im-select` does the same job and is a third-party download; this is
# the system call both of them make.
#
# **Windows is not macOS here.** On a Mac you switch the *input source*, which
# is a global setting with an id like `com.apple.keylayout.ABC`. On Windows
# the IME is open or closed **per window**, and the id of the keyboard layout
# is a different thing again — switching layouts is not what turns 日本語入力
# off. So this reads and writes the open state of whatever window has the
# focus, and its two ids are the words `on` and `off`.
#
# That still fits `cian.ime{}` exactly, because the contract there is only
# "print an id, and take one back": cian never looks inside the string.
#
#   cian.ime{
#     helper = [[powershell -NoProfile -File C:\Users\you\cian-ime.ps1]],
#     off    = "off",
#   }
#
# The foreground window is cian's own while cian is the one asking, which is
# the only moment this is ever called.

param([string]$Want)

$sig = @'
using System;
using System.Runtime.InteropServices;

public static class CianIme {
    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    // The IME's own window for a given one. Everything below hangs off it.
    [DllImport("imm32.dll")]
    public static extern IntPtr ImmGetDefaultIMEWnd(IntPtr hWnd);

    [DllImport("user32.dll", CharSet = CharSet.Auto)]
    public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    // WM_IME_CONTROL, with the two sub-commands that read and write the
    // open/closed state. 0x005 is get, 0x006 is set.
    const uint WM_IME_CONTROL = 0x0283;
    static readonly IntPtr IMC_GETOPENSTATUS = (IntPtr)0x005;
    static readonly IntPtr IMC_SETOPENSTATUS = (IntPtr)0x006;

    public static bool IsOpen() {
        IntPtr ime = ImmGetDefaultIMEWnd(GetForegroundWindow());
        if (ime == IntPtr.Zero) { return false; }
        return SendMessage(ime, WM_IME_CONTROL, IMC_GETOPENSTATUS, IntPtr.Zero) != IntPtr.Zero;
    }

    public static bool Set(bool open) {
        IntPtr ime = ImmGetDefaultIMEWnd(GetForegroundWindow());
        if (ime == IntPtr.Zero) { return false; }
        SendMessage(ime, WM_IME_CONTROL, IMC_SETOPENSTATUS, (IntPtr)(open ? 1 : 0));
        return true;
    }
}
'@

Add-Type -TypeDefinition $sig -Language CSharp | Out-Null

if ([string]::IsNullOrWhiteSpace($Want)) {
    # No argument: say which it is. cian remembers this and hands it back.
    if ([CianIme]::IsOpen()) { Write-Output "on" } else { Write-Output "off" }
    exit 0
}

$want = $Want.Trim().ToLowerInvariant()
if ($want -ne "on" -and $want -ne "off") {
    # Anything else is a mistake worth naming, not something to guess at.
    Write-Error "cian-ime: expected 'on' or 'off', got '$Want'"
    exit 1
}

if (-not [CianIme]::Set($want -eq "on")) {
    Write-Error "cian-ime: the focused window has no IME to switch"
    exit 1
}
exit 0
