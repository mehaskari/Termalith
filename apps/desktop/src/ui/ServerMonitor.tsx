import { useEffect, useState } from "react";
import { serverStats, serverStatsStop, type ServerStats } from "../terminal/api";
import { formatBytes } from "./monitorFormat";

const INTERVAL_MS = 3000;

/** CPU, memory, disk and network of the server behind an SSH terminal. */
export function ServerMonitor({ terminalId, active }: { terminalId: number; active: boolean }) {
  const [stats, setStats] = useState<ServerStats | null>(null);
  const [state, setState] = useState<"waiting" | "ok" | "unsupported">("waiting");

  useEffect(() => {
    if (!active) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    // The next sample is timed from the end of the last, so slow servers
    // never pile up requests.
    const tick = async () => {
      try {
        const s = await serverStats(terminalId);
        if (stopped) return;
        setStats(s);
        setState(s ? "ok" : "unsupported");
        if (!s) return;
      } catch {
        if (!stopped) setState("unsupported");
        return;
      }
      timer = setTimeout(() => void tick(), INTERVAL_MS);
    };
    void tick();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [active, terminalId]);

  useEffect(() => () => void serverStatsStop(terminalId).catch(() => {}), [terminalId]);

  if (state === "waiting") return null;
  if (state === "unsupported" || !stats)
    return <div className="server-monitor">Server monitor is available on Linux servers only.</div>;
  return (
    <div className="server-monitor" role="status" aria-label="Server monitor" aria-live="off">
      <span>CPU {stats.cpuPercent === null ? "…" : `${Math.round(stats.cpuPercent)}%`}</span>
      <span>Load {stats.load1.toFixed(2)}</span>
      <span>
        Memory {formatBytes(stats.memUsed)} / {formatBytes(stats.memTotal)}
      </span>
      <span>
        Disk / {formatBytes(stats.diskUsed)} / {formatBytes(stats.diskTotal)}
      </span>
      <span>
        ↓ {stats.rxPerSec === null ? "…" : `${formatBytes(stats.rxPerSec)}/s`} ↑{" "}
        {stats.txPerSec === null ? "…" : `${formatBytes(stats.txPerSec)}/s`}
      </span>
    </div>
  );
}
