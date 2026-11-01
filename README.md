# Termalith

**One Workspace. Every Connection.**

Termalith is an open-source, cross-platform remote administration workstation designed for infrastructure engineers, network administrators, and system administrators.

It aims to provide a unified workspace for managing remote systems, network devices, servers, and infrastructure services.

## Product identity

- Product name: Termalith
- Naming status: Selected working name; domain, trademark, social handle, GitHub organization, and package-registry availability are not yet confirmed.
- License: MIT (see `LICENSE`).
- UI language: English
- Target platforms: macOS, Windows, Linux
- Primary development host: macOS on Apple Silicon
- Architecture: Tauri 2 + Rust + React + TypeScript

## Product vision

Termalith is intended to be a comprehensive alternative to MobaXterm, with a modern interface and a cross-platform architecture.

It should combine terminal access, remote file management, remote desktop connectivity, connection organization, and network administration tools in one application.

## Core capabilities

- SSH terminal
- Local terminal
- SFTP file manager
- RDP remote desktop
- VNC remote desktop
- SSH tunnels and port forwarding
- Jump hosts and ProxyJump
- Serial console
- Session manager
- Credential management
- Network utilities
- Remote file editor
- Session logging
- Command snippets and macros
- Multi-tab and split-pane interface

## Naming notice

Termalith is the selected working name. It is not yet claimed or legally cleared.

Before public release, verify trademark, domain, GitHub organization, social handles, and package-registry availability. Do not claim any identifier is reserved until it has actually been registered.

## Development strategy

- Develop primarily on macOS (Apple Silicon).
- Use macOS as the first local development and acceptance target.
- Keep Windows and Linux as first-class product targets.
- Use native CI runners or dedicated test machines for Windows and Linux builds and integration tests.
- Keep OS-specific functionality behind platform adapters.
## Development (Phase 0 spike)

Prerequisites (macOS): Xcode Command Line Tools, Node.js >= 24, rustup (`brew install rustup`; it is keg-only, so add `/opt/homebrew/opt/rustup/bin` to `PATH`).

```bash
corepack pnpm install
corepack pnpm dev        # launches the Tauri app with a local terminal
cargo test --workspace
corepack pnpm test
```


## What works today (first usable version)

- Local terminals in tabs (macOS, Windows, Linux) with a choice of installed shell (from /etc/shells; PowerShell, cmd, Git Bash and WSL distributions on Windows) and a default-shell setting, split side by side or top/bottom; SSH splits reuse the same connection.
- Saved sessions with folders, favorites, tags and search (`#tag` filters by tag); quick connect.
- Password, private-key, passphrase and ssh-agent authentication; keyboard-interactive for PAM servers and second factors (one-time codes are asked in a dialog); optional saving to the OS keychain (only after a successful login).
- Jump hosts (ProxyJump) through another saved session; each hop's host key is verified.
- Automatic reconnect when an SSH connection drops (not after `exit`), with a visible countdown and Cancel; can be turned off in Settings.
- SSH keys: list the key pairs in ~/.ssh with fingerprints, generate Ed25519 keys (optionally passphrase-protected), copy a public key, and install it in authorized_keys on a connected server (folder 0700, file 0600, never duplicated).
- X11 forwarding per session (graphical programs on the server open windows here; needs XQuartz on macOS or VcXsrv/Xming on Windows). The server only gets a random fake cookie; the real one stays local. Imported from `ForwardX11 yes`.
- Optional SSH agent forwarding per session (off by default, never to jump hosts; imported from `ForwardAgent yes`).
- Import sessions from a MobaXterm `.mxtsessions` export (SSH, RDP and VNC sessions with their folders; other types are listed as not imported).
- Import sessions from `~/.ssh/config` (supported subset, including single-hop ProxyJump; unsupported options are listed per entry).
- Host-key verification: dialog for unknown keys; changed keys are refused with an explicit warning and a deliberate "remove recorded key" step.
- Tunnels per SSH tab: local forwards (-L), remote forwards (-R) and a SOCKS5 proxy (-D); loopback-only unless you explicitly allow other machines.
- Serial consoles (COM / tty): port discovery, baud, data bits, parity, stop bits, flow control.
- Command snippets with `{{variables}}`, a preview of exactly what will be typed, and an explicit "press Enter" choice.
- Network tools tab: ping, traceroute, TCP port check, DNS lookup, IPv4 subnet calculator.
- Find in terminal output; shortcuts for tabs, copy/paste and font zoom (Cmd on macOS, Ctrl+Shift elsewhere); optional copy-on-select and right-click paste.
- Broadcast input to several terminals: explicit target selection, red banner on the source, markers on targets, disconnected targets skipped and listed.
- RDP and VNC sessions opened in the installed client (Windows App / Microsoft Remote Desktop / Screen Sharing on macOS, Remote Desktop Connection on Windows, FreeRDP / TigerVNC on Linux), optionally through an SSH tunnel; passwords are never passed on the command line.
- Session logs: save a terminal's output as plain text (escape sequences removed) to Documents/Termalith Logs, per tab or automatically for every terminal; files are readable only by you.
- Remote file editor: open a text file (UTF-8, up to 2 MiB) from the Files panel in an editor tab and save it back with Cmd/Ctrl+S; permissions are kept, the original is only replaced once the new content is fully written, and a save is refused if the file changed on the server meanwhile (reload or overwrite).
- SFTP side panel per SSH tab: browse, create folders, rename and delete (files and empty folders; no recursive delete), download to the Downloads folder, upload by dropping files on the window, progress and cancel; up to 3 transfers at once (others queue), automatic retry of connection errors and a manual Retry button.

Not yet: embedded RDP/VNC (external clients only).

UI preview without a backend (development only): run `corepack pnpm --filter @termalith/desktop dev` and open `http://localhost:1420/mock.html`.
