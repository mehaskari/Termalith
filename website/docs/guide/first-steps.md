---
description: Make your first connection with Termalith — save an SSH server, confirm its host key, log in, save the password, open files and tunnels.
---

# Your first connection

This walk-through saves an SSH server, connects to it and opens its files.
It takes about two minutes.

## 1. The window

![The main window](../assets/screenshots/main-window.png)

- **Far left, the rail:** **Home**, **Connect** (quick connect, without
  saving), **Local** (a terminal on this computer; right-click to pick a
  shell), **Serial**, **Tools** (network tools and file servers), **Keys**
  (SSH key manager) and **Import** (import and export sessions); at the
  bottom **Settings** and **About** (update check, report a problem).
- **Left:** your saved sessions as a folder tree, with search.
- **Right:** the **Home** screen when nothing is open, else your tabs. Every
  connection opens in a tab. Above a terminal is its toolbar: split,
  broadcast, log, macro and the side panels (snippets and, for SSH tabs,
  **Tunnels** and **Files**).

A small light shows the state of every connection, on its tab and next to
its session: green connected, amber connecting (blinking), red dropped.

On **Home**, type a session name, a tag or `user@host` and press
<kbd>Enter</kbd> to connect; below are buttons for the common first steps
and your recent and favorite sessions.

![The Home screen: connect by typing, first steps, recent and favorite sessions](../assets/screenshots/home.png)

## 2. Save a server

Click **+** next to *Sessions*.

![The New session dialog](../assets/screenshots/session-dialog.png){ width="440" }

1. **Type:** *SSH* (the other types are RDP, VNC, Telnet, FTPS, FTP and
   S3).
2. **Name:** anything you like, e.g. `web-01`.
3. **Host** and **Port:** the server's name or IP address; the port is 22
   for SSH unless your server uses another one.
4. **Username:** your account on the server.
5. **Authentication:** *Password*, *Private key file* (enter its full path, e.g.
   `/Users/you/.ssh/id_ed25519`) or *SSH agent*. See [SSH → Logging in](ssh.md#logging-in).
6. Optional: a **Folder** (`Production/Web` makes nested folders) and
   **Tags** (`prod, web`).
7. **Save.** The session appears in the list.

Everything else is optional and folded away under **Connection options**
(jump host, files only, Mosh, run after login, forwarding, old algorithms)
and **Look** (icon, colors, font); see [SSH](ssh.md) and
[Sessions](sessions.md).

## 3. Connect and confirm the host key

Click the session. The first time you connect to a server, Termalith shows
its **host key fingerprint**:

![Unknown host key dialog](../assets/screenshots/host-key.png){ width="440" }

A fingerprint is the server's identity. If you can, compare it with the one
your administrator gives you (`ssh-keygen -lf /etc/ssh/ssh_host_ed25519_key.pub`
on the server). Then choose:

- **Trust and connect:** remembered; you will not be asked again.
- **Connect once:** connects now and asks again next time.
- **Don't connect.**

If you already used the server with OpenSSH, Termalith trusts the keys in
your `~/.ssh/known_hosts` and does not ask.

!!! warning "If Termalith says the host key changed"
    It refuses to connect. This happens when a server is reinstalled, but it
    can also mean someone is intercepting the connection. Check with the
    server's administrator before you use **Remove recorded key…**.

## 4. Log in

For a password session, Termalith asks for the password:

![Password dialog](../assets/screenshots/password.png){ width="440" }

Tick **Save in the system keychain** to log in without typing it next time.
It is saved only after the login worked, in your system's keychain (macOS
Keychain, Windows Credential Manager or the Secret Service on Linux), never
in Termalith's own files. Servers that ask for a one-time code from an
authenticator app show that question in the same way.

You now have a terminal on the server.

## 5. Open files and tunnels

- **Files** on the toolbar opens the server's files next to the terminal.
  Drop files or folders onto the window to upload; click **↓** to download
  to your Downloads folder. See [Files](files.md).
- **Tunnels** forwards ports through the connection, like `ssh -L`, `-R`
  and `-D`. See [SSH → Tunnels](ssh.md#tunnels).

## Next steps

- Bring in your existing sessions:
  [Import from MobaXterm or `~/.ssh/config`](sessions.md#import-and-export).
- Organize them with folders, favorites and tags: [Sessions](sessions.md).
- Work in several servers at once: [Terminals](terminals.md).
- Protect saved passwords with a master password:
  [Passwords and your data](passwords-and-data.md#master-password).
