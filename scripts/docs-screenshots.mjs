// Regenerates the screenshots in website/docs/assets/screenshots from the UI
// mock (no Rust backend needed), through Chrome's DevTools protocol. No npm
// dependencies: Node 22+ (built-in WebSocket) and an installed Chrome/Chromium.
//
//   corepack pnpm --filter @termalith/desktop dev      # serves the mock on :1420
//   node scripts/docs-screenshots.mjs                  # writes 2x PNGs
//   python3 scripts/optimize-screenshots.py            # optional: smaller files (Pillow)
//
// CHROME=/path/to/chrome overrides the browser. The plan below names every
// screenshot the guide uses; when the UI changes, run it and commit the images.
import { spawn } from "node:child_process";
import { writeFileSync, mkdtempSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "website", "docs", "assets", "screenshots");
const MOCK = process.env.MOCK_URL || "http://localhost:1420/mock.html";
const CHROMES = [process.env.CHROME, "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser",
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"].filter(Boolean);
const chrome = CHROMES.find((c) => existsSync(c));
if (!chrome) { console.error("No Chrome found; set CHROME=/path/to/chrome"); process.exit(1); }

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const TRUST = `document.querySelectorAll("dialog button").forEach(b => { if (/Trust/.test(b.textContent)) b.click(); })`;
const PASSWORD = `document.querySelector("dialog input[type=password]")?.focus()`;
const TERMINAL = `document.querySelector(".xterm-helper-textarea")?.focus()`;
// The mock accepts the password "secret".
const login = (session) => [
  { load: true, click: session, wait: 2500 },
  { js: TRUST, wait: 1500 }, { js: PASSWORD, wait: 300 }, { type: "secret", enter: true, wait: 2000 },
];

const PLAN = [
  { load: true, click: "web-01, SSH", wait: 2500, shot: "host-key", clip: "dialog" },
  { js: TRUST, wait: 1500, shot: "password", clip: "dialog" },
  ...login("web-01, SSH").slice(2), { js: TERMINAL, wait: 300 },
  { type: "systemctl is-active nginx", enter: true },
  { click: "Files", wait: 1800, shot: "main-window" },
  { click: "Tunnels", wait: 1200, shot: "tunnels" },
  { load: true, click: "New session", wait: 1200, shot: "session-dialog", clip: "dialog" },
  { load: true, click: "Settings", wait: 1200, shot: "settings", clip: "dialog" },
  { load: true, click: "Keys", wait: 1200, shot: "ssh-keys", clip: "dialog" },
  { load: true, click: "Tools", wait: 1200, shot: "network-tools" },
  { load: true, click: "Four panes", wait: 1500, shot: "four-panes" },
  { load: true, click: "Snippets", wait: 1200, shot: "snippets" },
  ...login("backups, SFTP"), { wait: 1500, shot: "file-browser" },
];

let proc;
let ws, seq = 0;
const pending = new Map();
const cmd = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++seq;
  const timer = setTimeout(() => { pending.delete(id); reject(new Error(`${method} timed out`)); }, 15000);
  pending.set(id, (m) => { clearTimeout(timer); resolve(m); });
  ws.send(JSON.stringify({ id, method, params }));
});
const js = async (expression) => {
  const r = await cmd("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
  if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || expression);
  return r.result?.result?.value;
};
/**
 * Runs `fn` in the page with `args` passed as values (never pasted into
 * code), so labels and selectors cannot change what runs.
 */
const call = async (fn, ...args) => {
  const doc = await cmd("Runtime.evaluate", { expression: "document" });
  const r = await cmd("Runtime.callFunctionOn", {
    objectId: doc.result.result.objectId,
    functionDeclaration: fn.toString(),
    arguments: args.map((value) => ({ value })),
    awaitPromise: true,
    returnByValue: true,
  });
  if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || String(fn));
  return r.result?.result?.value;
};
const click = (label) =>
  call((want) => {
    const els = [...document.querySelectorAll("button,[role=tab],[role=menuitem],a,label,summary")];
    const el = els.find((e) => e.getAttribute("aria-label") === want) || els.find((e) => (e.textContent || "").trim() === want)
      || els.find((e) => (e.getAttribute("aria-label") || "").startsWith(want)) || els.find((e) => (e.textContent || "").trim().startsWith(want));
    if (!el) throw new Error("not found: " + want);
    el.scrollIntoView({ block: "center" });
    el.click();
  }, label);
