import { type FormEvent, type ReactNode, useEffect, useMemo, useState } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Globe,
  Highlighter,
  Keyboard,
  Network,
  Palette,
  RefreshCw,
  RotateCcw,
  Search,
  ShieldCheck,
  SquareTerminal,
  Users,
  X,
} from "lucide-react";
import {
  DEFAULT_SETTINGS,
  type Settings,
  type ShellChoice,
  isIpcError,
  proxyPasswordStatus,
  setProxyPassword,
} from "../terminal/api";
import { Dialog } from "./Dialog";
import { MasterPasswordSettings } from "./MasterPassword";
import { SharedSessionsSettings } from "./SharedSessions";
import { isMacPlatform, keyCaps, shortcutHelp } from "./shortcuts";
import { TERMINAL_SCHEMES, fontStack, resolveTheme, terminalTheme } from "./theme";

interface Props {
  initial: Settings;
  /** Installed shells for the default-shell choice. */
  shells: ShellChoice[];
  onSave: (s: Settings) => Promise<void>;
  onCancel: () => void;
  /** The OS credential store works, so passwords can be saved (and protected). */
  secretsAvailable?: boolean;
  /** Saved passwords were deleted (forgotten master password). */
  onSecretsReset?: () => void;
  /** Running from a portable folder (passwords only with a master password). */
  portable?: boolean;
}

export type SectionId =
  | "appearance"
  | "terminal"
  | "keywords"
  | "connections"
  | "proxy"
  | "security"
  | "shared"
  | "updates"
  | "shortcuts";

export const SECTIONS: Array<{ id: SectionId; label: string; icon: LucideIcon; blurb: string }> = [
  { id: "appearance", label: "Appearance", icon: Palette, blurb: "Theme, terminal colors, font and cursor." },
  { id: "terminal", label: "Terminal", icon: SquareTerminal, blurb: "Scrollback, clipboard, shells and logs." },
  { id: "keywords", label: "Keyword colors", icon: Highlighter, blurb: "Color words such as error and ok in output." },
  { id: "connections", label: "Connections", icon: Network, blurb: "Reconnecting, remote desktops and the server monitor." },
  { id: "proxy", label: "Proxy", icon: Globe, blurb: "Reach servers through a SOCKS5 or HTTP proxy." },
  { id: "security", label: "Security", icon: ShieldCheck, blurb: "Master password and locking saved passwords." },
  { id: "shared", label: "Shared sessions", icon: Users, blurb: "Session lists shared by your team." },
  { id: "updates", label: "Updates", icon: RefreshCw, blurb: "Hear about new versions." },
  { id: "shortcuts", label: "Keyboard shortcuts", icon: Keyboard, blurb: "Keys that work everywhere in Termalith." },
];

/** Every setting by section, with the words a search finds it by. */
export const SETTINGS_INDEX: Array<{ id: string; section: SectionId; label: string; words: string }> = [
  { id: "theme", section: "appearance", label: "Theme", words: "dark light system mode" },
  { id: "scheme", section: "appearance", label: "Terminal colors", words: "color scheme palette solarized" },
  { id: "font", section: "appearance", label: "Terminal font", words: "typeface family monospace" },
  { id: "font-size", section: "appearance", label: "Font size", words: "text zoom bigger smaller" },
  { id: "line-height", section: "appearance", label: "Line height", words: "spacing rows" },
  { id: "cursor", section: "appearance", label: "Cursor", words: "block bar underline blink" },
  { id: "scrollback", section: "terminal", label: "Scrollback", words: "history lines buffer" },
  { id: "shell", section: "terminal", label: "Shell for new local terminals", words: "bash zsh powershell cmd default" },
  { id: "copy-on-select", section: "terminal", label: "Copy selected text", words: "clipboard selection automatically" },
  { id: "right-click-paste", section: "terminal", label: "Right-click pastes", words: "mouse clipboard paste" },
  { id: "paste-warning", section: "terminal", label: "Warn before risky pastes", words: "paste multiple lines clipboard" },
  { id: "confirm-close", section: "terminal", label: "Ask before closing a connected tab", words: "close tab confirm quit" },
  { id: "log", section: "terminal", label: "Log every terminal", words: "logging record file documents" },
  { id: "highlight", section: "keywords", label: "Color keywords in output", words: "highlight error warning ok red yellow green" },
  { id: "reconnect", section: "connections", label: "Reconnect automatically", words: "drop lost ssh reconnect" },
  { id: "embedded-rdp", section: "connections", label: "Show RDP and VNC inside Termalith", words: "remote desktop embedded vnc" },
  { id: "monitor", section: "connections", label: "Server monitor", words: "cpu memory load network bar" },
  { id: "proxy", section: "proxy", label: "Proxy", words: "socks socks5 http connect firewall" },
  { id: "master", section: "security", label: "Master password", words: "protect encrypt vault passwords" },
  { id: "auto-lock", section: "security", label: "Lock saved passwords when idle", words: "idle timeout lock away" },
  { id: "shared", section: "shared", label: "Shared sessions", words: "team network share https" },
  { id: "update-check", section: "updates", label: "Tell me about new versions", words: "update version release github" },
  { id: "shortcuts", section: "shortcuts", label: "Keyboard shortcuts", words: "keys hotkeys cmd ctrl" },
];

