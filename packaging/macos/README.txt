cian — macOS
============

FIRST, READ THIS: macOS will refuse to open cian.app
----------------------------------------------------

The dialog says cian.app "cannot be opened" and offers only Done and Move
to Bin. Nothing is wrong with the download. macOS marks anything a browser
delivered and refuses to run what Apple has not certified, and cian is not
signed with an Apple Developer certificate. Since Sequoia there is no
right-click -> Open around it either.

One command clears it. In Terminal, type the following, drag cian.app onto
the Terminal window so the path fills itself in, and press Return:

    xattr -dr com.apple.quarantine

It prints nothing when it works. Double-click cian.app after that and it
opens, this time and every time.

If you would rather not use Terminal: open System Settings -> Privacy &
Security, scroll to the bottom, and press Open Anyway. The button is there
for about an hour after you tried to open the app, so try it first.

Contents
  cian.app          The window build. Double-click it.
  cian-tui          The terminal build. Run it inside a terminal you
                    already have — this is the one that works over ssh
                    and inside tmux.
  examples/init.lua Optional starter config.
  README.md         The full manual, in English.
  README.ja.md      The same, in Japanese.

Running the terminal build
  From a terminal, in this folder:
      ./cian-tui

  To type `cian-tui` from anywhere, put it somewhere on your PATH:
      mkdir -p ~/.local/bin && cp cian-tui ~/.local/bin/

  The quarantine applies to this one too, and the same command clears it:
      xattr -d com.apple.quarantine cian-tui

Notes
  - For the file-type icons, use a terminal with a Nerd Font — WezTerm and
    iTerm2 are good choices. Without one you will see boxes instead of
    icons. The window build carries its own font and needs nothing.
  - Configuration lives at ~/.config/cian/init.lua (override the directory
    with the CIAN_CONFIG_DIR environment variable). Put init.lua next to
    the executable instead and that copy wins, so a folder on a USB stick
    travels with its own settings.
  - Press ? inside cian for the full key list.
  - Uninstall: delete cian.app, cian-tui, and ~/.config/cian.
