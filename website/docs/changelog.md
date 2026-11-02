# Changelog

## Unreleased

- Upload whole folders by dropping them on the Files panel (subfolders
  included). Previously a dropped folder was silently ignored. The panel is
  now outlined while files are dragged over the window.
- RDP on Linux: the server certificate is checked by Termalith and shown in
  a trust dialog (with the old and new fingerprints when it changes);
  FreeRDP then accepts only that certificate instead of trusting any
  certificate on first use.
- Installers for Windows on ARM, Linux on ARM64 and Intel Macs (built and
  tested in CI; not yet tried on real hardware).
- Folders are now first-class: create empty folders and subfolders, rename,
  delete, and drag sessions or folders to move them.
- A refreshed look: protocol badges (SSH, RDP, VNC, Telnet) in the session
  tree, drawn icons instead of emoji, a tidier sidebar footer and quieter
  toolbars.
- Crash reports: if Termalith closes unexpectedly, the next start shows the
  saved report with options to copy it or report it on GitHub. Nothing is
  sent automatically, and reports contain no passwords or terminal contents.
- A window error now shows a recovery screen with **Reload** instead of a
  blank window.

## 0.1.0 — first public preview

The first public build. Installers for macOS (Apple Silicon), Windows and
Linux are attached to the release; they are not code-signed yet.

**Terminals:** local shells in tabs and split panes with a choice of installed
shell, find, shortcuts, color schemes, copy-on-select and right-click paste
(optional), clickable links (Cmd/Ctrl+click), broadcast typing, snippets with
variables, session logs and "save output", a command palette.

**SSH:** password, key, ssh-agent and keyboard-interactive (one-time codes)
login; secrets in the OS keychain only after a successful login; strict host
keys; jump hosts; SOCKS5/HTTP proxy; local, remote and SOCKS tunnels; X11 and
agent forwarding (opt-in); startup commands; automatic reconnect; an SSH key
manager.

**Files:** SFTP panel and file-browser tabs, uploads by drag and drop,
transfer queue with retry, folders/rename/delete, a remote text editor with
conflict detection.

**Other connections:** RDP and VNC through the installed client (optionally
tunnelled), serial consoles, Telnet (also through a jump host), network tools
(ping, traceroute, TCP check, port scan, DNS, subnet calculator,
Wake-on-LAN).

**Sessions:** collapsible folder tree, favorites, tags, search, "open all";
import from `~/.ssh/config` and MobaXterm; export/import without secrets.
