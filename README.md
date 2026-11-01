# Termalith

**One workspace. Every connection.**

Termalith is an open-source, cross-platform remote administration workstation for system and network administrators: SSH terminals, SFTP, remote desktop, tunnels, serial and Telnet consoles and network tools in one app. It is built with Tauri 2, Rust, React and TypeScript.

> **Status: early preview (0.1).** Most features are in daily use on macOS. Windows and Linux builds pass CI (unit and integration tests, a launch smoke test) but have had little hands-on use. Expect rough edges, and please report them.

## Features

**Terminals**
- Local terminals in tabs and split panes, with a choice of installed shell (zsh/bash/fish…; PowerShell, cmd, Git Bash and WSL on Windows).
- Find, copy/paste and zoom shortcuts, copy-on-select and right-click paste (optional), color schemes (Solarized, Nord, Gruvbox, Tomorrow Night).
- Broadcast typing to several terminals, with a visible banner and explicit targets.
- Command snippets with `{{variables}}` and a preview of exactly what will be typed.
- Session logs (plain text, opt-in) and "save output" of a tab's scrollback.
- Command palette (Cmd+K / Ctrl+Shift+K) over tabs, sessions and actions.
- Links in terminal output open in your browser with Cmd/Ctrl+click (http and https only).

**SSH**
- Password, private key, ssh-agent and keyboard-interactive (PAM, one-time codes) authentication; secrets are stored only in the OS keychain, and only after a successful login.
- Strict host-key verification: unknown keys are shown for confirmation, changed keys are refused.
- Jump hosts (ProxyJump), SOCKS5/HTTP proxy for the first hop, automatic reconnect when a connection drops.
- Tunnels: local (-L), remote (-R) and dynamic SOCKS (-D), loopback-only unless you allow other machines.
- X11 forwarding (with cookie substitution, like OpenSSH) and agent forwarding, both opt-in per session.
- Startup command per session; SSH key manager (generate Ed25519, install a public key on a server).

**Files**
- SFTP panel next to a terminal, or a file-browser tab without a shell.
- Upload by drag and drop, downloads, folders, rename, delete (no recursive delete), transfer queue with retry.
- Edit a remote text file in place; saving refuses to overwrite changes made on the server meanwhile.

**Other connections**
- RDP and VNC through the installed client (Windows App/Microsoft Remote Desktop and Screen Sharing on macOS, Remote Desktop Connection on Windows, FreeRDP/TigerVNC on Linux), optionally through an SSH tunnel.
- Serial consoles (COM/tty) and Telnet (marked as unencrypted; can go through an SSH jump host).
- Network tools: ping, traceroute, TCP port check, port scan, DNS lookup, subnet calculator, Wake-on-LAN.

**Sessions**
- Collapsible folder tree with subfolders (`Production/DB`), favorites, tags and search; "open all" for a folder.
- Import from `~/.ssh/config` and from MobaXterm `.mxtsessions` exports; export/import as JSON (never containing secrets).

Not yet: embedded RDP/VNC (external clients are used), code-signed installers.

## Install

Pre-release installers are built by GitHub Actions for macOS (Apple Silicon DMG), Windows (NSIS and MSI) and Linux (AppImage and deb). They are **not code-signed yet**:

- macOS: right-click the app and choose **Open** the first time.
- Windows: SmartScreen may warn; choose **More info → Run anyway**.

## Build from source

Requirements: Rust (the version in `rust-toolchain.toml`), Node.js 24 with Corepack, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/) for your OS (on Linux: WebKitGTK 4.1 development packages).

```bash
corepack pnpm install
corepack pnpm dev                      # run the app
cargo test --workspace                 # Rust tests (some use a local sshd when available)
corepack pnpm test                     # frontend tests
corepack pnpm --filter @termalith/desktop tauri build   # installers for this OS
```

A UI preview without the Rust backend: `corepack pnpm --filter @termalith/desktop dev`, then open `http://localhost:1420/mock.html`.

## Security

Termalith handles credentials and remote access, so it is deliberately conservative: no shell or arbitrary command execution from the UI, secrets only in the OS credential store, strict host keys, loopback-only tunnels by default, no passwords on command lines, and opt-in for agent and X11 forwarding. If you find a security issue, please report it privately via GitHub's "Report a vulnerability" (Security tab) rather than in a public issue.

## License

[MIT](LICENSE). "Termalith" is a working name; it has not been checked for trademarks.
