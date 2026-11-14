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

## Open from a shortcut or a script

Start Termalith with a switch to open sessions right away (instead of the
first local terminal):

| Switch | Opens |
|---|---|
| `--session "web-01"` | the saved session with that name |
| `--folder "Production/DB"` | every session in that folder and its subfolders |
| `ssh://me@10.0.0.5:2222` (also `telnet://`, `rdp://`, `vnc://`, `sftp://`, `ftp://`) | that connection, like the command palette |
| `--connect "ssh -p 2222 me@host"` | a typed connection |

Examples: on Windows, copy the Termalith shortcut, open **Properties** and
add `--session "web-01"` after the path in **Target**; on macOS
`open -a Termalith --args --folder Production`; on Linux
`termalith --session web-01`. Switches apply when Termalith starts; a
running Termalith is not reused.

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

## Icons

Give sessions and folders an icon so servers, switches, firewalls and the
rest are easy to tell apart in the list and on tabs.

- **Session:** in the session dialog, **Icon** offers about 190 pictures
  in groups (servers, computers, network, security, devices, places, files
  and apps, tools, shapes) with a search box, e.g. *router*, *database*,
  *firewall* (*brick wall*), *cloud*, *printer*, plus 13 colors.
  **By type** (the default) shows the protocol badge (SSH, RDP…) instead.
- **Folder:** right-click a folder → **Icon…**, pick a picture and a color,
  **Save**. **Folder** goes back to the plain folder picture. A folder icon
  follows the folder when it is renamed or moved.
- Tabs of a session show its icon. Session icons are included in exported
  session files; folder icons are not.

## Favorites, tags and search

- **☆** pins a session to *Favorites* at the top.
- **Tags** (comma-separated in the session dialog) appear under the session.
  Click a tag, or type `#tag` in the search box, to show only those
  sessions.
- The search box matches names, hosts, user names, folders and tags; with
  several words, all must match.

## Shared session files (team)

A team can keep one session list for everybody: export it (**Import →
Export sessions…**) to a shared folder or a web server, then on each
computer open **Settings → Shared sessions**, type a folder name (e.g.
`Team`) and click **Add file…** (pick the `.json` file) or enter an
`https://` address and click **Add address**.

- Its sessions appear under that folder, marked *shared*. They are
  read-only: **Duplicate as my session** makes your own editable copy.
- Termalith reads the file again at every start (and on **Refresh now**).
  Changed sessions are updated, removed ones disappear; a file that cannot
  be read keeps the last list and Settings shows why.
- Export files never contain passwords. A password you save for a shared
  session stays on your computer and survives refreshes; favorites too.
- Only `https://` addresses are accepted, so the list cannot be changed on
  the way. **Remove** drops the source, its sessions and their saved
  passwords.

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
