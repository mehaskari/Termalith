import { type FormEvent, useCallback, useEffect, useState } from "react";
import {
  type TerminalId,
  type TunnelInfo,
  type TunnelSpec,
  isIpcError,
  listTunnels,
  startTunnel,
  stopTunnel,
} from "../terminal/api";

interface Props {
  terminalId: TerminalId;
  onClose: () => void;
}

const REFRESH_MS = 2000;

export function TunnelsPanel({ terminalId, onClose }: Props) {
  const [tunnels, setTunnels] = useState<TunnelInfo[]>([]);
  const [kind, setKind] = useState<"local" | "socks">("local");
  const [localPort, setLocalPort] = useState("");
  const [remoteHost, setRemoteHost] = useState("localhost");
  const [remotePort, setRemotePort] = useState("");
  const [expose, setExpose] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(() => {
    listTunnels(terminalId).then(setTunnels, () => {});
  }, [terminalId]);

  useEffect(() => {
    refresh();
    const t = setInterval(refresh, REFRESH_MS);
    return () => clearInterval(t);
  }, [refresh]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const lp = localPort.trim() === "" ? 0 : Number(localPort);
    if (!Number.isInteger(lp) || lp < 0 || lp > 65535) return setError("Local port must be 0-65535 (0 = any free port)");
    let spec: TunnelSpec;
    if (kind === "local") {
      const rp = Number(remotePort);
      if (!remoteHost.trim()) return setError("Remote host is required");
      if (!Number.isInteger(rp) || rp < 1 || rp > 65535) return setError("Remote port must be 1-65535");
      spec = { kind: "local", remoteHost: remoteHost.trim(), remotePort: rp, localPort: lp, allowRemoteClients: expose };
    } else {
      spec = { kind: "socks", localPort: lp, allowRemoteClients: expose };
    }
    setBusy(true);
    try {
      await startTunnel(terminalId, spec);
      setLocalPort("");
      setExpose(false);
      refresh();
    } catch (err) {
      setError(isIpcError(err) ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="files-panel tunnels-panel" aria-label="Tunnels">
      <header className="files-head">
        <strong className="panel-title">Tunnels</strong>
        <button type="button" className="icon" title="Close tunnels" aria-label="Close tunnels" onClick={onClose}>
          ×
        </button>
      </header>

      <ul className="files-list">
        {tunnels.length === 0 && <li className="files-empty">No tunnels on this connection.</li>}
        {tunnels.map((t) => (
          <li key={t.id} className="tunnel">
            <div className="transfer-line">
              <code className="file-name" title={t.localAddress}>
                {t.localAddress}
              </code>
              <button type="button" className="icon" aria-label={`Stop tunnel on ${t.localAddress}`} title="Stop"
                onClick={() => void stopTunnel(t.id).then(refresh)}>
                ×
              </button>
            </div>
            <span className="transfer-detail">
              {t.kind === "local" ? `→ ${t.remoteHost}:${t.remotePort} (via server)` : "SOCKS5 proxy"} ·{" "}
              {t.activeConnections} open / {t.totalConnections} total
            </span>
            {t.exposed && <span className="tunnel-exposed">Reachable from other machines</span>}
          </li>
        ))}
      </ul>

      <form className="tunnel-form" onSubmit={submit}>
        <label>
          Type
          <select value={kind} onChange={(e) => setKind(e.target.value as "local" | "socks")}>
            <option value="local">Local forward (-L)</option>
            <option value="socks">SOCKS proxy (-D)</option>
          </select>
        </label>
        <label>
          Local port
          <input value={localPort} inputMode="numeric" placeholder="auto" onChange={(e) => setLocalPort(e.target.value)} />
        </label>
        {kind === "local" && (
          <>
            <label>
              Remote host (from the server)
              <input value={remoteHost} spellCheck={false} onChange={(e) => setRemoteHost(e.target.value)} />
            </label>
            <label>
              Remote port
              <input value={remotePort} inputMode="numeric" onChange={(e) => setRemotePort(e.target.value)} />
            </label>
          </>
        )}
        <label className="checkbox">
          <input type="checkbox" checked={expose} onChange={(e) => setExpose(e.target.checked)} />
          Allow other machines to connect
        </label>
        {expose && (
          <p className="tunnel-warning">
            The port will listen on all network interfaces. Anyone who can reach this computer can use the tunnel.
          </p>
        )}
        {error && <p className="form-error" role="alert">{error}</p>}
        <button type="submit" className="primary" disabled={busy}>
          Start tunnel
        </button>
      </form>
    </section>
  );
}
