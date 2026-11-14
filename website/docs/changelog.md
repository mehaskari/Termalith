# Changelog

## Unreleased

- **Many more icons.** The icon picker now offers about 190 pictures in
  groups, with search and 13 colors. Icons chosen before keep working.

## 0.5.9 (2026-11-14)

- **Browse files as root.** On SSH, **root** in the Files panel switches
  file access to root through sudo (password asked once, never saved).
- **New version notice.** When a newer Termalith is out, a small notice in
  the corner offers **Download** or **Later** (that version is not shown
  again). The startup check is now on by default for everyone, including
  existing installs; turn it off in *Settings → Tell me when a new version
  is available*. Nothing is downloaded automatically.
- **Shared session files.** *Settings → Shared sessions* reads a team's
  session export from a shared folder or an https:// address; its sessions
  appear read-only under their own folder and refresh at every start.
- **Bandwidth test.** *Tools → Bandwidth (iperf3)* measures upload or
  download speed to any iperf3 server.
- **LAN scan.** *Tools → LAN scan* lists the hosts on your subnet with
  their open common ports and hardware addresses; no administrator rights
  needed.
- **New-output dot on tabs.** A tab that got output while you were
  elsewhere shows a small dot until you open it.
- **Risky pastes are shown first.** Several lines, a trailing line break,
  `curl … | sh`, or invisible and control characters open a short check
  with the text before it reaches the shell (*Settings → Warn before
  pasting several lines or risky text*).
- **Command-line switches.** `--session NAME`, `--folder PATH`,
  `--connect "ssh …"` or a URL such as `ssh://me@host` open sessions when
  Termalith starts, e.g. from a desktop shortcut or a script.
- **Icons for sessions and folders.** Pick a picture (server, router,
  switch, firewall, database, cloud and more) and a color in the session
  dialog, or right-click a folder → *Icon…*. Session icons also show on
  their tabs.
- **Connect by typing.** Type `ssh -p 2222 me@host`, `telnet host 23`,
  `rdp admin@win` or a URL like `ssh://me@host` in the command palette
  (Cmd/Ctrl+K) or the Quick connect host field. SSH connects at once;
  other types open New session filled in.
- **Broadcast to a whole folder, side by side.** Right-click a folder →
  *Open all and type into them at once…* opens its SSH and Telnet sessions
  and broadcasts your typing to all of them. The terminals can be shown
  side by side while broadcasting, and the broadcast dialog has
  **Select all**.

## 0.5.8 (2026-11-12)

- **File servers: login for HTTP, shown for every protocol.** The new
  server form always has a *Login* section: FTP asks for a user name and
  password, HTTP can now require one (**Require a user name and password**),
  and TFTP explains that the protocol has none. Running servers show their
  login, or *none*.

## 0.5.7 (2026-11-12)

- **File servers: choose the folder and the FTP login.** The folder to share
  is shown in full before you start, with **Choose folder…** (the system
  folder dialog) and **Use default folder**; each running server shows the
  folder it shares. For FTP you can set the user name and password (empty
  password: a random one), and the running server shows both. Protocol and
  port fields line up.

## 0.5.6 (2026-11-11)

- **Built-in FTP server:** *Tools → File servers* now also offers FTP
  (port 2121), with user `termalith` and a new random password on every
  start; read-only unless uploads are allowed, never deletes, renames or
  replaces files.
- **Built-in TFTP and HTTP servers:** *Tools → File servers* shares a
  folder of this computer, e.g. to copy firmware to a switch over TFTP or to
  let a server `curl` a file. Only this computer can connect unless you
  allow other computers; uploads are off unless you turn them on, and never
  replace a file. Each server shows its addresses and recent requests.

## 0.5.5 (2026-11-10)

- **Pause and resume downloads:** ⏸ pauses a download and ▶ continues it
  where it stopped, like a download manager (SFTP, FTP and FTPS, files and
  folders). Retries after a dropped connection also continue instead of
  starting over; a file that changed on the server starts again.
- **Folder downloads start at once:** files arrive while the folder is
  still being listed, and the transfer shows how much has arrived. Large
  folders (such as a website's `public_html`) no longer stop at 10,000
  entries; a retry keeps the files already downloaded.

## 0.5.4 (2026-11-10)

- **Download folders:** *⋯ → Download folder* in the file browser saves a
  remote folder with its subfolders, over SFTP, FTP and FTPS. Before, only
  files could be downloaded.
- **Built-in RDP viewer: the remote pointer's shapes.** The pointer now
  shows the remote computer's cursor (resize arrows on window edges, text
  beam, busy), so windows can be resized by their edges. Drags keep going
  when the pointer leaves the tab.

## 0.5.3 (2026-11-10)

- **Built-in VNC viewer and RealVNC Server or macOS Screen Sharing:** servers
  that announce a newer protocol version (RealVNC 5.0, macOS 3.889) failed
  with "no configured security type is supported for RFB protocol 3.3".
  Termalith now answers them with protocol 3.8. If the server offers only
  its own account login, the message says which setting enables a VNC
  password.
- **Windows: RealVNC Viewer** is used for VNC sessions when it is installed
  (as well as TigerVNC).

## 0.5.2 (2026-11-10)

