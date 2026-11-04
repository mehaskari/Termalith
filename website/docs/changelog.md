# Changelog

## Unreleased

- **Server monitor:** an optional bar under SSH terminals shows CPU, load,
  memory, disk and network for Linux servers.
- **Files-only sessions:** an SSH session can open straight as a file
  browser over SFTP, without a terminal. They show an SFTP badge.
- **Embedded RDP** uses the graphics pipeline (RDPEGFX) where the server
  offers it, and follows the tab's size: in place where the server
  supports resizing, otherwise by logging in again at the new size once
  you stop resizing (Windows reconnects to the same session).

## 0.5.0 — first public preview

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
