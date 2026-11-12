import { type FormEvent, useEffect, useState } from "react";
import {
  type DroppedFile,
  type ServerKind,
  type ServerState,
  type SharedFolder,
  isIpcError,
  serverFolder,
  serverList,
  serverOpenFolder,
  serverPickFolder,
  serverResetFolder,
  serverStart,
  serverStop,
  serverUseDropped,
} from "../terminal/api";
import { formatSize } from "./paths";

const msg = (e: unknown) => (isIpcError(e) ? e.message : String(e));
const DEFAULT_PORT: Record<ServerKind, number> = { tftp: 6969, http: 8080, ftp: 2121 };
const LABEL: Record<ServerKind, string> = { tftp: "TFTP", http: "HTTP", ftp: "FTP" };
const DEFAULT_USER = "termalith";
const PASSWORD_MIN = 6;
const copy = (text: string) => void navigator.clipboard.writeText(text).catch(() => {});

/**
 * Built-in TFTP, HTTP and FTP servers. The folder to share is kept by the
 * backend: the default "Termalith Share" folder, one picked in the system
 * dialog, or one dropped onto the window. What is shown is what is shared.
 */
export function ServersTool({ dropped }: { dropped: DroppedFile | null }) {
  const [kind, setKind] = useState<ServerKind>("tftp");
  const [port, setPort] = useState(String(DEFAULT_PORT.tftp));
  const [lan, setLan] = useState(false);
  const [uploads, setUploads] = useState(false);
  const [folder, setFolder] = useState<SharedFolder | null>(null);
  const [httpLogin, setHttpLogin] = useState(false);
  const [user, setUser] = useState(DEFAULT_USER);
  const [password, setPassword] = useState("");
  const [showPassword, setShowPassword] = useState(false);
  const [busy, setBusy] = useState(false);
  const [servers, setServers] = useState<ServerState[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    serverFolder().then(setFolder, (e) => setError(msg(e)));
  }, []);
  // A folder dropped onto the window becomes the one to share.
  useEffect(() => {
    if (dropped?.folder) serverUseDropped(dropped.token).then(setFolder, (e) => setError(msg(e)));
  }, [dropped]);
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

  const portNumber = Number(port);
  const portOk = Number.isInteger(portNumber) && portNumber > 0 && portNumber < 65536;
  // FTP always asks for a login; HTTP when turned on; TFTP cannot.
  const login = kind === "ftp" || (kind === "http" && httpLogin);
  const userOk = user === "" || /^[A-Za-z0-9._-]{1,64}$/.test(user);
  const passwordOk = password === "" || (password.length >= PASSWORD_MIN && password.length <= 64);
  const canStart = folder !== null && portOk && (!login || (userOk && passwordOk)) && !busy;

  async function chooseFolder() {
    setError(null);
    try {
      setFolder(await serverPickFolder());
    } catch (e) {
      setError(msg(e));
    }
  }

  async function start(e: FormEvent) {
    e.preventDefault();
    if (!canStart) return;
    setError(null);
    setBusy(true);
    try {
      await serverStart({
        kind,
        lan,
        port: portNumber,
        allowUploads: uploads,
        requireLogin: login,
        user: login ? user : null,
        password: login ? password : null,
      });
      // The password now shows with the running server; do not keep it here.
      setPassword("");
      setServers(await serverList());
    } catch (err) {
      setError(msg(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="tool-form servers">
      <form className="tool-form" onSubmit={start} aria-label="New file server">
        <h3>New server</h3>
        <div className="server-fields">
          <label>
            Protocol
            <select
              value={kind}
              onChange={(e) => {
                const k = e.target.value as ServerKind;
                setKind(k);
                setPort(String(DEFAULT_PORT[k]));
              }}
            >
              <option value="tftp">TFTP</option>
              <option value="http">HTTP</option>
              <option value="ftp">FTP</option>
            </select>
          </label>
          <label>
            Port
            <input
              className="port"
              value={port}
              inputMode="numeric"
              aria-invalid={!portOk}
              onChange={(e) => setPort(e.target.value.replace(/[^0-9]/g, ""))}
            />
          </label>
        </div>

        <fieldset className="server-folder">
          <legend>Shared folder</legend>
          <div className="row">
            <code className="server-path" title={folder?.path}>{folder?.path ?? "…"}</code>
            {folder?.isDefault && <span className="hint">(default)</span>}
          </div>
          <div className="row">
            <button type="button" onClick={() => void chooseFolder()}>Choose folder…</button>
            {folder && !folder.isDefault && (
              <button type="button" onClick={() => void serverResetFolder().then(setFolder, (e) => setError(msg(e)))}>
                Use default folder
              </button>
            )}
            <button type="button" className="link" onClick={() => void serverOpenFolder(null).catch((e) => setError(msg(e)))}>
              Open folder
            </button>
          </div>
          <p className="hint">You can also drop a folder onto this window.</p>
        </fieldset>

        <fieldset className="server-login">
          <legend>Login</legend>
          {kind === "tftp" && (
            <p className="hint">
              TFTP has no user name or password: the protocol does not support them. Anyone who can reach this
              port can read the shared files (and upload new ones, if allowed), so keep "Allow other computers" off
              unless you need it.
            </p>
          )}
          {kind === "http" && (
            <label className="checkbox">
              <input type="checkbox" checked={httpLogin} onChange={(e) => setHttpLogin(e.target.checked)} />
              Require a user name and password
            </label>
          )}
          {kind === "ftp" && <p className="hint">FTP always asks for a user name and password.</p>}
          {login && (
            <>
              <div className="server-fields">
                <label>
                  User name
                  <input
                    value={user}
                    autoComplete="off"
                    spellCheck={false}
                    aria-invalid={!userOk}
                    placeholder={DEFAULT_USER}
                    onChange={(e) => setUser(e.target.value.trim())}
                  />
                </label>
                <label>
                  Password
                  <input
                    type={showPassword ? "text" : "password"}
                    value={password}
                    autoComplete="new-password"
                    spellCheck={false}
                    aria-invalid={!passwordOk}
                    placeholder="random"
                    onChange={(e) => setPassword(e.target.value)}
                  />
                </label>
                <label className="checkbox">
                  <input type="checkbox" checked={showPassword} onChange={(e) => setShowPassword(e.target.checked)} />
                  Show
                </label>
              </div>
              <p className="hint">
                {userOk
                  ? `Leave the password empty for a random one. A chosen password needs at least ${PASSWORD_MIN} characters.`
                  : "The user name may only use letters, digits, '.', '_' and '-'."}
              </p>
            </>
          )}
          {kind === "http" && !httpLogin && (
            <p className="hint">Off: anyone who can reach this port can read the shared files.</p>
          )}
        </fieldset>

        <label className="checkbox">
          <input type="checkbox" checked={lan} onChange={(e) => setLan(e.target.checked)} />
          Allow other computers (otherwise only this computer can connect)
        </label>
        <label className="checkbox">
          <input type="checkbox" checked={uploads} onChange={(e) => setUploads(e.target.checked)} />
          Allow uploads (new files only; existing files are never replaced)
        </label>
        {portOk && portNumber < 1024 && (
          <p className="hint">Ports below 1024 need administrator rights on macOS and Linux.</p>
        )}
        {error && <p className="files-error" role="alert">{error}</p>}
        <div className="row">
          <button type="submit" className="primary" disabled={!canStart}>
            Start {LABEL[kind]} server
          </button>
        </div>
      </form>

      <h3>Running servers</h3>
      {servers.length === 0 ? (
        <p className="hint">No server is running.</p>
      ) : (
        servers.map((s) => (
          <section key={s.id} className="server" aria-label={`${LABEL[s.kind]} server on port ${s.port}`}>
            <div className="row">
              <strong>{LABEL[s.kind]} · port {s.port}</strong>
              <span className="hint">
                {s.lan ? "other computers allowed" : "this computer only"} · {s.allowUploads ? "uploads on" : "read-only"}
              </span>
              <button type="button" onClick={() => void serverStop(s.id).then(() => serverList().then(setServers))}>
                Stop
              </button>
            </div>
            <dl className="server-details">
              <dt>Shared folder</dt>
              <dd>
                <code title={s.folder}>{s.folder}</code>{" "}
                <button type="button" className="link" onClick={() => void serverOpenFolder(s.id).catch((e) => setError(msg(e)))}>
                  Open
                </button>
              </dd>
              {!s.credentials && (
                <>
                  <dt>Login</dt>
                  <dd>none (anyone who can connect)</dd>
                </>
              )}
              {s.credentials && (
                <>
                  <dt>User name</dt>
                  <dd>
                    <code>{s.credentials.user}</code>{" "}
                    <button type="button" className="link" onClick={() => copy(s.credentials?.user ?? "")}>Copy</button>
                  </dd>
                  <dt>Password</dt>
                  <dd>
                    <code>{s.credentials.password}</code>{" "}
                    <button type="button" className="link" onClick={() => copy(s.credentials?.password ?? "")}>Copy</button>
                  </dd>
                </>
              )}
              <dt>Addresses</dt>
              <dd>
                <ul className="server-urls">
                  {s.urls.map((u) => (
                    <li key={u}>
                      <code>{u}</code>{" "}
                      <button type="button" className="link" onClick={() => copy(u)}>Copy</button>
                    </li>
                  ))}
                </ul>
              </dd>
            </dl>
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
