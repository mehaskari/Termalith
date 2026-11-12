import { type FormEvent, useEffect, useRef, useState } from "react";
import {
  type LanHost,
  cancelProbe,
  dnsLookup,
  isIpcError,
  localSubnet,
  startBandwidth,
  startLanScan,
  startProbe,
  startScan,
  tcpCheck,
  wakeOnLan,
} from "../terminal/api";
import type { DroppedFile } from "../terminal/api";
import { ServersTool } from "./ServersTool";
import { calcSubnet } from "./subnet";

type Tool = "ping" | "traceroute" | "tcp" | "scan" | "lan" | "bandwidth" | "dns" | "subnet" | "wol" | "servers";
const TOOLS: Array<[Tool, string]> = [
  ["ping", "Ping"],
  ["traceroute", "Traceroute"],
  ["tcp", "TCP port check"],
  ["scan", "Port scan"],
  ["lan", "LAN scan"],
  ["bandwidth", "Bandwidth (iperf3)"],
  ["dns", "DNS lookup"],
  ["subnet", "Subnet calculator"],
  ["wol", "Wake-on-LAN"],
  ["servers", "File servers (TFTP, HTTP, FTP)"],
];

const msg = (e: unknown) => (isIpcError(e) ? e.message : String(e));

/** `dropped`: a folder dropped onto the window while this tab is open (shared by the file servers). */
export function NetworkTools({ dropped = null }: { dropped?: DroppedFile | null }) {
  const [tool, setTool] = useState<Tool>("ping");
  // A dropped folder is meant for the file servers.
  useEffect(() => {
    if (dropped) setTool("servers");
  }, [dropped]);
  return (
    <div className="tools">
      <nav className="tools-nav" aria-label="Network tools">
        {TOOLS.map(([id, label]) => (
          <button key={id} type="button" aria-pressed={tool === id} onClick={() => setTool(id)}>
            {label}
          </button>
        ))}
      </nav>
      <div className="tools-body">
        {(tool === "ping" || tool === "traceroute") && <ProbeTool key={tool} kind={tool} />}
        {tool === "tcp" && <TcpTool />}
        {tool === "dns" && <DnsTool />}
        {tool === "subnet" && <SubnetTool />}
        {tool === "scan" && <ScanTool />}
        {tool === "lan" && <LanTool />}
        {tool === "bandwidth" && <BandwidthTool />}
        {tool === "wol" && <WolTool />}
        {tool === "servers" && <ServersTool dropped={dropped} />}
      </div>
    </div>
  );
}

function ProbeTool({ kind }: { kind: "ping" | "traceroute" }) {
  const [host, setHost] = useState("");
  const [lines, setLines] = useState<string[]>([]);
  const [running, setRunning] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const runningRef = useRef<number | null>(null);
  runningRef.current = running;
  useEffect(() => () => {
    if (runningRef.current !== null) void cancelProbe(runningRef.current);
  }, []);

  async function run(e: FormEvent) {
    e.preventDefault();
    setLines([]);
    setStatus(null);
    try {
      const id = await startProbe(
        kind,
        host,
        (l) => setLines((x) => [...x.slice(-500), l]),
        (code) => {
          setRunning(null);
          setStatus(code === null ? "Stopped" : code === 0 ? "Done" : `Exited with code ${code}`);
        },
      );
      setRunning(id);
    } catch (err) {
      setStatus(msg(err));
    }
  }

  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={host} placeholder="Host name or IP address" spellCheck={false} autoFocus
          onChange={(e) => setHost(e.target.value)} aria-label="Host" />
        {running === null ? (
          <button type="submit" className="primary" disabled={!host.trim()}>
            Run
          </button>
        ) : (
          <button type="button" onClick={() => void cancelProbe(running)}>
            Stop
          </button>
        )}
      </div>
      <pre className="tool-output" aria-live="polite">{lines.join("\n")}</pre>
      {status && <p className="hint">{status}</p>}
    </form>
  );
}

