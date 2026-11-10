---
description: Fixes for common Termalith problems — first-launch warnings, macOS local network access, SSH agent, Linux keychain, host key changes, X11, RDP/VNC, serial permissions and crashes.
---

# Troubleshooting

## Installing and starting

**macOS says Termalith "cannot be opened" or "is damaged".** The app is not
signed with a paid Apple certificate yet. Open it once, then use **System
Settings → Privacy & Security → Open Anyway**. If macOS calls it damaged,
the download was probably incomplete: download it again and compare the
checksum ([Verify the download](install.md#verify-the-download-optional)).

**Windows SmartScreen blocks the installer.** Click **More info → Run
anyway**. The installer is not signed with a paid certificate yet.

**Linux: the AppImage does not start.** Make it executable
(`chmod +x Termalith_*.AppImage`). Termalith needs WebKitGTK 4.1 and glibc
2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 or newer).

## Connecting

### "No route to host" on macOS

…while `ssh` in Terminal works: macOS blocks apps from the local network
until you allow it. Termalith notices this and shows **Allow local network
access** with a button that opens the right page in System Settings
(Privacy & Security → Local Network). Turn Termalith on (off and on again if
it already is), then use **Restart Termalith**. Because unsigned versions
count as a new app, this can come back after an update. If it still fails,
restart the Mac once.

**"No SSH agent is available".** Start an agent and add a key
(`ssh-add`). On macOS the system agent is available to apps automatically;
on Linux, apps started from the desktop need `SSH_AUTH_SOCK` from your
session; on Windows, start the *OpenSSH Authentication Agent* service.

**Host key changed.** This can mean an attack or a reinstalled server.
Confirm the new fingerprint with the server's administrator before you use
**Remove recorded key…**.

**The jump host asks for a password and fails.** A jump host must log in
without questions: save its password, or use a key or the agent.

**FTP/FTPS refuses to connect.** FTP and FTPS do not go through a proxy;
choose *Direct connection* in Settings → Proxy. FTPS needs a server that
offers explicit TLS (`AUTH TLS`); implicit FTPS on port 990 is not
supported.

## Passwords

**Passwords are not saved on Linux.** Termalith needs a Secret Service
(GNOME Keyring, KWallet). Without one, passwords are asked each time and
never written to disk.

**I forgot my master password.** It cannot be recovered. Use **Forgot the
master password?** to delete the saved passwords and start over; your
sessions stay.

## Remote desktop and X11

**"No RDP client found" (or VNC).** Since 0.5.2 this no longer stops you:
when no client program is installed, the session opens in Termalith's
built-in viewer and a notice says so. To make the built-in viewer the
default, turn on *Show RDP and VNC sessions inside Termalith* in
[Settings](settings.md). To use an external client instead: on Windows,
Remote Desktop Connection (`mstsc.exe`) is looked for in
`%SystemRoot%\System32` and on the `PATH` (some Windows editions and images
remove it; the Windows folder is read from the registry, and the error
lists every place searched, which helps when you report it); on macOS install
*Windows App*; on Linux install FreeRDP
(`xfreerdp`), and for VNC TigerVNC.

**RDP/VNC does nothing.** With desktops in your own client, the session
dialog says which client is used; install it if it is missing. On macOS the
client keeps its own window, so Termalith cannot tell when you disconnect.
Or turn on *Show RDP and VNC sessions inside Termalith* in Settings.

**RDP: "only offers legacy RDP security".** The server did not offer TLS,
so its identity cannot be checked and Termalith refuses to connect. On
Windows, keep the default security layer (Negotiate or SSL/TLS). On xrdp,
make sure it can read its certificate (`sudo adduser xrdp ssl-cert`), set
`security_layer=tls` or `negotiate` in `/etc/xrdp/xrdp.ini` and restart it.

**X11 windows do not appear.** Install and start XQuartz (macOS; log out
and in once after installing) or VcXsrv/Xming (Windows), tick *Forward X11*
in the session, and make sure the server allows it (`X11Forwarding yes` in
`sshd_config`) and has `xauth` installed.

## Devices

**Serial port: permission denied (Linux).** Add yourself to the `dialout`
group (`sudo usermod -aG dialout $USER`) and log in again.

## Crashes

**Termalith closed unexpectedly.** The next start shows the saved crash
report (version, system, where the error happened; no passwords or terminal
contents). Nothing is sent automatically. Use **Copy report**, check it,
then **Report on GitHub** and paste it into the issue. Reports are kept in
the crash folder ([Where your data is](passwords-and-data.md#where-your-data-is),
at most 20) until you dismiss them. If the window itself hits an error, a
recovery screen offers **Reload**.

Still stuck? [Open an issue](https://github.com/mehaskari/Termalith/issues)
with your system, Termalith version (in About) and what you tried.
