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
   Release with the installers and `SHA256SUMS.txt`. Tags with a suffix
   (`v0.5.0-rc.4`) become pre-releases; plain tags (`v0.5.0`) do not.
7. Install at least the builds you can test, check the release notes, then
   publish the draft.

Code signing (Apple Developer ID with notarization, a Windows code-signing
certificate) and automatic updates are not set up yet. The app's
**Check for updates** compares the running version with the latest
published release and links to it; it never downloads or installs
anything by itself.

## Keeping the documentation current

The user guide (`website/docs/guide/`), the README and the changelog are
part of every change, and CI enforces it. `scripts/check-docs.py` runs in the
required *cargo-deny* job on every pull request and on `main`, and fails
when:

- the version differs between `Cargo.toml`, `apps/desktop/package.json`,
  `apps/desktop/src-tauri/tauri.conf.json` and the website's landing page, or
  the changelog has no entry for it;
- a setting in the Settings dialog is not described in `guide/settings.md`,
  or a session type in the session dialog is missing from
  `guide/sessions.md`;
- a guide page is not in the site navigation, or links to a page or image
  that does not exist;
- a pull request changes user-facing code (`apps/desktop/src/`,
  `apps/desktop/src-tauri/src/`) without touching the guide, the changelog
  or the README. If users really see no difference, add the label
  `no-docs-needed` before pushing, or write `[no docs]` in a commit
  message.

The website build (`mkdocs build --strict`) fails on broken links, and
download links on the website and in the README point to the latest release
instead of a version number, so they never go stale.

**Screenshots** come from the UI mock and can be regenerated after UI
changes:

```bash
corepack pnpm --filter @termalith/desktop dev     # serves the mock on :1420
node scripts/docs-screenshots.mjs                 # Chrome required, no npm packages
python3 scripts/optimize-screenshots.py           # optional, needs Pillow
```

Run locally before pushing: `python3 scripts/check-docs.py --base origin/main`.
