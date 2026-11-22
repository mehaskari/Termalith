import { useEffect, useMemo, useState } from "react";
import { useTheme } from "../app/useTheme";
import { TerminalView } from "../terminal/TerminalView";
import { DEFAULT_SETTINGS, type Settings, type TerminalId, getSettings, movedTerminal } from "../terminal/api";
import { isMacPlatform, matchShortcut } from "./shortcuts";

/** The terminal id a moved terminal's window shows (`?terminal=<id>`), if this is one. */
export function movedTerminalId(search: string): TerminalId | null {
  const raw = new URLSearchParams(search).get("terminal");
  if (raw === null || !/^\d+$/.test(raw)) return null;
  const id = Number(raw);
  return Number.isSafeInteger(id) ? id : null;
}

/**
 * The page of a terminal moved into its own window (P8-10, ADR-042): only
 * the terminal. Closing the window closes the terminal.
 */
export function MovedTerminal({ id }: { id: TerminalId }) {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [ended, setEnded] = useState(false);
  const [command, setCommand] = useState<{ seq: number; action: "find" | "copy" | "paste" | "save" } | undefined>();
  const theme = useTheme(settings?.theme ?? DEFAULT_SETTINGS.theme);
  const open = useMemo(() => movedTerminal(id), [id]);
  const label = document.title.replace(/ — Termalith$/, "") || "terminal";

  useEffect(() => {
    getSettings().then(setSettings, () => setSettings(DEFAULT_SETTINGS));
  }, []);

  // Find, copy and paste work as in the main window.
  useEffect(() => {
    const mac = isMacPlatform();
    const onKey = (e: KeyboardEvent) => {
      const action = matchShortcut(e, mac);
      if (action !== "find" && action !== "copy" && action !== "paste") return;
      e.preventDefault();
      e.stopPropagation();
      setCommand({ seq: performance.now(), action });
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, []);

  // Settings decide the first look; wait for them.
  if (!settings) return null;
  return (
    <main className="moved-terminal">
      {ended && (
        <div className="pane-toolbar">
          <span className="pane-warning" role="status">
            The session ended. Close this window when you are done.
          </span>
          <span />
        </div>
      )}
      {notice && (
        <div className="notice" role="status">
          {notice}
          <button type="button" className="icon" aria-label="Dismiss" onClick={() => setNotice(null)}>
            ×
          </button>
        </div>
      )}
      <div className="pane-terminal">
        <TerminalView
          open={open}
          active
          settings={settings}
          theme={theme}
          command={command}
          label={label}
          onNotice={setNotice}
          onExited={() => setEnded(true)}
          onFailed={() => setEnded(true)}
        />
      </div>
    </main>
  );
}
