# Termalith

**One workspace. Every connection.**

[![Latest release](https://img.shields.io/github/v/release/mehaskari/Termalith?label=release)](https://github.com/mehaskari/Termalith/releases/latest)
[![License: MIT](https://img.shields.io/badge/license-MIT-teal)](LICENSE)
[![Platforms](https://img.shields.io/badge/platforms-macOS%20%7C%20Windows%20%7C%20Linux-blue)](https://github.com/mehaskari/Termalith/releases/latest)
[![CI](https://github.com/mehaskari/Termalith/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/mehaskari/Termalith/actions/workflows/ci.yml)

**[Download](https://github.com/mehaskari/Termalith/releases/latest)** · **[Website](https://mehaskari.github.io/Termalith/)** · **[User guide](https://mehaskari.github.io/Termalith/guide/)**

Termalith is a free, open-source, cross-platform remote administration workstation for system and network administrators: SSH and Mosh terminals, SFTP/FTP/FTPS, remote desktop (RDP and VNC), tunnels, serial and Telnet consoles and network tools in one app. It is built with Tauri 2, Rust, React and TypeScript.

## Why Termalith

- **Free for any use.** MIT licensed: no paid edition, no account, no session limits, at home or at work.
- **The same on every system.** macOS (Apple Silicon and Intel), Windows (x64 and ARM64) and Linux (x64 and ARM64), one feature set, every build tested in CI.
- **Careful with your keys.** Passwords in the system keychain (optionally sealed with a master password), strict host keys, pinned RDP and FTPS certificates, no telemetry.
- **Easy to switch to.** Imports MobaXterm `.mxtsessions` exports and `~/.ssh/config`.

> **Status: 0.5.0, the first release.** Most features are in daily use on macOS. Windows and Linux builds pass CI on every change (unit and integration tests, real-server tests against OpenSSH, xrdp, Xvnc, FTP and Mosh, an installer and launch test) and have had less hands-on use. Please report anything that does not work.

## Features

**Terminals**
- Local terminals in tabs and split panes (up to a 2×2 grid), with a choice of installed shell (zsh/bash/fish…; PowerShell, cmd, Git Bash and WSL on Windows).
- Find, copy/paste and zoom shortcuts, copy-on-select and right-click paste (optional), color schemes (Solarized, Nord, Gruvbox, Tomorrow Night).
- Broadcast typing to several terminals, with a visible banner and explicit targets.
- Command snippets with `{{variables}}` and a preview of exactly what will be typed.
- Session logs (plain text, opt-in) and "save output" of a tab's scrollback.
- Command palette (Cmd+K / Ctrl+Shift+K) over tabs, sessions and actions.
- Links in terminal output open in your browser with Cmd/Ctrl+click (http and https only).
- Macros: record keystrokes with their pauses and play them back.
- Optional keyword colors (error, warning, ok) in terminal output, and per-session font and colors.

**SSH**
- Password, private key, ssh-agent and keyboard-interactive (PAM, one-time codes) authentication; secrets are stored only in the OS keychain, and only after a successful login.
- Strict host-key verification: unknown keys are shown for confirmation, changed keys are refused.
- Jump hosts (ProxyJump), SOCKS5/HTTP proxy for the first hop (with user name and password), automatic reconnect when a connection drops.
- Tunnels: local (-L), remote (-R) and dynamic SOCKS (-D), loopback-only unless you allow other machines.
- X11 forwarding (with cookie substitution, like OpenSSH) and agent forwarding, both opt-in per session.
- Startup command per session; SSH key manager (generate Ed25519, install a public key on a server).
- Mosh sessions through the installed `mosh` client.
- Optional server monitor under SSH terminals (CPU, load, memory, disk, network; Linux servers).

**Files**
- SFTP panel next to a terminal, or a file-browser tab without a shell (files-only sessions).
- FTP and FTPS sessions in the same file browser; FTPS accepts only the server certificate you confirmed, plain FTP is marked unencrypted.
- The Files panel can follow the terminal's current folder.
- Upload files and whole folders by drag and drop, downloads, folders, rename, delete (no recursive delete), transfer queue with retry.
- Edit a remote text file in place; saving refuses to overwrite changes made on the server meanwhile.
- Compare two remote text files side by side; transfer history (last 500, metadata only).

**Other connections**
- RDP and VNC inside a Termalith tab (IronRDP and vnc-rs; resizes with the tab, sharp on Retina screens), or through the installed client (Windows App/Microsoft Remote Desktop and Screen Sharing on macOS, Remote Desktop Connection on Windows, FreeRDP/TigerVNC on Linux), optionally through an SSH tunnel. On Linux, Termalith checks the RDP server certificate like an SSH host key and FreeRDP accepts only that one.
- Serial consoles (COM/tty) and Telnet (marked as unencrypted; can go through an SSH jump host).
- Network tools: ping, traceroute, TCP port check, port scan, DNS lookup, subnet calculator, Wake-on-LAN.

**Sessions**
- Folder tree with subfolders: create, rename, delete and move sessions and folders (drag, or **Move to folder…** from the keyboard); favorites, tags and search; "open all" for a folder.
- Import from `~/.ssh/config` and from MobaXterm `.mxtsessions` exports; export/import as JSON (never containing secrets).

**App**
- Optional master password that protects saved passwords (Argon2id + XChaCha20-Poly1305), asked at startup.
- Portable mode: a `TermalithData` folder next to the app keeps all data there, for running from a USB stick.
- Keyboard navigation and screen-reader labels in the session list, tab bar and Files panel; WCAG AA contrast in both themes.
- Crash reports kept locally and shown on the next start (copy, or report on GitHub; never sent automatically); a recovery screen with Reload if the window hits an error.
- **Check for updates** in About (or at startup, if you turn it on): links to a newer release, never installs anything. No telemetry.

Not yet: code-signed installers, automatic updates, sound and file clipboard in embedded RDP, a built-in X server on Windows.

## Install

Installers are on the [Releases](https://github.com/mehaskari/Termalith/releases/latest) page for macOS (Apple Silicon and Intel DMG), Windows (x64 and ARM64, NSIS and MSI) and Linux (x64 and ARM64, AppImage and deb), with a `SHA256SUMS.txt` to verify them. They are **not code-signed yet**:

- macOS: drag Termalith to Applications and open it; macOS refuses the first time. Then choose **Open Anyway** in System Settings → Privacy & Security (on macOS 14 and older, right-click the app and choose **Open**).
- Windows: SmartScreen may warn; choose **More info → Run anyway**.
- macOS: if connections to machines on your network fail with "No route to host", allow Termalith under System Settings → Privacy & Security → Local Network (Termalith shows this too).

## Build from source

Requirements: Rust (the version in `rust-toolchain.toml`), Node.js 24 with Corepack, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS (on Linux: WebKitGTK 4.1 development packages).

```bash
corepack pnpm install
corepack pnpm dev                      # run the app
cargo test --workspace                 # Rust tests (some use a local sshd when available)
corepack pnpm test                     # frontend tests
corepack pnpm --filter @termalith/desktop tauri build   # installers for this OS
```

Full instructions, including the release process: [Building and releasing](https://mehaskari.github.io/Termalith/releasing/). A UI preview without the Rust backend: `corepack pnpm --filter @termalith/desktop dev`, then open `http://localhost:1420/mock.html`.

## Security

Termalith handles credentials and remote access, so it is deliberately conservative: no shell or arbitrary command execution from the UI, secrets only in the OS credential store (or sealed with your master password), strict host keys, loopback-only tunnels by default, no passwords on command lines, and opt-in for agent and X11 forwarding. If you find a security issue, please report it privately via GitHub's "Report a vulnerability" (Security tab) rather than in a public issue.

## License

[MIT](LICENSE). Third-party components and their licenses: [website/docs/third-party.md](website/docs/third-party.md) (regenerate with `scripts/third-party-licenses.sh`). "Termalith" is a working name; it has not been checked for trademarks.