/** Settings whose label, section or search words contain every typed word. */
export function searchSettings(query: string) {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [];
  return SETTINGS_INDEX.filter((e) => {
    const section = SECTIONS.find((s) => s.id === e.section)?.label ?? "";
    const hay = `${e.label} ${section} ${e.words}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

/** The extra keyword lists, with their labels. */
const KEYWORD_LISTS = [
  ["highlightBad", "Red", "bad"],
  ["highlightWarn", "Yellow", "warn"],
  ["highlightGood", "Green", "good"],
] as const;

/** "a, b,,c " → ["a", "b", "c"]. */
export function splitWords(text: string): string[] {
  return text
    .split(",")
    .map((w) => w.trim())
    .filter(Boolean);
}

/** Idle times offered for locking saved passwords, in minutes (0 = never). */
export const AUTO_LOCK_CHOICES: Array<[number, string]> = [
  [0, "Never"],
  [5, "5 minutes"],
  [15, "15 minutes"],
  [30, "30 minutes"],
  [60, "1 hour"],
  [240, "4 hours"],
];

/** Line heights offered, in percent of the font size. */
export const LINE_HEIGHTS = [100, 110, 120, 130, 150];

/** Monospace fonts suggested for the terminal (any installed font can be typed). */
const FONT_SUGGESTIONS = [
  "JetBrains Mono",
  "Fira Code",
  "Cascadia Code",
  "SF Mono",
  "Menlo",
  "Consolas",
  "Source Code Pro",
  "Ubuntu Mono",
  "DejaVu Sans Mono",
];

/** The first problem with `s`, and the section that shows it; null when it can be saved. */
export function settingsProblem(s: Settings): { section: SectionId; message: string } | null {
  if (!Number.isInteger(s.fontSize) || s.fontSize < 8 || s.fontSize > 32)
    return { section: "appearance", message: "Font size must be 8–32." };
  if (!Number.isInteger(s.scrollback) || s.scrollback < 100 || s.scrollback > 100000)
    return { section: "terminal", message: "Scrollback must be 100–100,000 lines." };
  if (s.proxy && (!s.proxy.host.trim() || !Number.isInteger(s.proxy.port) || s.proxy.port < 1 || s.proxy.port > 65535))
    return { section: "proxy", message: "Enter the proxy host and a port (1–65535)." };
  return null;
}

// ------------------------------------------------------------ building blocks

/** One setting: its name and explanation on the left, the control on the right. */
function Row({
  id,
  label,
  hint,
  children,
  wide,
}: {
  id: string;
  label: string;
  hint?: ReactNode;
  children: ReactNode;
  /** Control under the text (lists, long inputs). */
  wide?: boolean;
}) {
  return (
    <div className={`setting-row ${wide ? "wide" : ""}`} id={`setting-${id}`}>
      <div className="setting-text">
        <label className="setting-label" htmlFor={`set-${id}`}>
          {label}
        </label>
        {hint && <p className="setting-hint">{hint}</p>}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

/** An on/off switch (a checkbox with the switch role). */
function Toggle({ id, checked, onChange }: { id: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <input
      id={`set-${id}`}
      type="checkbox"
      role="switch"
      className="switch"
      checked={checked}
      onChange={(e) => onChange(e.target.checked)}
    />
  );
}

/** A small set of choices as joined buttons. */
function Segmented<T extends string>({
  id,
  value,
  options,
  onChange,
  label,
}: {
  id: string;
  value: T;
  options: Array<[T, string]>;
  onChange: (v: T) => void;
  label: string;
}) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label} id={`set-${id}`}>
      {options.map(([v, text]) => (
        <button
          key={v}
          type="button"
          role="radio"
          aria-checked={value === v}
          className={value === v ? "on" : undefined}
          onClick={() => onChange(v)}
        >
          {text}
        </button>
      ))}
    </div>
  );
}

function Card({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="settings-card">
      {title && <h4 className="settings-card-title">{title}</h4>}
      {children}
    </section>
  );
}

/** A comma-separated word list: the text stays as typed, the list is clean. */
function WordListInput({ id, words, onChange }: { id: string; words: string[]; onChange: (w: string[]) => void }) {
  const [text, setText] = useState(() => words.join(", "));
  useEffect(() => {
    // Reset to defaults empties the list.
    if (words.length === 0) setText((t) => (splitWords(t).length ? "" : t));
  }, [words]);
  return (
    <input
      id={`set-${id}`}
      value={text}
      placeholder="Comma-separated, e.g. err-disabled, down"
      spellCheck={false}
      autoCapitalize="off"
      onChange={(e) => {
        setText(e.target.value);
        onChange(splitWords(e.target.value));
      }}
    />
  );
}

/** A few lines of terminal text drawn with the chosen colors, font and cursor. */
function TerminalPreview({ s }: { s: Settings }) {
  const systemDark = typeof matchMedia === "function" && matchMedia("(prefers-color-scheme: dark)").matches;
  // Same fallbacks as the keyword highlighter (and xterm's blue) when a scheme has no palette.
  const t = { red: "#e06c75", yellow: "#e5c07b", green: "#98c379", blue: "#61afef", ...terminalTheme(s.terminalScheme, resolveTheme(s.theme, systemDark)) };
  const hl = s.highlightOutput;
  const word = (text: string, color: string | undefined) => (hl ? <span style={{ color }}>{text}</span> : text);
  return (
    <div
      className="terminal-preview"
      aria-label="Preview"
      role="img"
      style={{
        background: t.background,
        color: t.foreground,
        fontFamily: fontStack(s.fontFamily),
        fontSize: s.fontSize,
        lineHeight: (s.lineHeight || 100) / 100,
      }}
    >
      <div>
        <span style={{ color: t.green }}>admin@core-sw1</span>:<span style={{ color: t.blue }}>~</span>
        {"$ show interfaces status"}
      </div>
      <div>
        {"Gi0/1   "}
        {word("connected", t.green)}
        {"      1000   a-full"}
      </div>
      <div>
        {"Gi0/2   "}
        {word("err-disabled", t.red)}
        {"   auto   "}
        {word("warning", t.yellow)}
        {": bpdu guard"}
      </div>
      <div>
        <span style={{ color: t.green }}>admin@core-sw1</span>:<span style={{ color: t.blue }}>~</span>${" "}
        <span
          className={`preview-cursor cursor-${s.cursorStyle} ${s.cursorBlink ? "blink" : ""}`}
          style={{ ["--cursor" as string]: t.cursor ?? t.foreground }}
        >
          &nbsp;
        </span>
      </div>
    </div>
  );
}

// ------------------------------------------------------------ the dialog

export function SettingsDialog({ initial, shells, onSave, onCancel, secretsAvailable = false, onSecretsReset, portable = false }: Props) {
  const start = useMemo(() => ({ ...DEFAULT_SETTINGS, ...initial }), [initial]);
  const [s, setS] = useState<Settings>(start);
  const [section, setSection] = useState<SectionId>("appearance");
  const [query, setQuery] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<Settings>) => {
    setS((x) => ({ ...x, ...patch }));
    setError(null);
  };
  /** A password is saved in the OS store (never loaded into the UI). */
  const [hasProxyPassword, setHasProxyPassword] = useState(false);
  /** Typed replacement; empty keeps the saved one. Cleared after saving. */
  const [proxyPassword, setProxyPasswordInput] = useState("");
  const [forgetProxyPassword, setForgetProxyPassword] = useState(false);
  useEffect(() => {
    proxyPasswordStatus().then(setHasProxyPassword, () => {});
  }, []);
  const mac = isMacPlatform();
  const dirty = JSON.stringify(s) !== JSON.stringify(start) || proxyPassword !== "" || forgetProxyPassword;
  const results = searchSettings(query);
  const current = SECTIONS.find((x) => x.id === section) ?? (SECTIONS[0] as (typeof SECTIONS)[number]);

  // After jumping to a search result, bring the row into view and mark it briefly.
  useEffect(() => {
    if (!flash) return;
    const el = document.getElementById(`setting-${flash}`);
    el?.scrollIntoView?.({ block: "center" });
    el?.classList.add("flash");
    const t = setTimeout(() => {
      el?.classList.remove("flash");
      setFlash(null);
    }, 1600);
    return () => clearTimeout(t);
  }, [flash, section]);

  const go = (id: SectionId, row?: string) => {
    setSection(id);
    setQuery("");
    if (row) setFlash(row);
  };

  async function submit(e: FormEvent) {
    e.preventDefault();
    const problem = settingsProblem(s);
    if (problem) {
      setSection(problem.section);
      setQuery("");
      return setError(problem.message);
    }
    const login = !!s.proxy?.username?.trim();
    setBusy(true);
    try {
      // The password goes to the OS store first, so a failure keeps the dialog open.
      if (login && proxyPassword) await setProxyPassword(proxyPassword);
      else if ((!login || forgetProxyPassword) && hasProxyPassword) await setProxyPassword(null);
      setProxyPasswordInput("");
      await onSave(s);
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  const resetDefaults = () => {
    // Keeps the proxy (with its saved password) and the shell: they describe this computer.
    setS({ ...DEFAULT_SETTINGS, proxy: s.proxy, defaultShell: s.defaultShell });
    setError(null);
  };

  const body = (() => {
    switch (section) {
      case "appearance":
        return (
          <>
            <TerminalPreview s={s} />
            <Card>
              <Row id="theme" label="Theme" hint="Colors of the app around the terminals.">
                <Segmented
                  id="theme"
                  label="Theme"
                  value={s.theme}
                  options={[
                    ["system", "System"],
                    ["dark", "Dark"],
                    ["light", "Light"],
                  ]}
                  onChange={(theme) => set({ theme })}
                />
              </Row>
              <Row id="scheme" label="Terminal colors" hint="A session can choose its own in its Appearance section.">
                <select id="set-scheme" value={s.terminalScheme} onChange={(e) => set({ terminalScheme: e.target.value })}>
                  {TERMINAL_SCHEMES.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
              </Row>
            </Card>
            <Card title="Text">
              <Row id="font" label="Terminal font" hint="Any installed monospace font; empty uses the system's.">
                <input
                  id="set-font"
                  list="settings-fonts"
                  value={s.fontFamily}
                  placeholder="Default (Menlo / Consolas / DejaVu Sans Mono)"
                  spellCheck={false}
                  onChange={(e) => set({ fontFamily: e.target.value })}
                />
                <datalist id="settings-fonts">
                  {FONT_SUGGESTIONS.map((f) => (
                    <option key={f} value={f} />
                  ))}
                </datalist>
              </Row>
              <Row id="font-size" label="Font size" hint={<>8–32 points. {mac ? "⌘" : "Ctrl+Shift+"}+ and − change it too.</>}>
                <span className="stepper">
                  <button type="button" aria-label="Smaller" disabled={s.fontSize <= 8} onClick={() => set({ fontSize: s.fontSize - 1 })}>
                    −
                  </button>
                  <input
                    id="set-font-size"
                    type="number"
                    min={8}
                    max={32}
                    value={s.fontSize}
                    onChange={(e) => set({ fontSize: Number(e.target.value) })}
                  />
                  <button type="button" aria-label="Larger" disabled={s.fontSize >= 32} onClick={() => set({ fontSize: s.fontSize + 1 })}>
                    +
                  </button>
                </span>
              </Row>
              <Row id="line-height" label="Line height" hint="More room between lines makes long output easier to read.">
                <select id="set-line-height" value={s.lineHeight} onChange={(e) => set({ lineHeight: Number(e.target.value) })}>
                  {LINE_HEIGHTS.map((h) => (
                    <option key={h} value={h}>
                      {h === 100 ? "Normal" : `${h} %`}
                    </option>
                  ))}
                </select>
              </Row>
            </Card>
            <Card title="Cursor">
              <Row id="cursor" label="Cursor shape">
                <Segmented
                  id="cursor"
                  label="Cursor shape"
                  value={s.cursorStyle}
                  options={[
                    ["block", "Block"],
                    ["bar", "Bar"],
                    ["underline", "Underline"],
                  ]}
                  onChange={(cursorStyle) => set({ cursorStyle })}
                />
              </Row>
              <Row id="cursor-blink" label="Blinking cursor">
                <Toggle id="cursor-blink" checked={s.cursorBlink} onChange={(cursorBlink) => set({ cursorBlink })} />
              </Row>
            </Card>
          </>
        );
      case "terminal":
        return (
          <>
            <Card>
              <Row id="scrollback" label="Scrollback" hint="Lines kept for scrolling back (100–100,000).">
                <span className="with-unit">
                  <input
                    id="set-scrollback"
                    type="number"
                    min={100}
                    max={100000}
                    step={1000}
                    value={s.scrollback}
                    onChange={(e) => set({ scrollback: Number(e.target.value) })}
                  />
                  <span className="unit">lines</span>
                </span>
              </Row>
              {shells.length > 1 && (
                <Row id="shell" label="Shell for new local terminals">
                  <select id="set-shell" value={s.defaultShell ?? ""} onChange={(e) => set({ defaultShell: e.target.value || null })}>
                    <option value="">System default ({shells[0]?.name})</option>
                    {shells.map((sh) => (
                      <option key={sh.id} value={sh.id}>
                        {sh.name}
                      </option>
                    ))}
                  </select>
                </Row>
              )}
            </Card>
            <Card title="Clipboard">
              <Row id="copy-on-select" label="Copy selected text automatically" hint="Selecting text in a terminal copies it.">
                <Toggle id="copy-on-select" checked={s.copyOnSelect} onChange={(copyOnSelect) => set({ copyOnSelect })} />
              </Row>
              <Row id="right-click-paste" label="Right-click pastes" hint="Instead of opening the menu.">
                <Toggle id="right-click-paste" checked={s.rightClickPaste} onChange={(rightClickPaste) => set({ rightClickPaste })} />
              </Row>
              <Row
                id="paste-warning"
                label="Warn before risky pastes"
                hint="Several lines, a download piped into a shell, or hidden characters."
              >
                <Toggle id="paste-warning" checked={s.pasteWarning} onChange={(pasteWarning) => set({ pasteWarning })} />
              </Row>
            </Card>
            <Card title="Tabs and logs">
              <Row
                id="confirm-close"
                label="Ask before closing a connected tab"
                hint="Closing a tab ends its SSH, Telnet, serial or desktop connection."
              >
                <Toggle id="confirm-close" checked={s.confirmClose} onChange={(confirmClose) => set({ confirmClose })} />
              </Row>
              <Row
                id="log"
                label="Log every new terminal"
                hint="Plain text in Documents/Termalith Logs. Logs may contain sensitive output."
              >
                <Toggle id="log" checked={s.logSessions} onChange={(logSessions) => set({ logSessions })} />
              </Row>
            </Card>
          </>
        );
      case "keywords":
        return (
          <>
            <Card>
              <Row id="highlight" label="Color keywords in terminal output" hint="Words like error, warning and ok, in red, yellow and green.">
                <Toggle id="highlight" checked={s.highlightOutput} onChange={(highlightOutput) => set({ highlightOutput })} />
              </Row>
            </Card>
            <Card title="Your own words">
              {KEYWORD_LISTS.map(([key, color, tone]) => (
                <Row
                  key={key}
                  id={key}
                  label={`${color} words`}
                  hint={!s.highlightOutput ? "Used when keyword colors are on." : undefined}
                >
                  <span className="keyword-input">
                    <span className={`keyword-dot ${tone}`} aria-hidden="true" />
                    <WordListInput id={key} words={s[key]} onChange={(words) => set({ [key]: words })} />
                  </span>
                </Row>
              ))}
              <p className="settings-note">Up to 50 words per color. Words match whole, also next to a hyphen.</p>
            </Card>
            <TerminalPreview s={s} />
          </>
        );
      case "connections":
        return (
          <Card>
            <Row id="reconnect" label="Reconnect automatically" hint="When an SSH connection drops, try again with a countdown.">
              <Toggle id="reconnect" checked={s.autoReconnect} onChange={(autoReconnect) => set({ autoReconnect })} />
            </Row>
            <Row
              id="embedded-rdp"
              label="Show RDP and VNC inside Termalith"
              hint={
                <>
                  <span className="badge-soft">Experimental</span> Otherwise the installed remote desktop client opens.
                </>
              }
            >
              <Toggle id="embedded-rdp" checked={s.embeddedRdp} onChange={(embeddedRdp) => set({ embeddedRdp })} />
            </Row>
            <Row
              id="monitor"
              label="Server monitor under SSH terminals"
              hint="CPU, memory, load and network of Linux, macOS and FreeBSD servers; runs a read-only command every 3 seconds."
            >
              <Toggle id="monitor" checked={s.serverMonitor} onChange={(serverMonitor) => set({ serverMonitor })} />
            </Row>
          </Card>
        );
      case "proxy":
        return (
          <>
            <Card>
              <Row id="proxy" label="Proxy" hint="SSH and Telnet connections go through it (the first hop; jump hosts follow).">
                <Segmented
                  id="proxy"
                  label="Proxy"
                  value={s.proxy?.kind ?? "none"}
                  options={[
                    ["none", "Direct"],
                    ["socks5", "SOCKS5"],
                    ["http", "HTTP"],
                  ]}
                  onChange={(kind) =>
                    set({
                      proxy:
                        kind === "none"
                          ? null
                          : {
                              kind,
                              host: s.proxy?.host ?? "",
                              port: s.proxy?.port ?? (kind === "socks5" ? 1080 : 3128),
                              username: s.proxy?.username ?? null,
                            },
                    })
                  }
                />
              </Row>
              {s.proxy && (
                <>
                  <Row id="proxy-host" label="Host and port">
                    <span className="row">
                      <input
                        id="set-proxy-host"
                        value={s.proxy.host}
                        placeholder="proxy.example.com"
                        spellCheck={false}
                        aria-label="Proxy host"
                        onChange={(e) => s.proxy && set({ proxy: { ...s.proxy, host: e.target.value } })}
                      />
                      <input
                        className="port"
                        value={s.proxy.port}
                        inputMode="numeric"
                        aria-label="Proxy port"
                        onChange={(e) => s.proxy && set({ proxy: { ...s.proxy, port: Number(e.target.value) } })}
                      />
                    </span>
                  </Row>
                  <Row id="proxy-user" label="User name" hint="Optional.">
                    <input
                      id="set-proxy-user"
                      value={s.proxy.username ?? ""}
                      autoComplete="off"
                      spellCheck={false}
                      onChange={(e) => s.proxy && set({ proxy: { ...s.proxy, username: e.target.value || null } })}
                    />
                  </Row>
                  <Row id="proxy-password" label="Password" hint="Kept in the system keychain.">
                    <input
                      id="set-proxy-password"
                      type="password"
                      value={proxyPassword}
                      autoComplete="new-password"
                      disabled={!s.proxy.username?.trim() || forgetProxyPassword}
                      placeholder={hasProxyPassword ? "Saved — type to replace" : ""}
                      onChange={(e) => setProxyPasswordInput(e.target.value)}
                    />
                  </Row>
                  {hasProxyPassword && s.proxy.username?.trim() && (
                    <Row id="proxy-forget" label="Forget the saved proxy password">
                      <Toggle id="proxy-forget" checked={forgetProxyPassword} onChange={setForgetProxyPassword} />
                    </Row>
                  )}
                </>
              )}
            </Card>
            {s.proxy && (
              <p className="settings-note">
                The proxy sees which server you connect to, not the encrypted SSH traffic. With HTTP proxies the password
                is sent in the clear (Basic), so use one you trust.
              </p>
            )}
          </>
        );
      case "security":
        return (
          <>
            <MasterPasswordSettings secretsAvailable={secretsAvailable} onReset={onSecretsReset} portable={portable} />
            {!secretsAvailable && (
              <p className="settings-note">This computer's password store is not available, so passwords are not saved.</p>
            )}
            <Card>
              <Row
                id="auto-lock"
                label="Lock saved passwords after"
                hint="Only with a master password: after this long without keyboard or mouse use, saved passwords lock until you enter it again. Open connections stay open."
              >
                <select id="set-auto-lock" value={s.autoLockMinutes} onChange={(e) => set({ autoLockMinutes: Number(e.target.value) })}>
                  {AUTO_LOCK_CHOICES.map(([minutes, label]) => (
                    <option key={minutes} value={minutes}>
                      {label}
                    </option>
                  ))}
                </select>
              </Row>
            </Card>
          </>
        );
      case "shared":
        return <SharedSessionsSettings onChanged={() => onSecretsReset?.()} />;
      case "updates":
        return (
          <Card>
            <Row
              id="update-check"
              label="Tell me when a new version is available"
              hint="Checks GitHub at startup. Nothing is downloaded automatically."
            >
              <Toggle id="update-check" checked={s.updateCheck} onChange={(updateCheck) => set({ updateCheck })} />
            </Row>
          </Card>
        );
      case "shortcuts":
        return (
          <Card>
            <table className="shortcut-table" id="setting-shortcuts">
              <tbody>
                {shortcutHelp(mac).map((h) => (
                  <tr key={h.action}>
                    <td>{h.label}</td>
                    <td>
                      {keyCaps(h.keys, mac).map((c, i) => (
                        <kbd key={i}>{c}</kbd>
                      ))}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <p className="settings-note">
              {mac
                ? "⌘C and ⌘V copy and paste."
                : "Plain Ctrl+C, Ctrl+V and Ctrl+W go to the program in the terminal, so Termalith uses Ctrl+Shift."}
            </p>
          </Card>
        );
    }
  })();

  return (
    <Dialog
      title="Settings"
      className="settings-dialog"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" className="settings-reset" onClick={resetDefaults} title="Back to the original settings (not saved until you press Save)">
            <RotateCcw size={14} aria-hidden="true" />
            Reset to defaults
          </button>
          {error ? (
            <span className="form-error settings-error" role="alert">
              {error}
            </span>
          ) : (
            dirty && <span className="settings-dirty">Unsaved changes</span>
          )}
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="settings-form" className="primary" disabled={busy}>
            Save
          </button>
        </>
      }
    >
      <div className="settings-layout">
        <nav className="settings-nav" aria-label="Settings sections">
          <div className="settings-search">
            <Search size={14} aria-hidden="true" />
            <input
              type="search"
              value={query}
              placeholder="Search settings"
              aria-label="Search settings"
              spellCheck={false}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && results[0]) {
                  e.preventDefault();
                  go(results[0].section, results[0].id);
                }
                if (e.key === "Escape" && query) {
                  e.preventDefault();
                  e.stopPropagation();
                  setQuery("");
                }
              }}
            />
            {query && (
              <button type="button" className="icon" aria-label="Clear search" onClick={() => setQuery("")}>
                <X size={12} aria-hidden="true" />
              </button>
            )}
          </div>
          <ul>
            {SECTIONS.map(({ id, label, icon: Glyph }) => (
              <li key={id}>
                <button
                  type="button"
                  className={!query && id === section ? "active" : undefined}
                  aria-current={!query && id === section ? "page" : undefined}
                  onClick={() => go(id)}
                >
                  <Glyph size={16} strokeWidth={1.8} aria-hidden="true" />
                  {label}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <form id="settings-form" className="settings-content" onSubmit={submit} noValidate>
          {query ? (
            <>
              <header className="settings-head">
                <h3>Search results</h3>
                <p>{results.length === 0 ? `Nothing found for “${query}”.` : `${results.length} found. Enter opens the first.`}</p>
              </header>
              <ul className="settings-results">
                {results.map((r) => (
                  <li key={r.id}>
                    <button type="button" onClick={() => go(r.section, r.id)}>
                      <span className="result-label">{r.label}</span>
                      <span className="result-section">{SECTIONS.find((x) => x.id === r.section)?.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <>
              <header className="settings-head">
                <h3>{current.label}</h3>
                <p>{current.blurb}</p>
              </header>
              {body}
            </>
          )}
        </form>
      </div>
    </Dialog>
  );
}
