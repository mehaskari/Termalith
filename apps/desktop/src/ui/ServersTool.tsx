import { type FormEvent, useEffect, useState } from "react";
import {
  type DroppedFile,
  type ServerKind,
  type ServerState,
  isIpcError,
  serverDefaultFolder,
  serverList,
  serverOpenFolder,
  serverStart,
  serverStop,
} from "../terminal/api";
import { formatSize } from "./paths";

const msg = (e: unknown) => (isIpcError(e) ? e.message : String(e));
const DEFAULT_PORT: Record<ServerKind, number> = { tftp: 6969, http: 8080, ftp: 2121 };
const LABEL: Record<ServerKind, string> = { tftp: "TFTP", http: "HTTP", ftp: "FTP" };

/**
 * Built-in TFTP, HTTP and FTP servers sharing one folder: the default
 * "Termalith Share" folder, or a folder dropped onto the window.
 */
export function ServersTool({ dropped }: { dropped: DroppedFile | null }) {
  const [kind, setKind] = useState<ServerKind>("tftp");
  const [port, setPort] = useState(String(DEFAULT_PORT.tftp));
  const [lan, setLan] = useState(false);
  const [uploads, setUploads] = useState(false);
  const [useDropped, setUseDropped] = useState(false);
  const [defaultFolder, setDefaultFolder] = useState<string | null>(null);
  const [servers, setServers] = useState<ServerState[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    serverDefaultFolder().then(setDefaultFolder, (e) => setError(msg(e)));
  }, []);
  // A newly dropped folder becomes the one to share.
  useEffect(() => setUseDropped(dropped !== null), [dropped]);
  // The list (with recent activity) is refreshed while the tool is shown.
  useEffect(() => {
    let alive = true;
    const refresh = () =>
      serverList().then(
        (s) => alive && setServers(s),
        () => {},
      );
    void refresh();
    const timer = setInterval(refresh, 1000);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, []);

  async function start(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const p = Number(port);
    try {
      await serverStart({
        kind,
        folderToken: useDropped && dropped ? dropped.token : null,
        lan,
        port: Number.isInteger(p) && p > 0 && p < 65536 ? p : null,
        allowUploads: uploads,
      });
      // A dropped folder's token is used up; the next start shares the default.
      if (useDropped) setUseDropped(false);
      setServers(await serverList());
    } catch (err) {
      setError(msg(err));
    }
  }

  return (
    <div className="tool-form servers">
      <form className="tool-form" onSubmit={start}>
        <div className="row">
          <label>
            Protocol{" "}
            <select value={kind} onChange={(e) => {
              const k = e.target.value as ServerKind;
              setKind(k);
              setPort(String(DEFAULT_PORT[k]));
            }}>
              <option value="tftp">TFTP</option>
              <option value="http">HTTP</option>
              <option value="ftp">FTP</option>
            </select>
          </label>
          <label>
            Port <input className="port" value={port} inputMode="numeric" aria-label="Port"
              onChange={(e) => setPort(e.target.value.replace(/[^0-9]/g, ""))} />
          </label>
          <button type="submit" className="primary">Start</button>
        </div>
        <p className="hint">
          Shared folder:{" "}
          {useDropped && dropped ? (
            <>
              <strong>{dropped.name}</strong> (dropped){" "}
              <button type="button" className="link" onClick={() => setUseDropped(false)}>Use the default folder</button>
            </>
          ) : (
            <>
              <code>{defaultFolder ?? "…"}</code>{" "}
              <button type="button" className="link" onClick={() => void serverOpenFolder().catch((e) => setError(msg(e)))}>
                Open folder
              </button>
            </>
          )}
          <br />
          Drop a folder onto this window to share it instead.
        </p>
        <label className="checkbox">
          <input type="checkbox" checked={lan} onChange={(e) => setLan(e.target.checked)} />
          Allow other computers (otherwise only this computer can connect)
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={uploads} onChange={(e) => setUploads(e.target.checked)} />
          Allow uploads (new files only; existing files are never replaced)
        </label>
        {Number(port) < 1024 && Number(port) > 0 && (
          <p className="hint">Ports below 1024 need administrator rights on macOS and Linux.</p>
        )}
        {error && <p className="files-error" role="alert">{error}</p>}
      </form>

      {servers.length === 0 ? (
        <p className="hint">No server is running.</p>
      ) : (
        servers.map((s) => (
          <section key={s.id} className="server" aria-label={`${LABEL[s.kind]} server on port ${s.port}`}>
            <div className="row">
              <strong>{LABEL[s.kind]}</strong>
              <span className="hint">
                {s.lan ? "other computers allowed" : "this computer only"} · {s.allowUploads ? "uploads on" : "read-only"}
              </span>
              <button type="button" onClick={() => void serverStop(s.id).then(() => serverList().then(setServers))}>
                Stop
              </button>
            </div>
            <p className="hint"><code>{s.folder}</code></p>
            {s.credentials && (
              <p className="hint">
                User <code>{s.credentials.user}</code> · password <code>{s.credentials.password}</code>{" "}
                <button type="button" className="link"
                  onClick={() => void navigator.clipboard.writeText(s.credentials?.password ?? "").catch(() => {})}>
                  Copy password
                </button>
                <br />A new random password is made each time the server starts.
              </p>
            )}
            <ul className="server-urls">
              {s.urls.map((u) => (
                <li key={u}>
                  <code>{u}</code>{" "}
                  <button type="button" className="link" onClick={() => void navigator.clipboard.writeText(u).catch(() => {})}>
                    Copy
                  </button>
                </li>
              ))}
            </ul>
            {s.activity.length > 0 && (
              <table className="server-activity">
                <thead>
                  <tr><th>Time</th><th>Client</th><th>Action</th><th>Path</th><th>Size</th><th>Result</th></tr>
                </thead>
                <tbody>
                  {[...s.activity].reverse().slice(0, 50).map((a) => (
                    <tr key={a.seq} className={a.error ? "failed" : undefined}>
                      <td>{new Date(a.at * 1000).toLocaleTimeString()}</td>
                      <td>{a.peer}</td>
                      <td>{a.action}</td>
                      <td title={a.path}>{a.path}</td>
                      <td>{a.action === "list" || a.action === "login" ? "" : formatSize(a.bytes)}</td>
                      <td>{a.error ?? "ok"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        ))
      )}
    </div>
  );
}
