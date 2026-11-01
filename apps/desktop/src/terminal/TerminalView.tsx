import { useEffect, useRef, useState } from "react";
import { SearchAddon } from "@xterm/addon-search";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { DEFAULT_SETTINGS, type ExitPayload, type Settings } from "./api";
import { type ResolvedTheme, fontStack, terminalTheme } from "../ui/theme";
import {
  type Opener,
  type TerminalId,
  ackTerminal,
  closeTerminal,
  resizeTerminal,
  writeTerminal,
} from "./api";
import { AckBatcher, SerialQueue, chunkInput } from "./flow";
import { connectFailureLines, describeError, describeExit } from "./messages";


interface Props {
  /** Creates the backend terminal; called once per mount. */
  open: Opener;
  /** Visible tab: refit and focus when it becomes active. */
  active: boolean;
  onReady?: (id: TerminalId) => void;
  onFailed?: (error: unknown) => void;
  /** The session ended; `payload.exit` is null when no exit status came
   * (for SSH: the connection was lost rather than the shell exiting). */
  onExited?: (payload: ExitPayload) => void;
  /** Keyboard input typed into this terminal (used for broadcast). */
  onInput?: (data: string) => void;
  settings?: Settings;
  theme?: ResolvedTheme;
  /** One-shot command from a keyboard shortcut; `seq` changes per request. */
  command?: { seq: number; action: "find" | "copy" | "paste" };
}

