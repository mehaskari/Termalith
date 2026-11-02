# Changelog

## Unreleased

- Proxy login: SOCKS5 user name/password (RFC 1929) and HTTP Basic
  authentication, with the password kept in the system keychain.

## 0.5.0 — first public preview

The first public build. Installers for macOS (Apple Silicon and Intel),
Windows (x64 and ARM64) and Linux (x64 and ARM64) are attached to the
release with a `SHA256SUMS.txt`; they are not code-signed yet. The ARM and
Intel Mac builds pass CI on native runners; Windows and Linux ARM64 also had
a first hands-on run.

**Terminals:** local shells in tabs and split panes with a choice of installed
shell, find, shortcuts, color schemes, copy-on-select and right-click paste
(optional), clickable links (Cmd/Ctrl+click), broadcast typing, snippets with
variables, session logs and "save output", a command palette.

**SSH:** password, key, ssh-agent and keyboard-interactive (one-time codes)
login; secrets in the OS keychain only after a successful login; strict host
keys; jump hosts; SOCKS5/HTTP proxy; local, remote and SOCKS tunnels; X11 and
agent forwarding (opt-in); startup commands; automatic reconnect; an SSH key
manager.

**Files:** SFTP panel and file-browser tabs, uploads of files and whole
folders by drag and drop, transfer queue with retry, folders/rename/delete, a remote text editor with
conflict detection.

**Other connections:** RDP and VNC through the installed client (optionally
tunnelled; on Linux the RDP server certificate is checked by Termalith and
pinned for FreeRDP), serial consoles, Telnet (also through a jump host), network tools
(ping, traceroute, TCP check, port scan, DNS, subnet calculator,
Wake-on-LAN).

**Sessions:** folder tree with empty folders, subfolders and drag-to-move,
favorites, tags, search, "open all"; import from `~/.ssh/config` and
MobaXterm; export/import without secrets.

**App:** keyboard navigation and screen-reader labels in the session list,
tab bar and Files panel; WCAG AA contrast in both themes; local crash
reports (never sent automatically) and a recovery screen; **Check for
updates** (manual, or at startup if enabled; links only); on macOS a guide
when the Local Network permission blocks connections.
