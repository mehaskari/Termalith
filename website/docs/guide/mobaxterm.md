---
description: Switching from MobaXterm to Termalith — import your sessions, where each MobaXterm feature lives in Termalith, and what is different or missing.
---

# Coming from MobaXterm

Termalith covers the daily work many people use MobaXterm for (SSH with
files, remote desktops, tunnels, serial and Telnet) and runs on macOS and
Linux as well as Windows. It is an independent project, not affiliated with
MobaXterm or Mobatek.

## Move your sessions

1. In MobaXterm, right-click **User sessions** and export all sessions to a file (the
   menu item is named like *Export all sessions to file*), then save the `.mxtsessions` file.
2. In Termalith, **Import → Import sessions (JSON, MobaXterm)…**, then paste
   the file's contents.
3. SSH, RDP, VNC and Telnet sessions arrive with their folders. Other kinds
   (FTP, serial, Mosh…) are listed as not imported; recreate them by hand.
   Passwords are never in the export, so you are asked for each one at the
   first login (tick *Save in the system keychain*).

## Where to find things

| In MobaXterm | In Termalith |
|---|---|
| Sessions sidebar, folders | Sessions list with folders, favorites, tags and search ([Sessions](sessions.md)) |
| SSH session with the SFTP browser | SSH tab → **Files** ([Files](files.md)) |
| "Follow terminal folder" | **⇣** in the Files panel |
| SFTP / FTP session | SSH session with *Files only*, or an FTPS/FTP session |
| RDP, VNC sessions | RDP/VNC sessions, inside a tab or in your client ([Remote desktop](remote-desktop.md)) |
| MobaSSHTunnel | **Tunnels** on an SSH tab ([Tunnels](ssh.md#tunnels)) |
| Multi-execution | **Broadcast…** ([Terminals](terminals.md#broadcast-typing)) |
| Macros | **Macro** ([Terminals](terminals.md#macros)) |
| Split terminals | Split side by side, top and bottom, or four panes |
| Remote monitoring bar | *Server monitor* setting ([SSH](ssh.md#server-monitor)) |
| Serial, Telnet, Mosh | Serial button, Telnet sessions, *Connect with Mosh* |
| Network tools (ping, port scan, WoL…) | **Tools** ([Network tools](serial-telnet-tools.md#network-tools)) |
| Master password | **Settings → Master password** |
| Portable edition | `TermalithData` folder next to the app ([Portable copy](passwords-and-data.md#portable-copy)) |

## What is different

- **Same app on three systems:** macOS (Apple Silicon and Intel), Windows
  and Linux, x64 and ARM.
- **Free for any use** under the MIT license, with no paid edition and no
  session limits.
- **Stricter by default:** changed host keys are refused, RDP and FTPS
  certificates are pinned, passwords go only to the system keychain.

## Not available (yet)

- A built-in X server on Windows: use VcXsrv or Xming with *Forward X11*.
- Built-in Unix tools on Windows (use WSL or Git Bash as the local shell).
- A built-in NFS server (TFTP, HTTP and FTP are in
  [Tools → File servers](serial-telnet-tools.md#file-servers-tftp-http-ftp)),
  and S3, RSH or XDMCP sessions.
- Sound and drive sharing in embedded RDP.

These are on the [roadmap](https://github.com/mehaskari/Termalith/issues);
tell us which matters most to you.