const key = async (k, code, text) => {
  await cmd("Input.dispatchKeyEvent", { type: "keyDown", key: k, code, windowsVirtualKeyCode: code === "Enter" ? 13 : 0, ...(text ? { text } : {}) });
  await cmd("Input.dispatchKeyEvent", { type: "keyUp", key: k, code });
};
async function shot(name, clipSelector) {
  await sleep(500);
  // Cosmetic: lists scrolled to the top, no "(mock)" marker in terminal text.
  await js(`(() => { document.querySelectorAll("*").forEach(e => { if (e.scrollTop > 0 && !e.closest(".xterm")) e.scrollTop = 0; });
    document.querySelectorAll(".xterm-rows span, .xterm-rows div").forEach(s => { if (s.childElementCount === 0 && s.textContent.includes("(mock"))
      s.textContent = s.textContent.replace(/ ?\\(mock[^)]*\\)/g, ""); }); })()`);
  let clip;
  if (clipSelector) {
    const b = await call((sel) => {
      const e = document.querySelector(sel);
      if (!e) return null;
      const r = e.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    }, clipSelector);
    if (b) clip = { x: Math.max(0, b.x - 12), y: Math.max(0, b.y - 12), width: b.width + 24, height: b.height + 24, scale: 1 };
  }
  const r = await cmd("Page.captureScreenshot", { format: "png", ...(clip ? { clip } : {}) });
  writeFileSync(join(OUT, `${name}.png`), Buffer.from(r.result.data, "base64"));
  console.log("saved", name);
}

// Each "load" starts a fresh headless Chrome: long sessions in one browser
// crashed it, and a clean profile keeps the shots reproducible.
async function openTab(url) {
  if (ws) ws.close();
  if (proc) { proc.kill(); await sleep(500); }
  proc = spawn(chrome, ["--headless=new", "--remote-debugging-port=9333", `--user-data-dir=${mkdtempSync(join(tmpdir(), "termalith-shots-"))}`,
    "--no-first-run", "--hide-scrollbars", "--disable-webgl", "--disable-3d-apis", url], { stdio: "ignore" });
  let page;
  for (let i = 0; i < 100 && !page; i++) {
    try { page = (await (await fetch("http://127.0.0.1:9333/json/list")).json()).find((t) => t.type === "page"); } catch {}
    if (!page) await sleep(200);
  }
  ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  ws.addEventListener("message", (e) => {
    const m = JSON.parse(e.data);
    if (m.method === "Page.javascriptDialogOpening") ws.send(JSON.stringify({ id: ++seq, method: "Page.handleJavaScriptDialog", params: { accept: true } }));
    if (pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
  });
  await cmd("Page.enable");
  // Tall enough for the longest dialog; dark theme like the app's default.
  await cmd("Emulation.setDeviceMetricsOverride", { width: 1360, height: 1000, deviceScaleFactor: 2, mobile: false });
  await cmd("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: "dark" }] });
  await cmd("Page.reload");
  await sleep(2500);
}

try {
  let failed = 0;
  for (const s of PLAN) {
    try {
      if (s.load) await openTab(MOCK);
      if (s.click) { await click(s.click); }
      if (s.js) await js(s.js);
      if (s.type) { for (const ch of s.type) { await cmd("Input.dispatchKeyEvent", { type: "char", text: ch }); await sleep(15); } if (s.enter) await key("Enter", "Enter", "\r"); }
      await sleep(s.wait ?? 600);
      if (s.shot) await shot(s.shot, s.clip);
    } catch (e) { failed++; console.error("step failed:", JSON.stringify(s), String(e).slice(0, 200)); }
  }
  process.exitCode = failed ? 1 : 0;
} finally { if (proc) proc.kill(); }
