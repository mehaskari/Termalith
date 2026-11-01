# User Guide

Termalith keeps your servers, network devices and tools in one window: saved
sessions on the left, tabs on the right. This guide covers everything that
works in version 0.1.

!!! note "Preview"
    Termalith is an early preview. Most features are used daily on macOS;
    Windows and Linux builds pass automated tests but have had little
    hands-on use. Please [report problems](https://github.com/mehaskari/Termalith/issues).

## Install

Download the installer for your system from the
[Releases page](https://github.com/mehaskari/Termalith/releases):

| System | File | First launch |
|---|---|---|
| macOS (Apple Silicon) | `Termalith_x.y.z_aarch64.dmg` | The app is not signed yet: right-click it and choose **Open**. |
| Windows 10/11 | `…_x64-setup.exe` or `….msi` | SmartScreen may warn: **More info → Run anyway**. |
| Linux | `….AppImage` or `….deb` | AppImage: make it executable (`chmod +x`) and run it. |

On Linux, Termalith needs WebKitGTK 4.1 (installed with most desktops) and,
for saved passwords, a Secret Service such as GNOME Keyring or KWallet.

### Build from source

You need Rust (the version in `rust-toolchain.toml`), Node.js 24 with
Corepack and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
git clone https://github.com/mehaskari/Termalith.git && cd Termalith
corepack pnpm install
corepack pnpm dev                                        # run
corepack pnpm --filter @termalith/desktop tauri build   # installer for this OS
```

## The window

- **Sidebar:** Quick connect, a local terminal (the ▾ next to **Local** picks
  a shell), serial consoles, and your saved sessions as a folder tree.
- **Tabs:** every connection opens in a tab. Right-click a tab to rename,
  duplicate, save its output or close other tabs.
- **Toolbar above a terminal:** split, broadcast, log, snippets, and for SSH
  tabs **Tunnels** and **Files**.
- **Command palette:** <kbd>Cmd</kbd>+<kbd>K</kbd> (macOS) or
  <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>K</kbd> searches tabs, sessions and
  actions.

## Sessions

Click **+** next to *Sessions* to save a connection. A session has a type
(SSH, RDP, VNC or Telnet), a host and port, a user name and options.

- **Folders:** type a folder name; use `/` for subfolders, e.g.
  `Production/DB`. Folders open and close with a click, show how many
  sessions they hold, and have **Open all** on hover.
- **Favorites and tags:** the ☆ button pins a session to *Favorites*; tags
  (comma-separated) appear under the session. Click a tag, or type `#name` in
  the search box, to filter.
- **Quick connect** accepts `user@host:port` in the host field.
- **Duplicate, edit, delete** are on each row when you hover it.

### Import and export

- **Import ~/.ssh/config:** reads your OpenSSH config and lets you choose
  hosts. Supported: `HostName`, `User`, `Port`, `IdentityFile`, single-hop
  `ProxyJump`, `ForwardAgent yes`, `ForwardX11 yes`. Anything else is listed
  per entry and not imported.
- **Import… (Termalith or MobaXterm):** paste a Termalith export or the
  contents of a MobaXterm `.mxtsessions` file. SSH, RDP, VNC and Telnet
  sessions are imported with their folders; other kinds are listed as not
  imported.
- **Export…** writes all sessions as JSON. Exports never contain passwords
  or keychain references.

## SSH

### Logging in

Choose how the session authenticates:

- **Password.** You are asked when you connect. Tick *Save in the system
  keychain* to store it; it is saved only after the login worked.
- **Private key file.** If the key has a passphrase you are asked for it
  (and may save it).
- **SSH agent.** Uses the keys in your running `ssh-agent` (or the OpenSSH
  agent on Windows).

Servers that ask extra questions — a PAM password prompt or a one-time code
from an authenticator app — show them in a dialog. A password you already
entered is reused for a plain password prompt; codes are always asked.

### Host keys

The first time you connect to a server, its key fingerprint is shown: choose
**Trust and connect** (remembered), **Connect once**, or **Don't connect**.
Keys you already accepted in OpenSSH (`~/.ssh/known_hosts`) are trusted too.

If a server's key **changes**, Termalith refuses to connect and shows both
fingerprints. Only after you have confirmed the change with the server's
administrator, use **Remove recorded key…** and connect again.

### Jump hosts and proxies

- **Jump host:** in a session, choose *Connect via (jump host)* another
  saved SSH session. Each hop's host key is verified. A jump host must log in without
  questions (saved password, key or agent).
- **Proxy:** *Settings → Proxy* sends SSH and Telnet connections through a
  SOCKS5 or HTTP CONNECT proxy (no proxy authentication yet). Only the first
  hop uses it.

### Options per session

- **Run after login:** one command typed into the shell when it is ready,
  e.g. `cd /var/log` or `tmux attach || tmux`.
- **Forward my SSH agent:** lets the server use your agent's keys while you
  are connected (for `git` or further SSH hops). Anyone with root on the
  server can use them too, so enable it only for hosts you trust.
- **Forward X11:** graphical programs on the server open windows on your
  screen. You need a local X server: XQuartz on macOS, VcXsrv or Xming on
  Windows. The server only receives a random fake cookie.

### Reconnect

If a connection drops (not when you type `exit`), the tab shows *Connection
lost. Reconnecting in 5 s…* with a **Cancel** button. Turn this off in
Settings.

### Tunnels

**Tunnels** on an SSH tab:

| Kind | Like | What it does |
|---|---|---|
| Local | `ssh -L` | A local port reaches a host:port from the server's side. |
| Remote | `ssh -R` | A port on the server reaches a host:port on your machine. |
| SOCKS | `ssh -D` | A SOCKS5 proxy on your machine that goes out through the server. |

Tunnels listen on `127.0.0.1` unless you tick *Allow other machines to
connect* (for remote tunnels: *Listen on all server interfaces*). They close
with the tab.

### SSH keys

**SSH keys** (sidebar footer) lists the key pairs in `~/.ssh`, generates a
new Ed25519 key (optionally with a passphrase), copies a public key, and
**Install on server** adds it to `~/.ssh/authorized_keys` on a connected
server (the folder and file get safe permissions; a key is never added
twice).

## Files

- **Files** on an SSH tab opens the SFTP panel; **⇅** on a session opens a
  file-browser tab without a shell (**Open terminal** adds a shell on the
  same connection).
- **Download** saves to your Downloads folder; **upload** by dropping files
  onto the window.
- Up to three transfers run at once; connection errors are retried
  automatically and failed transfers have **Retry**.
- **New folder**, **Rename** and **Delete** (files and empty folders; there
  is no recursive delete).
- **⏱ Transfer history** lists recent downloads and uploads (all
  connections, the last 500) with their outcome; **Clear** empties it. Only
  names, paths and sizes are kept — never file contents.
- **Compare…** (row menu ⋯) on one text file, then **Compare with …** on
  another, opens both side by side with the differences highlighted.
- **✎ Edit** opens a text file (UTF-8, up to 2 MiB) in an editor tab.
  <kbd>Cmd</kbd>/<kbd>Ctrl</kbd>+<kbd>S</kbd> saves it back with its
  permissions. If someone changed the file on the server meanwhile, the save
  is refused and you can reload or overwrite.

## Remote desktop

RDP and VNC sessions open in the client installed on your computer:

| System | RDP | VNC |
|---|---|---|
| macOS | Windows App / Microsoft Remote Desktop | Screen Sharing (built in) |
| Windows | Remote Desktop Connection (built in) | TigerVNC |
| Linux | FreeRDP (`xfreerdp`) | TigerVNC (`vncviewer`) |

The session dialog shows which client will be used, or how to install one.
Choose *Tunnel through* an SSH session to reach a desktop behind a bastion.
Passwords are never passed on a command line; the client asks for them
(FreeRDP on Linux receives it on standard input).

## Serial and Telnet

- **Serial:** pick a port (COM*n* or `/dev/tty…`), speed, data bits, parity,
  stop bits and flow control. On Linux your user usually needs to be in the
  `dialout` group.
- **Telnet:** for devices that offer nothing else. Telnet is **not
  encrypted**; choose a jump host so that only the last leg — from the jump
  host to the device — is unencrypted.

## Network tools

**Network tools** (sidebar footer) has ping, traceroute, a TCP port check,
a port scan (up to 1024 ports, 64 at a time — only scan hosts you may
test), DNS lookup, an IPv4 subnet calculator and Wake-on-LAN.

## Working in terminals

- **Split** a tab side by side or top and bottom; SSH splits share the
  connection.
- **Broadcast…** types into several terminals at once; the source shows a
  red banner and targets are marked.
- **Snippets:** saved commands with `{{variables}}`; you see exactly what
  will be typed and choose whether to press Enter.
- **Log** saves a tab's output as plain text in `Documents/Termalith Logs`
  (or for every new terminal, in Settings). **Save output…** (tab menu)
  saves what is on screen and in the scrollback.
- **Links:** <kbd>Cmd</kbd>/<kbd>Ctrl</kbd>+click opens an `http(s)` link in
  your browser.

### Keyboard shortcuts

On Windows and Linux use <kbd>Ctrl</kbd>+<kbd>Shift</kbd> where macOS uses
<kbd>Cmd</kbd>, so programs in the terminal keep their <kbd>Ctrl</kbd> keys.

| Action | macOS | Windows / Linux |
|---|---|---|
| Command palette | <kbd>Cmd</kbd>+<kbd>K</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>K</kbd> |
| New local terminal | <kbd>Cmd</kbd>+<kbd>T</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>T</kbd> |
| Close tab | <kbd>Cmd</kbd>+<kbd>W</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>W</kbd> |
| Next / previous tab | <kbd>Ctrl</kbd>+<kbd>Tab</kbd> / <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>Tab</kbd> | same |
| Go to tab 1–9 | <kbd>Cmd</kbd>+<kbd>1</kbd>…<kbd>9</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>1</kbd>…<kbd>9</kbd> |
| Find | <kbd>Cmd</kbd>+<kbd>F</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>F</kbd> |
| Copy / paste | <kbd>Cmd</kbd>+<kbd>C</kbd> / <kbd>V</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>C</kbd> / <kbd>V</kbd> |
| Font size | <kbd>Cmd</kbd>+<kbd>+</kbd> / <kbd>-</kbd> / <kbd>0</kbd> | <kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>+</kbd> / <kbd>-</kbd> / <kbd>0</kbd> |

## Settings

App theme and terminal colors (Solarized, Nord, Gruvbox, Tomorrow Night),
font, cursor, scrollback, default shell, copy-on-select, right-click paste,
automatic reconnect, logging every terminal, and the outgoing proxy.

## Troubleshooting

**"No SSH agent is available"** — start an agent and add a key
(`ssh-add`). On macOS the system agent is available to apps automatically;
on Linux, apps started from the desktop need `SSH_AUTH_SOCK` from your
session.

**Passwords are not saved on Linux** — Termalith needs a Secret Service
(GNOME Keyring, KWallet). Without one, passwords are asked each time and
never written to disk.

**Host key changed** — this can mean an attack, or a reinstalled server.
Confirm the new fingerprint with the server's administrator before removing
the recorded key.

**X11 windows do not appear** — install and start XQuartz (macOS, log out
and in once) or VcXsrv/Xming (Windows), and make sure the server allows
X11 forwarding (`X11Forwarding yes`) and has `xauth` installed.

**RDP/VNC does nothing** — the session dialog says which client is used;
install it if it is missing. On macOS the client keeps its own window, so
Termalith cannot tell when you disconnect.

**Serial port: permission denied (Linux)** — add yourself to the `dialout`
group and log in again.