function TcpTool() {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("22");
  const [result, setResult] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  async function run(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      const r = await tcpCheck(host, Number(port));
      setResult(r.open ? `Open — connected to ${r.address} in ${r.millis} ms` : `Closed — ${r.error} (${r.millis} ms)`);
    } catch (err) {
      setResult(msg(err));
    } finally {
      setBusy(false);
    }
  }
  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={host} placeholder="Host" spellCheck={false} autoFocus onChange={(e) => setHost(e.target.value)} aria-label="Host" />
        <input className="port" value={port} inputMode="numeric" onChange={(e) => setPort(e.target.value)} aria-label="Port" />
        <button type="submit" className="primary" disabled={busy || !host.trim()}>
          Check
        </button>
      </div>
      {result && <p className="tool-result">{result}</p>}
    </form>
  );
}

function DnsTool() {
  const [name, setName] = useState("");
  const [result, setResult] = useState<string[] | string | null>(null);
  async function run(e: FormEvent) {
    e.preventDefault();
    try {
      const r = await dnsLookup(name);
      setResult(r.length ? r : "No addresses");
    } catch (err) {
      setResult(msg(err));
    }
  }
  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={name} placeholder="Name to resolve" spellCheck={false} autoFocus onChange={(e) => setName(e.target.value)} aria-label="Name" />
        <button type="submit" className="primary" disabled={!name.trim()}>
          Resolve
        </button>
      </div>
      {Array.isArray(result) ? (
        <pre className="tool-output">{result.join("\n")}</pre>
      ) : (
        result && <p className="tool-result">{result}</p>
      )}
    </form>
  );
}

function SubnetTool() {
  const [input, setInput] = useState("192.168.1.10/24");
  const s = calcSubnet(input);
  return (
    <div className="tool-form">
      <input value={input} onChange={(e) => setInput(e.target.value)} spellCheck={false} autoFocus
        aria-label="Address and mask" placeholder="10.0.0.5/24 or 10.0.0.5 255.255.255.0" />
      {s ? (
        <dl className="fingerprint subnet">
          <dt>Network</dt><dd>{s.network}/{s.prefix}</dd>
          <dt>Netmask</dt><dd>{s.netmask}</dd>
          <dt>Wildcard</dt><dd>{s.wildcard}</dd>
          <dt>Broadcast</dt><dd>{s.broadcast}</dd>
          <dt>Hosts</dt><dd>{s.firstHost} – {s.lastHost} ({s.hosts.toLocaleString()})</dd>
        </dl>
      ) : (
        <p className="hint">Enter an IPv4 address with a prefix (/24) or a dotted mask.</p>
      )}
    </div>
  );
}

const COMMON_PORTS = "21,22,23,25,53,80,110,143,443,445,1433,3306,3389,5432,5900,6379,8080,8443";

function ScanTool() {
  const [host, setHost] = useState("");
  const [ports, setPorts] = useState(COMMON_PORTS);
  const [open, setOpen] = useState<number[]>([]);
  const [checked, setChecked] = useState(0);
  const [running, setRunning] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const runningRef = useRef<number | null>(null);
  runningRef.current = running;
  useEffect(() => () => {
    if (runningRef.current !== null) void cancelProbe(runningRef.current);
  }, []);

  async function run(e: FormEvent) {
    e.preventDefault();
    setOpen([]);
    setChecked(0);
    setStatus(null);
    try {
      const id = await startScan(
        host,
        ports,
        (r) => {
          setChecked((n) => n + 1);
          if (r.open) setOpen((o) => [...o, r.port].sort((a, b) => a - b));
        },
        (d) => {
          setRunning(null);
          setStatus(d.error ?? `${d.cancelled ? "Stopped" : "Done"}${d.address ? ` (${d.address})` : ""}`);
        },
      );
      setRunning(id);
    } catch (err) {
      setStatus(msg(err));
    }
  }

  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={host} placeholder="Host name or IP address" spellCheck={false} autoFocus
          onChange={(e) => setHost(e.target.value)} aria-label="Host" />
        {running === null ? (
          <button type="submit" className="primary" disabled={!host.trim() || !ports.trim()}>
            Scan
          </button>
        ) : (
          <button type="button" onClick={() => void cancelProbe(running)}>
            Stop
          </button>
        )}
      </div>
      <input value={ports} spellCheck={false} onChange={(e) => setPorts(e.target.value)} aria-label="Ports"
        placeholder="22,80,443 or 1-1024 (at most 1024 ports)" />
      <p className="hint">
        Only scan hosts you are allowed to test. Checked {checked}; open: {open.length ? open.join(", ") : "none yet"}
      </p>
      {status && <p className="tool-result">{status}</p>}
    </form>
  );
}