export function TerminalView({
  open,
  active,
  onReady,
  onFailed,
  onExited,
  onInput,
  settings = DEFAULT_SETTINGS,
  theme = "dark",
  command,
}: Props) {
  const searchRef = useRef<SearchAddon | null>(null);
  const findInput = useRef<HTMLInputElement>(null);
  const [findOpen, setFindOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  // `open` must not re-run the effect; keep the first one.
  const openRef = useRef(open);
  const initial = useRef({ settings, theme });
  const callbacks = useRef({ onReady, onFailed, onExited, onInput });
  callbacks.current = { onReady, onFailed, onExited, onInput };

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const s = initial.current.settings;
    const term = new Terminal({
      cursorBlink: s.cursorBlink,
      cursorStyle: s.cursorStyle,
      fontFamily: fontStack(s.fontFamily),
      fontSize: s.fontSize,
      scrollback: s.scrollback,
      theme: terminalTheme(initial.current.settings.terminalScheme, initial.current.theme),
      // Needed by the search addon's match decorations.
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    term.loadAddon(fit);
    const search = new SearchAddon();
    term.loadAddon(search);
    searchRef.current = search;
    term.open(host);
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => webgl.dispose());
      term.loadAddon(webgl);
    } catch {
      // Falls back to xterm's DOM renderer.
    }
    fit.fit();
    termRef.current = term;
    fitRef.current = fit;

    let id: TerminalId | null = null;
    let disposed = false;
    let exited = false;
    let startFailed = false;
    const report = (what: string) => (e: unknown) => {
      if (!disposed) term.write(`\r\n\x1b[31m[${what}: ${describeError(e)}]\x1b[0m\r\n`);
    };
    const acks = new AckBatcher<TerminalId>((tid, bytes) => {
      ackTerminal(tid, bytes).catch(report("ack failed"));
    });
    const input = new SerialQueue(report("write failed"));

    // Start on the next tick: a mount that is torn down immediately (React
    // StrictMode in development) must not open a connection nobody owns.
    let startTimer = 0;
    const opened = new Promise<TerminalId>((resolve, reject) => {
      startTimer = window.setTimeout(() => {
        openRef
          .current(
            term.cols,
            term.rows,
            (data) => term.write(data, () => acks.add(data.length)),
            (payload) => {
              exited = true;
              term.write(`\r\n\x1b[2m[${describeExit(payload)}]\x1b[0m\r\n`);
              callbacks.current.onExited?.(payload);
            },
          )
          .then(resolve, reject);
      }, 0);
    });
    opened.then(
      (tid) => {
        if (disposed) {
          void closeTerminal(tid).catch(() => {});
          return;
        }
        id = tid;
        acks.setId(tid);
        callbacks.current.onReady?.(tid);
        // The window may have been resized while the shell was starting.
        if (term.cols && term.rows) void resizeTerminal(tid, term.cols, term.rows).catch(() => {});
      },
      (e) => {
        startFailed = true;
        if (disposed) return;
        callbacks.current.onFailed?.(e);
        const lines = connectFailureLines(e);
        term.write(`\r\n\x1b[31m${lines.join("\r\n")}\x1b[0m\r\n`);
      },
    );

    // Input typed (or generated by xterm, e.g. replies to the cursor
    // position query ConPTY sends at startup) before the id is known is
    // queued, not dropped: on Windows the shell produces no output until
    // that reply arrives.
    const onData = term.onData((data) => {
      if (exited || startFailed) return;
      for (const chunk of chunkInput(data)) {
        input.push(async () => writeTerminal(await opened, chunk));
      }
      callbacks.current.onInput?.(data);
    });

    let resizeFrame = 0;
    const onResize = term.onResize(({ cols, rows }) => {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = requestAnimationFrame(() => {
        if (id !== null && !exited) resizeTerminal(id, cols, rows).catch(report("resize failed"));
      });
    });
    // Hidden tabs have zero size; fitting them would shrink the PTY.
    const observer = new ResizeObserver(() => {
      if (host.offsetWidth > 0 && host.offsetHeight > 0) fit.fit();
    });
    observer.observe(host);

    return () => {
      disposed = true;
      clearTimeout(startTimer);
      observer.disconnect();
      cancelAnimationFrame(resizeFrame);
      onData.dispose();
      onResize.dispose();
      if (id !== null) void closeTerminal(id).catch(() => {});
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
      searchRef.current = null;
    };
  }, []);

  // Shortcut commands (find / copy / paste) for this terminal.
  useEffect(() => {
    if (!command) return;
    const term = termRef.current;
    if (!term) return;
    if (command.action === "find") {
      setFindOpen(true);
      requestAnimationFrame(() => findInput.current?.select());
    } else if (command.action === "copy") {
      const sel = term.getSelection();
      if (sel) void navigator.clipboard.writeText(sel).catch(() => {});
    } else {
      void navigator.clipboard.readText().then((t) => t && term.paste(t), () => {});
    }
    // Only react to new requests.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [command?.seq]);

  const find = (backwards: boolean) => {
    const s = searchRef.current;
    if (!s || !query) return;
    const opts = {
      caseSensitive,
      decorations: { matchOverviewRuler: "#e6b45c", activeMatchColorOverviewRuler: "#7fd1b9", matchBackground: "#5c4a1f", activeMatchBackground: "#1f5c4a" },
    };
    if (backwards) s.findPrevious(query, opts);
    else s.findNext(query, opts);
  };

  const closeFind = () => {
    setFindOpen(false);
    searchRef.current?.clearDecorations();
    termRef.current?.focus();
  };

  // Apply setting and theme changes to the live terminal.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = terminalTheme(settings.terminalScheme, theme);
    // Padding around the canvas uses the scheme's background too.
    if (hostRef.current) hostRef.current.style.background = term.options.theme.background ?? "";
    term.options.fontSize = settings.fontSize;
    term.options.fontFamily = fontStack(settings.fontFamily);
    term.options.cursorStyle = settings.cursorStyle;
    term.options.cursorBlink = settings.cursorBlink;
    term.options.scrollback = settings.scrollback;
    fitRef.current?.fit();
  }, [settings, theme]);

  // Optional MobaXterm-style clipboard: copy on select, paste on right-click.
  useEffect(() => {
    const term = termRef.current;
    if (!term || !settings.copyOnSelect) return;
    const d = term.onSelectionChange(() => {
      const sel = term.getSelection();
      if (sel) void navigator.clipboard.writeText(sel).catch(() => {});
    });
    return () => d.dispose();
  }, [settings.copyOnSelect]);

  useEffect(() => {
    const host = hostRef.current;
    if (!host || !settings.rightClickPaste) return;
    const onContext = (e: MouseEvent) => {
      e.preventDefault();
      const term = termRef.current;
      if (term) void navigator.clipboard.readText().then((t) => t && term.paste(t), () => {});
    };
    host.addEventListener("contextmenu", onContext);
    return () => host.removeEventListener("contextmenu", onContext);
  }, [settings.rightClickPaste]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(frame);
  }, [active]);

  return (
    <div className="terminal-wrap">
      <div ref={hostRef} className="terminal-host" />
      {findOpen && (
        <form
          className="find-bar"
          role="search"
          onSubmit={(e) => {
            e.preventDefault();
            find(false);
          }}
        >
          <input
            ref={findInput}
            autoFocus
            value={query}
            placeholder="Find"
            aria-label="Find in terminal"
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") closeFind();
              if (e.key === "Enter" && e.shiftKey) {
                e.preventDefault();
                find(true);
              }
            }}
          />
          <button type="button" className="icon" aria-pressed={caseSensitive} title="Match case" aria-label="Match case"
            onClick={() => setCaseSensitive((c) => !c)}>
            Aa
          </button>
          <button type="button" className="icon" title="Previous (Shift+Enter)" aria-label="Previous match" onClick={() => find(true)}>
            ↑
          </button>
          <button type="submit" className="icon" title="Next (Enter)" aria-label="Next match">
            ↓
          </button>
          <button type="button" className="icon" title="Close (Esc)" aria-label="Close find" onClick={closeFind}>
            ×
          </button>
        </form>
      )}
    </div>
  );
}
