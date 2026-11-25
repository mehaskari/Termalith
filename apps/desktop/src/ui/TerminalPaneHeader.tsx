import { Braces, CircleDot, Columns2, FolderTree, Grid2x2, Megaphone, RotateCw, Rows2, ScrollText, Square, Waypoints } from "lucide-react";
import type { Tab } from "../app/tabs";
import { type BroadcastState, resolveTargets } from "./broadcast";

/** Which side panel of a terminal tab is open (at most one). */
export type SidePanel = "snippets" | "tunnels" | "files";

/** The side panel flags after toggling `panel`: it opens alone, or closes. */
export function togglePanel(t: Pick<Tab, "snippetsOpen" | "tunnelsOpen" | "filesOpen">, panel: SidePanel) {
  const open = { snippets: !!t.snippetsOpen, tunnels: !!t.tunnelsOpen, files: !!t.filesOpen }[panel];
  return {
    snippetsOpen: panel === "snippets" && !open,
    tunnelsOpen: panel === "tunnels" && !open,
    filesOpen: panel === "files" && !open,
  };
}

/** The live and waiting targets of a broadcast, as the banner says them. */
export function broadcastBanner(broadcast: BroadcastState, tabs: Tab[]): string {
  const { live, skipped } = resolveTargets(broadcast, tabs);
  const names = live.map((x) => tabs.find((y) => y.key === x.key)?.title).join(", ");
  const waiting = skipped.length > 0 ? ` · waiting (not connected): ${skipped.map((x) => x.title).join(", ")}` : "";
  return `BROADCASTING to ${names || "no live terminals"}${waiting}`;
}

/**
 * The toolbar and banners above a terminal tab: reconnect countdown or a
 * changed host key, split, broadcast, log, macro and the side panels.
 */
export function TerminalPaneHeader(props: {
  tab: Tab;
  tabs: Tab[];
  /** The clock the reconnect countdown is drawn with. */
  now: number;
  broadcast: BroadcastState | null;
  /** The tab that records a macro, if any. */
  recordingKey: number | null;
  onCancelReconnect: () => void;
  onForgetKey: () => void;
  onSplit: (direction: "row" | "column" | "grid") => void;
  onBroadcast: () => void;
  onStopBroadcast: () => void;
  onToggleTiled: () => void;
  onToggleLog: () => void;
  onToggleMacro: () => void;
  onTogglePanel: (panel: SidePanel) => void;
  onReconnect: () => void;
}) {
  const { tab: t, tabs, now, broadcast, recordingKey } = props;
  const idle = t.terminalId === undefined || t.ended;
  const noSplit = idle || t.kind === "serial" || t.kind === "telnet";
  const recording = recordingKey === t.key;
  return (
    <>
      <div className="pane-toolbar">
        {t.reconnectAt !== undefined ? (
          <span className="pane-warning" role="status">
            Connection lost. Reconnecting in {Math.max(0, Math.ceil((t.reconnectAt - now) / 1000))} s…
            <button type="button" onClick={props.onCancelReconnect}>
              Cancel
            </button>
          </span>
        ) : t.changedKey ? (
          <span className="pane-warning">
            Host key changed for {t.changedKey.host}.
            <button type="button" className="danger" onClick={props.onForgetKey}>
              Remove recorded key…
            </button>
          </span>
        ) : (
          <span />
        )}
        <span className="pane-actions">
          <button
            type="button"
            title="Split side by side"
            aria-label="Split side by side"
            disabled={noSplit}
            onClick={() => props.onSplit("row")}
          >
            <Columns2 size={15} aria-hidden="true" />
          </button>
          <button
            type="button"
            title="Split top and bottom"
            aria-label="Split top and bottom"
            disabled={noSplit}
            onClick={() => props.onSplit("column")}
          >
            <Rows2 size={15} aria-hidden="true" />
          </button>
          <button
            type="button"
            title="Four panes"
            aria-label="Four panes"
            disabled={noSplit}
            onClick={() => props.onSplit("grid")}
          >
            <Grid2x2 size={15} aria-hidden="true" />
          </button>
          <span className="toolbar-sep" aria-hidden="true" />
          {broadcast?.source === t.key ? (
            <button type="button" className="danger" onClick={props.onStopBroadcast}>
              <Square size={13} aria-hidden="true" />
              Stop broadcast
            </button>
          ) : (
            <button
              type="button"
              title="Type into several terminals at once"
              disabled={idle || broadcast !== null}
              onClick={props.onBroadcast}
            >
              <Megaphone size={15} aria-hidden="true" />
              Broadcast…
            </button>
          )}
          <button
            type="button"
            aria-pressed={!!t.log}
            className={t.log ? "recording" : undefined}
            title={t.log ? `Logging to ${t.log.path}. Click to stop.` : "Save this terminal's output to a text file"}
            disabled={idle}
            onClick={props.onToggleLog}
          >
            <ScrollText size={15} aria-hidden="true" />
            Log
          </button>
          <button
            type="button"
            aria-pressed={recording}
            className={recording ? "recording" : undefined}
            title={recording ? "Stop recording" : "Record what you type as a macro"}
            disabled={idle || (recordingKey !== null && !recording)}
            onClick={props.onToggleMacro}
          >
            <CircleDot size={15} aria-hidden="true" />
            Macro
          </button>
          {t.reconnect && t.ended && (
            <button type="button" onClick={props.onReconnect}>
              <RotateCw size={15} aria-hidden="true" />
              Reconnect
            </button>
          )}
          <span className="toolbar-sep" aria-hidden="true" />
          <span className="segmented" role="group" aria-label="Side panel">
          <button
            type="button"
            aria-pressed={!!t.snippetsOpen}
            disabled={idle}
            onClick={() => props.onTogglePanel("snippets")}
          >
            <Braces size={15} aria-hidden="true" />
            Snippets
          </button>
          {t.kind === "ssh" && (
            <>
              <button
                type="button"
                aria-pressed={!!t.tunnelsOpen}
                disabled={idle}
                onClick={() => props.onTogglePanel("tunnels")}
              >
                <Waypoints size={15} aria-hidden="true" />
                Tunnels
              </button>
              <button
                type="button"
                aria-pressed={!!t.filesOpen}
                disabled={idle}
                onClick={() => props.onTogglePanel("files")}
              >
                <FolderTree size={15} aria-hidden="true" />
                Files
              </button>
            </>
          )}
          </span>
        </span>
      </div>
      {broadcast?.source === t.key && (
        <div className="broadcast-banner" role="status">
          {broadcastBanner(broadcast, tabs)}
          <button type="button" className="banner-action" onClick={props.onToggleTiled}>
            {broadcast.tiled ? "Show as tabs" : "Show side by side"}
          </button>
        </div>
      )}
      {broadcast?.targets.includes(t.key) && broadcast.source !== t.key && (
        <div className="broadcast-banner receiving" role="status">
          Receiving input broadcast from {tabs.find((y) => y.key === broadcast.source)?.title}
        </div>
      )}
      {recording && (
        <div className="broadcast-banner" role="status">
          RECORDING: everything you type here is captured, including passwords.
        </div>
      )}
    </>
  );
}
