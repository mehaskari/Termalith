---
description: Save and organize connections in Termalith — session types, folders, favorites, tags, search, Quick connect, and import from MobaXterm, OpenSSH config and JSON.
---

# Sessions

A session is a saved connection: its type, host, port, user name and
options. Passwords are never part of a session; they live in the system
keychain (see [Passwords and your data](passwords-and-data.md)).

## Session types

| Type | Opens | Notes |
|---|---|---|
| **SSH terminal** | A terminal tab | Can be *Files only* (opens the file browser instead) or use [Mosh](ssh.md#mosh). |
| **RDP remote desktop** | A desktop tab or your RDP client | See [Remote desktop](remote-desktop.md). |
| **VNC remote desktop** | A desktop tab or your VNC client | Unencrypted unless tunnelled through SSH. |
| **Telnet (unencrypted)** | A terminal tab | For devices without SSH; can go through a jump host. |
| **FTPS files (encrypted)** | A file browser tab | Explicit TLS; certificate checked once and pinned. |
| **FTP files (unencrypted)** | A file browser tab | Prefer SFTP or FTPS. |

Serial consoles and local terminals are opened from the buttons at the top
of the sidebar and are not saved as sessions.

## Create, edit, connect

- **+** next to *Sessions* creates a session; hovering a row shows ☆
  (favorite), ⇅ (browse files, for SSH), ✎ (edit) and ⋯ (more: duplicate,
  move, delete).
- Click a session, or select it and press <kbd>Enter</kbd>, to connect.
- **Quick connect** connects without saving. The host field accepts
  `user@host:port` or an ssh command such as `ssh -p 2222 -l root web1`
  or `ssh -i ~/.ssh/id_ed25519 me@web1`.
- **Type a connection in the command palette:** press
  <kbd>Cmd</kbd>/<kbd>Ctrl</kbd>+<kbd>K</kbd> and type e.g.
  `ssh me@10.0.0.5`, `telnet 10.0.0.254 23`, `mosh me@box`,
  `sftp me@files`, `rdp admin@win`, `vnc 192.168.1.40` or a URL such as
  `ssh://me@host:2222`. **Connect** opens SSH at once (not saved); the
  other types open **New session** filled in, ready to save.

## Folders

- Create one with the folder button next to *Sessions*, or right-click the
  list.
- Right-click a folder (or its **⋯**) for **New session here**, **New
  subfolder**, **Rename**, **Open all** and **Delete folder** (only when it
  holds no sessions).
- Drag a session or a whole folder onto another folder to move it, or onto
  empty space to move it to the top level. **Move to folder…** in the menu
  does the same without a mouse.
- In the session dialog, the **Folder** field suggests existing folders;
  `/` makes subfolders, e.g. `Production/DB`.

## Favorites, tags and search

- **☆** pins a session to *Favorites* at the top.
- **Tags** (comma-separated in the session dialog) appear under the session.
  Click a tag, or type `#tag` in the search box, to show only those
  sessions.
- The search box matches names, hosts, user names, folders and tags; with
  several words, all must match.

## Import and export

**Import** in the sidebar footer:

- **Import ~/.ssh/config…** reads your OpenSSH configuration and lets you
  choose hosts. Imported: `HostName`, `User`, `Port`, `IdentityFile`,
  single-hop `ProxyJump`, `ForwardAgent yes` and `ForwardX11 yes`. Anything
  else is listed per host and not imported.
- **Import sessions (JSON, MobaXterm)…** accepts a Termalith export or the
  contents of a MobaXterm `.mxtsessions` file (in MobaXterm: right-click
  *User sessions* and export all sessions to a file). SSH, RDP, VNC and Telnet
  sessions come with their folders; other kinds are listed as not imported.
- **Export sessions…** writes all sessions as JSON, to back them up or move
  them to another computer. Exports never contain passwords or keychain
  references.

Files up to 5 MiB can be imported.
