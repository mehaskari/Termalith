import { type FormEvent, useEffect, useRef, useState } from "react";
import { cancelProbe, dnsLookup, isIpcError, startProbe, startScan, tcpCheck, wakeOnLan } from "../terminal/api";
import type { DroppedFile } from "../terminal/api";
import { ServersTool } from "./ServersTool";
import { calcSubnet } from "./subnet";

type Tool = "ping" | "traceroute" | "tcp" | "scan" | "dns" | "subnet" | "wol" | "servers";
const TOOLS: Array<[Tool, string]> = [
  ["ping", "Ping"],
  ["traceroute", "Traceroute"],
  ["tcp", "TCP port check"],
  ["scan", "Port scan"],
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
