---
description: How Termalith stores passwords (system keychain, optional master password), the portable copy for USB sticks, where your data lives on each system, and what Termalith never sends.
---

# Passwords and your data

## Saved passwords

Passwords, key passphrases and proxy passwords are kept in your system's
credential store, never in Termalith's own files, exports or logs:

| System | Store |
|---|---|
| macOS | Keychain (entries named *Termalith*) |
| Windows | Credential Manager |
| Linux | Secret Service: GNOME Keyring, KWallet or compatible |

A password is saved only when you tick **Save in the system keychain** and
the login worked. If the keychain refuses to save, the connection stays open
and Termalith tells you; if it refuses to read, you are asked for the
password instead. On Linux without a Secret Service, passwords are asked
each time and never written to disk.

## Master password

**Settings → Master password** adds a second lock: saved passwords stay in
the system keychain, but encrypted with a key that only your master
password opens (Argon2id and XChaCha20-Poly1305, standard algorithms).

- Termalith asks for the master password at startup. Skip the question and
  servers ask for their passwords as if none were saved.
- **Lock now** forgets the key until you unlock again.
- A forgotten master password cannot be recovered. **Forgot the master
  password?** deletes the saved passwords so you can start over; your
  sessions are kept.
- There is no automatic lock after inactivity yet.

## Portable copy

To carry Termalith on a USB stick, put the app on the stick and create a
folder named **TermalithData** next to it:

- Windows: next to `Termalith.exe`.
- Linux: next to the `.AppImage` file.
- macOS: next to `Termalith.app`.

Sessions, host keys, settings, logs and crash reports are then kept in that
folder, not on the computer. Passwords are saved there only after you set a
master password, encrypted with it, because the computer's keychain does
not travel with the stick.

## Where your data is

| What | macOS | Windows | Linux |
|---|---|---|---|
| Sessions, settings, host keys (`termalith.db`) | `~/Library/Application Support/local.termalith.desktop/` | `%APPDATA%\local.termalith.desktop\` | `~/.local/share/local.termalith.desktop/` |
| Crash reports | `~/Library/Logs/local.termalith.desktop/crashes/` | `%LOCALAPPDATA%\local.termalith.desktop\logs\crashes\` | `~/.local/share/local.termalith.desktop/logs/crashes/` |
| Session logs (when you turn logging on) | `~/Documents/Termalith Logs/` | `Documents\Termalith Logs\` | `~/Documents/Termalith Logs/` |
| Downloads from the Files panel | your Downloads folder | your Downloads folder | your Downloads folder |

In portable mode everything except downloads is in `TermalithData`.
Back up your sessions with **Import → Export sessions…**.

## What Termalith never does

- **No telemetry, no account.** Termalith connects only to the hosts you
  choose.
- **No automatic updates or downloads.** The update check runs only when
  you ask, or at startup if you turn it on; it reads the latest version from
  GitHub and links to it.
- **Crash reports stay local.** After a crash, the next start shows the
  report (version, system, where the error happened; no passwords or
  terminal contents). You decide whether to copy it into a GitHub issue.
- **No commands from the interface.** The app never runs a shell command
  line on your computer on its own; SSH forwarding of your agent or X11 is
  off unless you turn it on per session.

More technical detail: [Security](../security.md).