const PORT_NAMES: Record<number, string> = {
  22: "SSH",
  23: "Telnet",
  80: "HTTP",
  443: "HTTPS",
  445: "SMB",
  139: "NetBIOS",
  3389: "RDP",
  5900: "VNC",
  8080: "HTTP",
  8443: "HTTPS",
};

/** Finds hosts on a local subnet: TCP probes on common ports, hardware addresses from the ARP table. */
function LanTool() {
  const [subnet, setSubnet] = useState("");
  const [hosts, setHosts] = useState<LanHost[]>([]);
  const [macs, setMacs] = useState<Record<string, string>>({});
  const [running, setRunning] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const runningRef = useRef<number | null>(null);
  runningRef.current = running;
  useEffect(() => {
    localSubnet().then((s) => s && setSubnet((cur) => cur || s), () => {});
    return () => {
      if (runningRef.current !== null) void cancelProbe(runningRef.current);
    };
  }, []);

  const key = (a: string) => a.split(".").reduce((n, o) => n * 256 + Number(o), 0);

  async function run(e: FormEvent) {
    e.preventDefault();
    setHosts([]);
    setMacs({});
    setStatus("Scanning…");
    try {
      const id = await startLanScan(
        subnet,
        (h) => setHosts((list) => [...list, h].sort((a, b) => key(a.address) - key(b.address))),
        (d) => {
          setRunning(null);
          setMacs(d.macs);
          setStatus(d.cancelled ? "Stopped." : "Done.");
        },
      );
      setRunning(id);
    } catch (err) {
      setStatus(msg(err));
    }
  }

  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={subnet} placeholder="192.168.1.0/24" spellCheck={false} autoFocus aria-label="Subnet"
          onChange={(e) => setSubnet(e.target.value)} />
        {running === null ? (
          <button type="submit" className="primary" disabled={!subnet.trim()}>
            Scan
          </button>
        ) : (
          <button type="button" onClick={() => void cancelProbe(running)}>
            Stop
          </button>
        )}
      </div>
      <p className="hint">
        Finds hosts that answer on common ports (SSH, Telnet, HTTP, SMB, RDP, VNC…); a host with every
        port filtered stays hidden. /22 to /30. Only scan networks you are allowed to test.
      </p>
      {status && (
        <p className="tool-result">
          {status} {hosts.length} host{hosts.length === 1 ? "" : "s"} found.
        </p>
      )}
      {hosts.length > 0 && (
        <table className="lan-hosts">
          <thead>
            <tr><th>Address</th><th>Hardware address</th><th>Open ports</th></tr>
          </thead>
          <tbody>
            {hosts.map((h) => (
              <tr key={h.address}>
                <td><code>{h.address}</code></td>
                <td><code>{macs[h.address] ?? ""}</code></td>
                <td>
                  {h.openPorts.length === 0
                    ? <span className="hint">none open</span>
                    : h.openPorts.map((p) => (
                        <span key={p} className="port-chip" title={`port ${p}`}>{PORT_NAMES[p] ?? p} {p}</span>
                      ))}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </form>
  );
}

const mbit = (bps: number) => `${(bps / 1e6).toFixed(bps < 1e7 ? 2 : 0)} Mbit/s`;

/** Measures throughput to an iperf3 server (iperf3 -s), up or down, one TCP stream. */
function BandwidthTool() {
  const [host, setHost] = useState("");
  const [port, setPort] = useState("5201");
  const [seconds, setSeconds] = useState("10");
  const [download, setDownload] = useState(false);
  const [samples, setSamples] = useState<number[]>([]);
  const [running, setRunning] = useState<number | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const runningRef = useRef<number | null>(null);
  runningRef.current = running;
  useEffect(() => () => {
    if (runningRef.current !== null) void cancelProbe(runningRef.current);
  }, []);

  async function run(e: FormEvent) {
    e.preventDefault();
    setSamples([]);
    setStatus("Testing…");
    const p = Number(port);
    try {
      const id = await startBandwidth(
        host.trim(),
        Number.isInteger(p) && p > 0 && p < 65536 ? p : null,
        Math.min(60, Math.max(1, Number(seconds) || 10)),
        download,
        (s) => setSamples((list) => [...list, s.bitsPerSecond]),
        (d) => {
          setRunning(null);
          if (d.error) setStatus(d.error);
          else if (d.result)
            setStatus(
              `${d.cancelled ? "Stopped. " : ""}${d.result.download ? "Download" : "Upload"}: ${mbit(d.result.bitsPerSecond)} (${(d.result.bytes / 1e6).toFixed(1)} MB in ${d.result.seconds.toFixed(1)} s)`,
            );
        },
      );
      setRunning(id);
    } catch (err) {
      setStatus(msg(err));
    }
  }

  const peak = Math.max(1, ...samples);
  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={host} placeholder="iperf3 server, e.g. 10.0.0.5" spellCheck={false} autoFocus aria-label="Server"
          onChange={(e) => setHost(e.target.value)} />
        <input className="port" value={port} inputMode="numeric" aria-label="Port"
          onChange={(e) => setPort(e.target.value.replace(/[^0-9]/g, ""))} />
        {running === null ? (
          <button type="submit" className="primary" disabled={!host.trim()}>
            Start
          </button>
        ) : (
          <button type="button" onClick={() => void cancelProbe(running)}>
            Stop
          </button>
        )}
      </div>
      <div className="row">
        <label>
          Seconds{" "}
          <input className="port" value={seconds} inputMode="numeric" aria-label="Seconds"
            onChange={(e) => setSeconds(e.target.value.replace(/[^0-9]/g, ""))} />
        </label>
        <label className="checkbox">
          <input type="radio" name="direction" checked={!download} onChange={() => setDownload(false)} />
          Upload (this computer sends)
        </label>
        <label className="checkbox">
          <input type="radio" name="direction" checked={download} onChange={() => setDownload(true)} />
          Download (the server sends)
        </label>
      </div>
      <p className="hint">
        Needs an iperf3 server on the other side (<code>iperf3 -s</code>, port 5201). One TCP stream.
      </p>
      {samples.length > 0 && (
        <ol className="bandwidth-bars" aria-label="Throughput per second">
          {samples.map((b, i) => (
            <li key={i} title={`second ${i + 1}: ${mbit(b)}`}>
              <span style={{ height: `${Math.round((b / peak) * 100)}%` }} />
            </li>
          ))}
        </ol>
      )}
      {status && <p className="tool-result">{status}</p>}
    </form>
  );
}

function WolTool() {
  const [mac, setMac] = useState("");
  const [broadcast, setBroadcast] = useState("");
  const [result, setResult] = useState<string | null>(null);
  async function run(e: FormEvent) {
    e.preventDefault();
    try {
      await wakeOnLan(mac, broadcast.trim() || null);
      setResult(`Magic packet sent to ${broadcast.trim() || "255.255.255.255"}:9.`);
    } catch (err) {
      setResult(msg(err));
    }
  }
  return (
    <form className="tool-form" onSubmit={run}>
      <div className="row">
        <input value={mac} placeholder="MAC address, e.g. aa:bb:cc:dd:ee:ff" spellCheck={false} autoFocus
          onChange={(e) => setMac(e.target.value)} aria-label="MAC address" />
        <button type="submit" className="primary" disabled={!mac.trim()}>
          Wake
        </button>
      </div>
      <input value={broadcast} placeholder="Broadcast address (optional), e.g. 192.168.1.255" spellCheck={false}
        onChange={(e) => setBroadcast(e.target.value)} aria-label="Broadcast address" />
      <p className="hint">The computer must have Wake-on-LAN enabled and be on this network (or the broadcast must reach it).</p>
      {result && <p className="tool-result">{result}</p>}
    </form>
  );
}