- **Windows: finding Remote Desktop Connection** no longer depends on the
  environment Termalith was started with: the Windows folder is read from
  the registry. If no client is found, the message lists every place that
  was searched.
- **RDP and VNC without a client:** when no external client is installed
  (for example a Windows image without Remote Desktop Connection), the
  session opens in the built-in viewer with a notice, instead of failing
  with "No RDP client found".

## 0.5.1 (2026-11-10)

- **Windows:** Remote Desktop Connection is found wherever Windows is
  installed (not only on `C:`), and "No RDP client found" now points to the
  built-in viewer in Settings, which needs no client.
- **New user guide:** fourteen pages with screenshots: download and
  install (which file for which computer), first connection, sessions, SSH,
  terminals, files, remote desktop, serial/Telnet/tools, passwords and your
  data, every setting, shortcuts, coming from MobaXterm, troubleshooting.
- **Download page:** the website offers the right file for your computer
  and lists every download with its size, always for the latest release.
- **Documentation stays current:** CI checks that settings, session types,
  versions and guide links match the app, and that user-facing changes come
  with guide or changelog updates.

## 0.5.0 — first release (2026-11-10)

The first stable release, after four release candidates. Free and open source
(MIT), for macOS, Windows and Linux. Added since the first preview builds:

- **VNC inside Termalith:** VNC sessions can open in a tab too (experimental).
- **Per-session look:** sessions can have their own terminal colors and font.
- **Mosh:** SSH sessions can connect with the installed Mosh client.
- **Portable copy:** a `TermalithData` folder next to the app keeps all data
  there, for running from a USB stick.
- **Master password:** optionally protect saved passwords with a password
  asked for at startup.
- **Sharp RDP on Retina screens:** the embedded viewer asks for the desktop in
  device pixels with the matching display scale, so text is sharp and keeps
  its size.
- **RDP passwords:** RDP sessions shown inside Termalith can save the
  password in the system keychain.
- **Saved passwords:** if the system keychain refuses to save a password, the
  connection stays open and the app says so; if it refuses to read one, the
  app asks for the password instead of failing.
- **Macros:** record keystrokes with their pauses and play them back.
- **Keyword colors:** an optional setting colors words such as error and
  warning in terminal output.
- **Four panes:** a tab can be split into a 2×2 grid.
- **Files follow the terminal:** the Files panel can follow the shell's
  current folder.
- **FTP and FTPS sessions:** browse, transfer, edit, rename and delete files
  in the same file browser. FTPS checks the server's certificate like RDP;
  plain FTP is not encrypted, and the session dialog says so.
- **Server monitor:** an optional bar under SSH terminals shows CPU, load,
  memory, disk and network for Linux servers.
- **Files-only sessions:** an SSH session can open straight as a file
  browser over SFTP, without a terminal. They show an SFTP badge.
- **Embedded RDP** uses the graphics pipeline (RDPEGFX) where the server
  offers it, and follows the tab's size: in place where the server
  supports resizing, otherwise by logging in again at the new size once
  you stop resizing (Windows reconnects to the same session).

### Included from the first preview

The first public build. Installers for macOS (Apple Silicon and Intel),
Windows (x64 and ARM64) and Linux (x64 and ARM64) are attached to the
release with a `SHA256SUMS.txt`; they are not code-signed with a developer
certificate yet (the Mac app is signed ad hoc). Before each release, every
installer is installed on its own system in CI and the installed app is
started.

**Terminals:** local shells in tabs and split panes with a choice of installed
shell, find, shortcuts, color schemes, copy-on-select and right-click paste
(optional), clickable links (Cmd/Ctrl+click), broadcast typing, snippets with
variables, session logs and "save output", a command palette.

**SSH:** password, key, ssh-agent and keyboard-interactive (one-time codes)
login; secrets in the OS keychain only after a successful login; strict host
keys; jump hosts; local, remote and SOCKS tunnels; X11 and
agent forwarding (opt-in); SOCKS5/HTTP proxy with optional login; startup
commands; automatic reconnect; an SSH key
manager.

**Files:** SFTP panel and file-browser tabs, uploads of files and whole
folders by drag and drop, transfer queue with retry, folders/rename/delete, a remote text editor with
conflict detection.

**Other connections:** RDP and VNC through the installed client (optionally
tunnelled; on Linux the RDP server certificate is checked by Termalith and
pinned for FreeRDP), serial consoles, Telnet (also through a jump host), network tools
(ping, traceroute, TCP check, port scan, DNS, subnet calculator,
Wake-on-LAN).

**Experimental:** RDP inside Termalith, in a tab on every system (Settings →
Show RDP sessions inside Termalith), built on IronRDP: certificate check,
NLA login, keyboard with the remote layout, mouse, text clipboard both ways.
The desktop keeps its size and is scaled to the tab (resizing the remote
desktop is not working yet).

**Sessions:** folder tree with empty folders, subfolders and drag-to-move,
favorites, tags, search, "open all"; import from `~/.ssh/config` and
MobaXterm; export/import without secrets.

**App:** keyboard navigation and screen-reader labels in the session list,
tab bar and Files panel; WCAG AA contrast in both themes; local crash
reports (never sent automatically) and a recovery screen; **Check for
updates** (manual, or at startup if enabled; links only); on macOS a guide
when the Local Network permission blocks connections.
