# Security

Termalith is a tool for remote access, so it is built to be conservative by
default. This page explains what it does with your credentials and
connections, and how to report a problem.

## Reporting a vulnerability

Please report security issues **privately** through GitHub:
[Report a vulnerability](https://github.com/mehaskari/Termalith/security/advisories/new)
(Security tab of the repository). Do not open a public issue for them.

## Credentials

- Passwords and key passphrases are stored **only in the operating system's
  credential store** (macOS Keychain, Windows Credential Manager, Secret
  Service on Linux), and only after a login with them succeeded and you
  asked to save them.
- The session database holds only an opaque reference to a stored secret.
  Exports never contain secrets or references.
- Secrets are never written to logs, and are not passed to other programs
  on a command line.
- Without a working credential store, passwords are asked each time and
  never persisted.

## Host keys

- Unknown host keys are shown with their fingerprint for confirmation.
- A **changed** key is refused; there is no "connect anyway". Removing the
  recorded key is a separate, deliberate step.
- SHA-1-based `ssh-rsa` host key signatures are not accepted.
- Every hop of a jump-host chain is verified.

## Connections

- Tunnels listen on the loopback address unless you explicitly allow other
  machines.
- Agent forwarding and X11 forwarding are off unless enabled per session,
  and never apply to jump hosts. X11 forwarding gives the server a random
  fake cookie; the real one stays on your computer.
- Channels the server opens without being asked (sessions, X11, agent,
  forwarded ports) are refused unless the matching feature is enabled.
- Telnet is marked as unencrypted wherever it appears; through a jump host
  only the last leg is unencrypted.
- Links in terminal output open only with a modifier click, and only
  `http`/`https` links are accepted.
- RDP on Linux: Termalith reads the server certificate and asks you, like
  an SSH host key; FreeRDP then accepts only that certificate. Servers
  without TLS are refused.

## What Termalith sends on its own

Nothing, unless you ask. There is no telemetry and no analytics. Crash
reports stay on your computer until you copy or file one. **Check for
updates** (in About, or at startup if you turn it on in Settings) asks
GitHub's public releases list for the newest version and only shows a link;
it downloads and installs nothing.

The **server monitor** is off by default. When you turn it on in Settings,
the active SSH tab runs one fixed, read-only command on that server every
3 seconds. It reads `/proc` and runs `df`, and the values stay in the app.

## The app itself

- The user interface cannot run arbitrary commands: every operation is a
  typed, permission-listed command in the Rust backend. Local terminals
  start only installed shells, network tools run fixed programs with
  validated arguments.
- Remote file names are validated before anything is written locally;
  downloads never overwrite existing files.
- Dependencies are checked for known vulnerabilities and license problems
  in CI (`cargo-deny`), and GitHub's code scanning and Dependabot alerts are
  enabled for the repository.

## Current limitations

- Installers are not code-signed yet.
- RDP/VNC run in external clients; their security depends on those
  clients.
- HTTP proxies receive their password with Basic authentication, which is
  readable on the way to the proxy; SOCKS5 user/password is likewise
  unencrypted. Use a proxy on a network you trust.
