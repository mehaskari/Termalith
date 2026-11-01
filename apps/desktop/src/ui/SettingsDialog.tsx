import { type FormEvent, useState } from "react";
import { type Settings, type ShellChoice, isIpcError } from "../terminal/api";
import { Dialog } from "./Dialog";
import { TERMINAL_SCHEMES } from "./theme";

interface Props {
  initial: Settings;
  /** Installed shells for the default-shell choice. */
  shells: ShellChoice[];
  onSave: (s: Settings) => Promise<void>;
  onCancel: () => void;
}

export function SettingsDialog({ initial, shells, onSave, onCancel }: Props) {
  const [s, setS] = useState<Settings>(initial);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<Settings>) => setS((x) => ({ ...x, ...patch }));

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!Number.isInteger(s.fontSize) || s.fontSize < 8 || s.fontSize > 32) return setError("Font size must be 8-32");
    if (!Number.isInteger(s.scrollback) || s.scrollback < 100 || s.scrollback > 100000)
      return setError("Scrollback must be 100-100000 lines");
    if (s.proxy && (!s.proxy.host.trim() || !Number.isInteger(s.proxy.port) || s.proxy.port < 1 || s.proxy.port > 65535))
      return setError("Enter the proxy host and a port (1-65535)");
    try {
      await onSave(s);
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    }
  }

  return (
    <Dialog
      title="Settings"
      onCancel={onCancel}
      actions={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="submit" form="settings-form" className="primary">
            Save
          </button>
        </>
      }
    >
      <form id="settings-form" className="form-grid" onSubmit={submit}>
        <label className="span-2">
          Terminal colors
          <select value={s.terminalScheme} onChange={(e) => set({ terminalScheme: e.target.value })}>
            {TERMINAL_SCHEMES.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Theme
          <select value={s.theme} onChange={(e) => set({ theme: e.target.value as Settings["theme"] })}>
            <option value="system">Match system</option>
            <option value="dark">Dark</option>
            <option value="light">Light</option>
          </select>
        </label>
        <label>
          Font size
          <input type="number" min={8} max={32} value={s.fontSize} onChange={(e) => set({ fontSize: Number(e.target.value) })} />
        </label>
        <label className="span-2">
          Terminal font
          <input
            value={s.fontFamily}
            placeholder="Default (Menlo / Consolas / DejaVu Sans Mono)"
            onChange={(e) => set({ fontFamily: e.target.value })}
          />
        </label>
        <label>
          Cursor
          <select value={s.cursorStyle} onChange={(e) => set({ cursorStyle: e.target.value as Settings["cursorStyle"] })}>
            <option value="block">Block</option>
            <option value="bar">Bar</option>
            <option value="underline">Underline</option>
          </select>
        </label>
        <label>
          Scrollback (lines)
          <input type="number" min={100} max={100000} value={s.scrollback} onChange={(e) => set({ scrollback: Number(e.target.value) })} />
        </label>
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.cursorBlink} onChange={(e) => set({ cursorBlink: e.target.checked })} />
          Blinking cursor
        </label>
        <label>
          Proxy
          <select
            value={s.proxy?.kind ?? ""}
            onChange={(e) => {
              const kind = e.target.value as "" | "socks5" | "http";
              set({
                proxy: kind
                  ? { kind, host: s.proxy?.host ?? "", port: s.proxy?.port ?? (kind === "socks5" ? 1080 : 3128) }
                  : null,
              });
            }}
          >
            <option value="">Direct connection</option>
            <option value="socks5">SOCKS5</option>
            <option value="http">HTTP (CONNECT)</option>
          </select>
        </label>
        {s.proxy ? (
          <label>
            Proxy host and port
            <span className="row">
              <input
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
          </label>
        ) : (
          <span />
        )}
        {s.proxy && (
          <p className="hint span-2">
            SSH and Telnet connections go through the proxy (the first hop only; jump hosts are reached through the
            first connection). The proxy sees which server you connect to, not the encrypted SSH traffic.
          </p>
        )}
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.copyOnSelect} onChange={(e) => set({ copyOnSelect: e.target.checked })} />
          Copy selected terminal text automatically
        </label>
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.rightClickPaste} onChange={(e) => set({ rightClickPaste: e.target.checked })} />
          Right-click in a terminal pastes
        </label>
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.autoReconnect} onChange={(e) => set({ autoReconnect: e.target.checked })} />
          Reconnect SSH tabs automatically when the connection drops
        </label>
        {shells.length > 1 && (
          <label className="span-2">
            Shell for new local terminals
            <select
              value={s.defaultShell ?? ""}
              onChange={(e) => set({ defaultShell: e.target.value || null })}
            >
              <option value="">System default ({shells[0]?.name})</option>
              {shells.map((sh) => (
                <option key={sh.id} value={sh.id}>
                  {sh.name}
                </option>
              ))}
            </select>
          </label>
        )}
        <label className="checkbox span-2">
          <input type="checkbox" checked={s.logSessions} onChange={(e) => set({ logSessions: e.target.checked })} />
          Log every new terminal to Documents/Termalith Logs (plain text; may contain sensitive output)
        </label>
        {error && <p className="form-error span-2" role="alert">{error}</p>}
      </form>
    </Dialog>
  );
}
