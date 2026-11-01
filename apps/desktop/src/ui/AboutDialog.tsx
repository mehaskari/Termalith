import icon from "../assets/icon.png";
import { type AppInfo, openWebLink } from "../terminal/api";
import { Dialog } from "./Dialog";

const REPO = "https://github.com/mehaskari/Termalith";
const SITE = "https://mehaskari.github.io/Termalith/";

/** Main open-source components (all MIT or MIT/Apache-2.0). */
const COMPONENTS: Array<[string, string]> = [
  ["Tauri", "MIT / Apache-2.0"],
  ["russh, russh-sftp", "Apache-2.0"],
  ["portable-pty", "MIT"],
  ["xterm.js", "MIT"],
  ["React", "MIT"],
  ["rusqlite / SQLite", "MIT / public domain"],
  ["keyring", "MIT / Apache-2.0"],
  ["serialport", "MPL-2.0"],
];

function Link({ href, children }: { href: string; children: string }) {
  return (
    <button type="button" className="link" onClick={() => void openWebLink(href).catch(() => {})}>
      {children}
    </button>
  );
}

/** Version, links and credits. */
export function AboutDialog({ info, onClose }: { info: AppInfo | null; onClose: () => void }) {
  return (
    <Dialog
      title="About Termalith"
      onCancel={onClose}
      actions={
        <button type="button" className="primary" onClick={onClose}>
          Close
        </button>
      }
    >
      <div className="about">
        <div className="about-head">
          <img src={icon} alt="" width={64} height={64} />
          <div>
            <h3>Termalith</h3>
            <p className="about-tagline">One workspace. Every connection.</p>
            <p className="hint">
              Version {info?.version ?? "?"} (preview){info?.platform ? ` · ${info.platform}` : ""}
            </p>
          </div>
        </div>
        <p>
          An open-source remote administration workstation: SSH, SFTP, RDP/VNC, tunnels, serial and Telnet
          consoles and network tools in one app.
        </p>
        <p className="about-links">
          <Link href={SITE}>Website &amp; docs</Link>
          <Link href={REPO}>Source code</Link>
          <Link href={`${REPO}/issues`}>Report a problem</Link>
          <Link href={`${REPO}/security`}>Security</Link>
        </p>
        <details>
          <summary>Open-source components</summary>
          <ul className="about-components">
            {COMPONENTS.map(([name, license]) => (
              <li key={name}>
                {name} <span className="hint">{license}</span>
              </li>
            ))}
          </ul>
          <p className="hint">The complete list of dependencies and their licenses is in the source repository.</p>
        </details>
        <p className="hint">Released under the MIT License. © 2026 mehaskari and contributors.</p>
      </div>
    </Dialog>
  );
}
