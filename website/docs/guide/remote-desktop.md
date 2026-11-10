---
description: RDP and VNC in Termalith — inside a tab (IronRDP and vnc-rs) or in your installed client, server certificates, SSH tunnels, clipboard and resizing.
---

# Remote desktop

RDP and VNC sessions open either **inside Termalith**, in a tab, or in the
**client installed on your computer**. Switch with [Settings](settings.md)
→ *Show RDP and VNC sessions inside Termalith*.

## Inside Termalith (experimental)

With the setting on, RDP and VNC desktops open in a tab on every system:

- The remote desktop follows the tab's size. Servers that can resize a
  running session do it in place; with others (including Windows Server)
  Termalith logs in again at the new size a moment after you stop resizing,
  and Windows returns you to the same session. Meanwhile the desktop is
  scaled to the tab. On Retina and other high-density screens the desktop is
  requested in device pixels, so text stays sharp.
- The pointer takes the remote computer's shapes (RDP), so the resize
  arrows on a window's edge and the text beam show where they apply. Drags
  keep going when the pointer leaves the tab.
- Keyboard input uses the remote computer's layout. **Ctrl+Alt+Del** is on
  the tab's toolbar.
- Copied text goes both ways: text copied on the remote computer lands on
  your clipboard, and your clipboard is offered to the remote computer when
  the desktop gets focus. For VNC, text is limited to Latin-1, as the
  protocol defines.
- The password can be saved in the system keychain after a successful
  login.
- VNC logins: no password, or a VNC password. Servers that offer only
  their own account login cannot be used inside Termalith yet: in RealVNC
  Server set *Options → Security → Authentication* to **VNC password**; on
  a Mac turn on *System Settings → General → Sharing → Screen Sharing → ⓘ
  → VNC viewers may control screen with password*.
- Not supported yet: files on the clipboard, sound and drive sharing; VNC
  desktops are scaled, not resized.

## In your own client

| System | RDP | VNC |
|---|---|---|
| macOS | Windows App / Microsoft Remote Desktop | Screen Sharing (built in) |
| Windows | Remote Desktop Connection (built in) | TigerVNC or RealVNC Viewer |
| Linux | FreeRDP (`xfreerdp`) | TigerVNC (`vncviewer`) |

The session dialog shows which client will be used. If none is installed,
the session opens inside Termalith instead, with a notice.
Passwords are never passed on a command line; the client asks for them
(FreeRDP on Linux receives it on standard input).

## Through an SSH server

Choose **Tunnel through (SSH session)** in the session dialog to reach a
desktop behind a bastion or firewall. Termalith opens the SSH connection
and forwards the desktop through it. Use this for VNC, which is not
encrypted on its own.

## Server certificates

RDP servers identify themselves with a certificate. When Termalith
connects (inside a tab, and for FreeRDP on Linux), it reads the certificate
first and shows its fingerprint in the same dialog as an unknown SSH host
key. After you accept it, only that certificate is accepted. If the server
later presents a different one (Windows renews its self-signed certificate
from time to time), you see both fingerprints and decide again. Servers that
offer only legacy RDP security without TLS are refused. On macOS and
Windows, the Microsoft clients check certificates themselves.
