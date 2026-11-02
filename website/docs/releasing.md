# Building and releasing

## Verify a download

Every release has a `SHA256SUMS.txt` file next to the installers. Compare
the checksum of the file you downloaded with the line for its name:

```bash
shasum -a 256 Termalith_0.5.0_aarch64.dmg        # macOS
sha256sum Termalith_0.5.0_amd64.AppImage         # Linux
```

```powershell
Get-FileHash .\Termalith_0.5.0_x64-setup.exe     # Windows (PowerShell)
```

Installers are not code-signed yet, so the checksum is the way to confirm
that a file came from the release page unchanged.

## Build from source

Requirements on every system: Git, Node.js 24 or newer (with Corepack, which
ships with Node), and Rust through `rustup`. The exact Rust version is pinned
in `rust-toolchain.toml` and installed automatically on first build; the
pnpm version is pinned in `package.json`.

| System | Extra requirements |
|---|---|
| macOS | Xcode Command Line Tools (`xcode-select --install`) |
| Windows | Visual Studio Build Tools with "Desktop development with C++"; WebView2 (included in Windows 11) |
| Linux (Debian/Ubuntu) | `libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libssl-dev build-essential` |

```bash
git clone https://github.com/mehaskari/Termalith.git
cd Termalith
corepack pnpm install --frozen-lockfile
corepack pnpm --filter @termalith/desktop tauri build
```

The app is written to `target/release/bundle/` (on macOS
`target/release/bundle/macos/Termalith.app`). `corepack pnpm dev` runs it
in development mode.

Checks run in CI on every pull request:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
corepack pnpm typecheck
corepack pnpm test
```

## Make a release

Releases are built by the **Release builds** workflow on native GitHub
runners for macOS (Apple Silicon, Intel), Windows (x64, ARM64) and Linux
(x64, ARM64). Builds are reproducible from the tag: Rust, pnpm and every
dependency are pinned (`rust-toolchain.toml`, `Cargo.lock`,
`pnpm-lock.yaml`), and the workflow actions are pinned to commits.

1. Make sure `main` is green in CI.
2. Set the version in `Cargo.toml` (workspace), `apps/desktop/package.json`
   and `apps/desktop/src-tauri/tauri.conf.json`.
3. Move the *Unreleased* entries in the changelog under the new version.
4. If dependencies changed, regenerate the license notices with
   `scripts/third-party-licenses.sh`.
5. Merge, then tag the merge commit and push the tag:
   `git tag v0.5.0 && git push origin v0.5.0`.
6. The workflow builds every installer and creates a **draft** GitHub
   Release with the installers and `SHA256SUMS.txt`.
7. Install at least the builds you can test, check the release notes, then
   publish the draft.

Code signing (Apple Developer ID with notarization, a Windows code-signing
certificate) and automatic updates are not set up yet. The app's
**Check for updates** compares the running version with the latest
published release and links to it; it never downloads or installs
anything by itself.
