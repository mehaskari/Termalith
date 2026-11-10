#!/usr/bin/env python3
"""Fails when the user documentation falls behind the code.

Run locally:   python3 scripts/check-docs.py
In CI (PRs):   python3 scripts/check-docs.py --base origin/main

Checks
  1. One version everywhere: Cargo.toml (workspace), package.json,
     tauri.conf.json, the website's landing page; the changelog has an entry
     for it (or an "Unreleased" section above it).
  2. Every setting in the Settings dialog is described in guide/settings.md.
  3. Every session type in the session dialog is described in guide/sessions.md.
  4. Every guide page is in the website navigation, and every image or page
     a guide page links to exists.
  5. With --base: a pull request that changes the app's user-facing code
     also changes the user guide or the changelog, unless it carries the
     label "no-docs-needed" (or a commit says "[no docs]").
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DOCS = ROOT / "website" / "docs"
GUIDE = DOCS / "guide"
errors: list[str] = []


def fail(msg: str) -> None:
    errors.append(msg)


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def norm(text: str) -> str:
    """Lower-case words only, so markup and punctuation do not matter."""
    return " ".join(re.findall(r"[a-z0-9]+", text.lower()))


def check_versions() -> None:
    cargo = re.search(r'^\[workspace\.package\][^\[]*?^version\s*=\s*"([^"]+)"', read("Cargo.toml"), re.M | re.S)
    versions = {
        "Cargo.toml": cargo.group(1) if cargo else None,
        "apps/desktop/package.json": json.loads(read("apps/desktop/package.json"))["version"],
        "apps/desktop/src-tauri/tauri.conf.json": json.loads(read("apps/desktop/src-tauri/tauri.conf.json"))["version"],
    }
    home = read("website/overrides/home.html")
    m = re.search(r'"softwareVersion":"([^"]+)"', home)
    versions["website landing page (schema.org softwareVersion)"] = m.group(1) if m else None
    m = re.search(r'<span class="cmd">Termalith ([0-9][^ <]*)', home)
    versions["website landing page (hero)"] = m.group(1) if m else None
    expected = versions["Cargo.toml"]
    for where, v in versions.items():
        if v != expected:
            fail(f"version {v!r} in {where} differs from Cargo.toml ({expected!r})")
    changelog = read("website/docs/changelog.md")
    heads = re.findall(r"^## (.+)$", changelog, re.M)
    if not heads:
        fail("changelog has no version headings")
    elif not (heads[0].startswith(expected) or heads[0].lower().startswith("unreleased")):
        fail(f"changelog: first heading {heads[0]!r} is neither {expected} nor 'Unreleased'")
    if not any(h.startswith(expected) for h in heads):
        fail(f"changelog has no entry for {expected}")


def jsx_labels(source: str) -> list[str]:
    """Visible label texts of form controls in a dialog (checkboxes and fields)."""
    out = []
    # <label ...> <input type="checkbox" .../> Text
    for m in re.finditer(r'<label className="checkbox[^"]*">\s*<input\b.*?/>\s*([^<{]+?)\s*</label>', source, re.S):
        out.append(m.group(1))
    # <label ...> Text <select|input|span
    for m in re.finditer(r"<label(?: className=\"(?!checkbox)[^\"]*\")?>\s*\n\s*([A-Z][^<{\n]+?)\s*\n\s*<", source):
        out.append(m.group(1))
    return [" ".join(t.split()) for t in out]


def check_settings() -> None:
    labels = jsx_labels(read("apps/desktop/src/ui/SettingsDialog.tsx"))
    if len(labels) < 10:
        fail(f"check-docs found only {len(labels)} settings in SettingsDialog.tsx; update the parser")
    doc = norm((GUIDE / "settings.md").read_text(encoding="utf-8"))
    for label in labels:
        # The first words of a label identify it; the rest is explanation.
        key = norm(re.split(r"[(;]", label)[0])
        if key and key not in doc:
            fail(f"setting {label!r} (SettingsDialog.tsx) is not described in website/docs/guide/settings.md")


def check_session_types() -> None:
    dialog = read("apps/desktop/src/ui/SessionDialog.tsx")
    types = re.findall(r'<option value="(?:ssh|rdp|vnc|telnet|ftps|ftp)">([^<]+)</option>', dialog)
    if len(types) < 6:
        fail(f"check-docs found only {len(types)} session types in SessionDialog.tsx; update the parser")
    doc = norm((GUIDE / "sessions.md").read_text(encoding="utf-8"))
    for t in types:
        if norm(t) not in doc:
            fail(f"session type {t!r} (SessionDialog.tsx) is not described in website/docs/guide/sessions.md")


def check_guide_pages() -> None:
    nav = read("website/mkdocs.yml")
    for page in sorted(GUIDE.glob("*.md")):
        rel = f"guide/{page.name}"
        if rel not in nav:
            fail(f"{rel} is not in the navigation of website/mkdocs.yml")
        text = page.read_text(encoding="utf-8")
        for target in re.findall(r"\]\(([^)#\s]+)", text):
            if re.match(r"[a-z]+:", target):
                continue
            if not (page.parent / target).resolve().exists():
                fail(f"{rel} links to {target}, which does not exist")


USER_FACING = re.compile(r"^(apps/desktop/src/(?!dev/).*\.(tsx|ts)|apps/desktop/src-tauri/src/.*\.rs|crates/app-core/src/settings\.rs)$")
DOC_PATHS = re.compile(r"^(website/docs/(guide/|changelog\.md)|README\.md)")


def check_pr(base: str) -> None:
    changed = subprocess.run(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=ROOT,
                             capture_output=True, text=True, check=True).stdout.split()
    code = [f for f in changed if USER_FACING.match(f) and not re.search(r"\.test\.tsx?$", f)]
    docs = [f for f in changed if DOC_PATHS.match(f)]
    if not code or docs:
        return
    labels: list[str] = []
    event = os.environ.get("GITHUB_EVENT_PATH")
    if event and Path(event).exists():
        data = json.loads(Path(event).read_text(encoding="utf-8"))
        labels = [l["name"] for l in data.get("pull_request", {}).get("labels", [])]
    messages = subprocess.run(["git", "log", "--format=%B", f"{base}..HEAD"], cwd=ROOT,
                              capture_output=True, text=True, check=True).stdout
    if "no-docs-needed" in labels or "[no docs]" in messages.lower():
        print("docs: user-facing code changed without docs; allowed by 'no-docs-needed' / '[no docs]'.")
        return
    fail("this change touches user-facing code but not the user guide (website/docs/guide/), the changelog "
         "or the README:\n    " + "\n    ".join(code) +
         "\n  Update the guide/changelog, or label the PR 'no-docs-needed' (or write '[no docs]' in a commit "
         "message) if users see no difference.")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--base", help="git ref of the PR base, e.g. origin/main")
    args = ap.parse_args()
    check_versions()
    check_settings()
    check_session_types()
    check_guide_pages()
    if args.base:
        check_pr(args.base)
    if errors:
        print("Documentation check failed:")
        for e in errors:
            print("  - " + e)
        return 1
    print("Documentation check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
