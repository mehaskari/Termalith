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

## Documentation

- `MASTER_SPEC.md` — Product requirements and functional specification.
- `ARCHITECTURE.md` — Proposed technical architecture.
- `ROADMAP.md` — Development phases.
- `TASKS.md` — Initial implementation backlog.
- `SECURITY.md` — Security requirements and threat considerations.
- `DECISIONS.md` — Architecture decision records.
- `AGENTS.md` — Instructions for AI coding agents.
- `MACOS_DEVELOPMENT.md` — macOS-first development setup and workflow.

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

See `docs/phase0/macos-terminal-spike.md` for scope, verification status, and findings.

## What works today (first usable version)

- Local terminals in tabs (macOS, Windows, Linux).
- Saved SSH sessions with folders and search; quick connect.
- Password, private-key, passphrase and ssh-agent authentication; optional saving to the OS keychain (only after a successful login).
- Jump hosts (ProxyJump) through another saved session; each hop's host key is verified.
- Import sessions from `~/.ssh/config` (supported subset, including single-hop ProxyJump; unsupported options are listed per entry).
- Host-key verification: dialog for unknown keys; changed keys are refused with an explicit warning and a deliberate "remove recorded key" step.
- SFTP side panel per SSH tab: browse, download to the Downloads folder, upload by dropping files on the window, progress and cancel.

Not yet: split panes, port forwarding/tunnels, serial, RDP/VNC, tags/favorites, transfer retry. See `TASKS.md`.

UI preview without a backend (development only): run `corepack pnpm --filter @termalith/desktop dev` and open `http://localhost:1420/mock.html`.
