---
description: Download and install Termalith on macOS (Apple Silicon or Intel), Windows (x64 or ARM64) or Linux (x64 or ARM64), verify the download, and get past the first-launch warning.
---

# Download and install

Termalith is free. Every version is on the
**[Releases page](https://github.com/mehaskari/Termalith/releases/latest)**
(the link always opens the newest one). Scroll to **Assets** and pick the
file for your system from the table below.

## Which file do I need?

| Your computer | Download | How to check |
|---|---|---|
| **Mac with Apple Silicon** (M1, M2, M3, M4…) | `Termalith_<version>_aarch64.dmg` | Apple menu → About This Mac shows *Chip: Apple M…* |
| **Mac with Intel** | `Termalith_<version>_x64.dmg` | About This Mac shows *Processor: Intel…* |
| **Windows 10 or 11** (most PCs) | `Termalith_<version>_x64-setup.exe` | Settings → System → About: *x64-based processor* |
| **Windows on ARM** (Surface Pro X, Snapdragon laptops) | `Termalith_<version>_arm64-setup.exe` | *ARM-based processor* |
| **Linux x64**, Ubuntu / Debian / Mint | `Termalith_<version>_amd64.deb` | `uname -m` prints `x86_64` |
| **Linux x64**, any other distribution | `Termalith_<version>_amd64.AppImage` | as above |
| **Linux ARM64** (Raspberry Pi 5, ARM servers with a desktop) | `Termalith_<version>_arm64.deb` or `…_aarch64.AppImage` | `uname -m` prints `aarch64` |

Windows also has `.msi` files (`…_x64_en-US.msi`, `…_arm64_en-US.msi`) for
administrators who deploy software with Group Policy or Intune; the
`-setup.exe` is the simpler choice for everyone else.

**Requirements:** macOS 11 (Big Sur) or newer; Windows 10 version 1809 or
newer (the WebView2 runtime is included in Windows 11 and installed by the
setup if missing); Linux with glibc 2.35 or newer and WebKitGTK 4.1 (Ubuntu
22.04, Debian 12, Fedora 36 or newer). For saved passwords on Linux you need
a Secret Service such as GNOME Keyring or KWallet (most desktops have one).

## Install

=== "macOS"

    1. Open the downloaded `.dmg` and drag **Termalith** onto
       **Applications**.
    2. Open Termalith from Applications. macOS says it cannot verify the
       developer, because the app is not signed with a paid Apple
       certificate yet. Click **Done** (or **Cancel**).
    3. Open **System Settings → Privacy & Security**, scroll down and click
       **Open Anyway** next to the message about Termalith, then confirm.
       (macOS 14 and older: right-click the app in Applications and choose
       **Open**.)
    4. The first time Termalith connects to a machine on your own network,
       macOS asks whether it may find devices on your local network: click
       **Allow**. If you missed it, see
       [Troubleshooting](troubleshooting.md#no-route-to-host-on-macos).

=== "Windows"

    1. Run `Termalith_<version>_x64-setup.exe` (or the ARM64 one).
    2. If SmartScreen shows *Windows protected your PC*, click **More info →
       Run anyway**. This appears because the installer is not signed with a
       paid certificate yet.
    3. Follow the setup. Termalith appears in the Start menu.

=== "Linux (.deb)"

    ```bash
    sudo apt install ./Termalith_<version>_amd64.deb
    ```

    Termalith appears in your applications menu. Remove it with
    `sudo apt remove termalith`.

=== "Linux (AppImage)"

    ```bash
    chmod +x Termalith_<version>_amd64.AppImage
    ./Termalith_<version>_amd64.AppImage
    ```

    Nothing is installed; keep the file wherever you like. To carry it on a
    USB stick with all your data, see
    [Portable copy](passwords-and-data.md#portable-copy).

## Verify the download (optional)

Each release has a `SHA256SUMS.txt`. Compare it with the checksum of your
file; they must be identical:

```bash
shasum -a 256 Termalith_<version>_aarch64.dmg        # macOS
sha256sum Termalith_<version>_amd64.AppImage         # Linux
```

```powershell
Get-FileHash .\Termalith_<version>_x64-setup.exe     # Windows (PowerShell)
```

Every installer is installed and started on its own system in an automated
test before a release is published.

## Updates

Termalith never updates itself or downloads anything on its own. Use
**About (ⓘ in the sidebar footer) → Check for updates**, or turn on *Check
GitHub for a newer version at startup* in [Settings](settings.md). Install a
newer version over the old one; your sessions and settings are kept.

## Uninstall

- **macOS:** drag Termalith from Applications to the Trash.
- **Windows:** Settings → Apps → Installed apps → Termalith → Uninstall.
- **Linux:** `sudo apt remove termalith`, or delete the AppImage.

Your sessions and settings stay on the computer until you delete the data
folder listed in [Where your data is](passwords-and-data.md#where-your-data-is);
saved passwords stay in the system keychain under entries named
*Termalith*.

## Build from source

You need Git, Rust (the version in `rust-toolchain.toml` is installed
automatically through `rustup`), Node.js 24 with Corepack, and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) for your
system.

```bash
git clone https://github.com/mehaskari/Termalith.git && cd Termalith
corepack pnpm install --frozen-lockfile
corepack pnpm dev                                        # run it
corepack pnpm --filter @termalith/desktop tauri build   # build installers for this system
```

More detail: [Building and releasing](../releasing.md).
