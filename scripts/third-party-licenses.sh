#!/usr/bin/env bash
# Regenerates website/docs/third-party.md: license texts of every Rust crate
# (cargo-about, policy in about.toml) and frontend package (pnpm) that ships
# in Termalith. Run after changing dependencies:
#   cargo install cargo-about --version 0.9.3 --locked --features cli
#   scripts/third-party-licenses.sh
set -euo pipefail
cd "$(dirname "$0")/.."
out=website/docs/third-party.md
cargo about generate --workspace scripts/about.hbs -o "$out"
(cd apps/desktop && corepack pnpm licenses list --prod --json) | node scripts/npm-licenses.mjs >> "$out"
echo "wrote $out"
