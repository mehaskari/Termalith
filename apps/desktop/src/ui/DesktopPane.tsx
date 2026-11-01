import type { DesktopInfo } from "../terminal/api";

interface Props {
  info: DesktopInfo | null;
  /** Message when opening failed. */
  error: string | null;
  onReopen: () => void;
  onClose: () => void;
}

const PROTOCOL = { rdp: "RDP", vnc: "VNC" };

/** Status of a remote desktop running in an external client window. */
export function DesktopPane({ info, error, onReopen, onClose }: Props) {
  if (error) {
    return (
      <div className="desktop-pane">
        <div className="desktop-card">
          <h2>Could not open the remote desktop</h2>
          <p className="form-error" role="alert">
            {error}
          </p>
          <div className="desktop-actions">
            <button type="button" onClick={onReopen}>
              Try again
            </button>
          </div>
        </div>
      </div>
    );
  }
  if (!info) {
    return (
      <div className="desktop-pane">
        <div className="desktop-card">
          <p>Opening…</p>
        </div>
      </div>
    );
  }
  const status =
    info.kind === "running"
      ? `Running in ${info.client}`
      : info.kind === "handedOff"
        ? `Opened in ${info.client}`
        : `${info.client} closed${info.code !== null && info.code !== 0 ? ` (exit code ${info.code})` : ""}`;
  return (
    <div className="desktop-pane">
      <div className="desktop-card">
        <h2>
          <span className={`desktop-dot ${info.kind}`} aria-hidden="true" />
          {info.label}
        </h2>
        <dl className="desktop-facts">
          <dt>Status</dt>
          <dd role="status">{status}</dd>
          <dt>Protocol</dt>
          <dd>{PROTOCOL[info.protocol]}</dd>
          <dt>Connects to</dt>
          <dd>
            <code>{info.address}</code>
            {info.via && ` (SSH tunnel through ${info.via})`}
          </dd>
        </dl>
        {info.kind === "handedOff" && (
          <p className="hint">
            {info.client} manages this window itself, so Termalith cannot tell when you disconnect.
            {info.via && " Keep this tab open while you use it: closing it closes the tunnel."}
          </p>
        )}
        <div className="desktop-actions">
          {info.kind === "exited" && (
            <button type="button" onClick={onReopen}>
              Reconnect
            </button>
          )}
          <button type="button" className={info.kind === "exited" ? "" : "danger"} onClick={onClose}>
            {info.kind === "running" ? "Disconnect" : "Close"}
          </button>
        </div>
      </div>
    </div>
  );
}
