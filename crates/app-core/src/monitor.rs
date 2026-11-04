//! Server monitor bar (opt-in): one fixed, read-only command run over the
//! terminal's SSH connection. Linux only, because it reads `/proc`; other
//! servers report `None`. The UI never supplies any part of the command.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use protocol_ssh::SshSession;
use serde::Serialize;

use crate::AppError;

/// Wrapped in `sh -c` so the user's login shell (fish, csh...) does not matter.
const COMMAND: &str = "sh -c 'echo @stat; head -n 1 /proc/stat; echo @load; cat /proc/loadavg; \
                       echo @mem; cat /proc/meminfo; echo @net; cat /proc/net/dev; \
                       echo @disk; LC_ALL=C df -Pk /'";

/// A stalled connection must not leave the bar waiting forever.
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);

/// What one run of the command reports (counters since boot).
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    cpu_total: u64,
    cpu_idle: u64,
    load1: f32,
    mem_total: u64,
    mem_available: u64,
    net_rx: u64,
    net_tx: u64,
    disk_total: u64,
    disk_used: u64,
}

/// Values for the bar. Rates need two samples, so the first has none.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub cpu_percent: Option<f32>,
    pub load1: f32,
    pub mem_total: u64,
    pub mem_used: u64,
    pub disk_total: u64,
    pub disk_used: u64,
    pub rx_per_sec: Option<u64>,
    pub tx_per_sec: Option<u64>,
}

/// Splits the output into its `@name` sections.
fn section<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("@{name}\n");
    let start = text.find(&marker)? + marker.len();
    let rest = &text[start..];
    Some(rest.find("\n@").map_or(rest, |end| &rest[..end]))
}

fn parse_cpu(stat: &str) -> Option<(u64, u64)> {
    let fields: Vec<u64> = stat
        .strip_prefix("cpu")?
        .split_whitespace()
        .map(|f| f.parse().ok())
        .collect::<Option<_>>()?;
    // user nice system idle iowait irq softirq steal; guest time is already
    // part of user and nice.
    let idle = fields.get(3)? + fields.get(4).unwrap_or(&0);
    Some((fields.iter().take(8).sum(), idle))
}

/// Bytes for a `/proc/meminfo` entry (given in KiB).
fn meminfo(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|l| {
        let value = l.strip_prefix(key)?.strip_prefix(':')?;
        Some(value.split_whitespace().next()?.parse::<u64>().ok()? * 1024)
    })
}

/// Received and sent bytes, all interfaces but loopback.
fn parse_net(text: &str) -> Option<(u64, u64)> {
    let mut totals = (0, 0);
    for line in text.lines().skip(2) {
        let (name, values) = line.split_once(':')?;
        if name.trim() == "lo" {
            continue;
        }
        let values: Vec<&str> = values.split_whitespace().collect();
        totals.0 += values.first()?.parse::<u64>().ok()?;
        totals.1 += values.get(8)?.parse::<u64>().ok()?;
    }
    Some(totals)
}

/// Size and used bytes of the root file system (`df -Pk`).
fn parse_disk(text: &str) -> Option<(u64, u64)> {
    let mut fields = text.lines().nth(1)?.split_whitespace().skip(1);
    let total: u64 = fields.next()?.parse().ok()?;
    let used: u64 = fields.next()?.parse().ok()?;
    Some((total * 1024, used * 1024))
}

fn parse(text: &str) -> Option<Sample> {
    let (cpu_total, cpu_idle) = parse_cpu(section(text, "stat")?)?;
    let mem = section(text, "mem")?;
    let mem_total = meminfo(mem, "MemTotal")?;
    let (net_rx, net_tx) = parse_net(section(text, "net")?)?;
    let (disk_total, disk_used) = parse_disk(section(text, "disk")?)?;
    Some(Sample {
        cpu_total,
        cpu_idle,
        load1: section(text, "load")?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?,
        mem_total,
        // Older kernels have no MemAvailable.
        mem_available: meminfo(mem, "MemAvailable")
            .or_else(|| meminfo(mem, "MemFree"))?
            .min(mem_total),
        net_rx,
        net_tx,
        disk_total,
        disk_used,
    })
}

fn stats(now: &Sample, before: Option<(&Sample, Duration)>) -> ServerStats {
    let rate = |new: u64, old: u64, elapsed: Duration| {
        // Counters restart with an interface or a reboot.
        (new.saturating_sub(old) as f64 / elapsed.as_secs_f64().max(0.001)) as u64
    };
    let cpu_percent = before.and_then(|(old, _)| {
        let total = now.cpu_total.checked_sub(old.cpu_total)?;
        let idle = now.cpu_idle.checked_sub(old.cpu_idle)?;
        (total > 0).then(|| (100.0 * (1.0 - idle as f32 / total as f32)).clamp(0.0, 100.0))
    });
    ServerStats {
        cpu_percent,
        load1: now.load1,
        mem_total: now.mem_total,
        mem_used: now.mem_total - now.mem_available,
        disk_total: now.disk_total,
        disk_used: now.disk_used,
        rx_per_sec: before.map(|(old, t)| rate(now.net_rx, old.net_rx, t)),
        tx_per_sec: before.map(|(old, t)| rate(now.net_tx, old.net_tx, t)),
    }
}

/// Last sample of each terminal, for CPU and network rates.
#[derive(Default)]
pub struct ServerMonitor {
    last: Mutex<HashMap<u64, (Instant, Sample)>>,
}

impl ServerMonitor {
    /// Current values for terminal `id`; `None` where the server has no
    /// `/proc` (not Linux) or does not allow running commands.
    pub async fn sample(
        &self,
        id: u64,
        session: &SshSession,
    ) -> Result<Option<ServerStats>, AppError> {
        let output = tokio::time::timeout(SAMPLE_TIMEOUT, session.exec(COMMAND))
            .await
            .map_err(|_| AppError::invalid("server monitor", "the server did not answer"))??;
        let Some(sample) = parse(&String::from_utf8_lossy(&output.stdout)) else {
            return Ok(None);
        };
        let now = Instant::now();
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        let before = last.get(&id).map(|(at, s)| (s, now - *at));
        let result = stats(&sample, before);
        last.insert(id, (now, sample));
        Ok(Some(result))
    }

    pub fn forget(&self, id: u64) {
        self.last
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OUTPUT: &str = "@stat
cpu  100 0 100 700 100 0 0 0 0 0
@load
0.42 0.30 0.20 1/123 4567
@mem
MemTotal:       2048 kB
MemFree:         100 kB
MemAvailable:   1024 kB
@net
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 999 1 0 0 0 0 0 0 999 1 0 0 0 0 0 0
  eth0: 1000 1 0 0 0 0 0 0 2000 1 0 0 0 0 0 0
  eth1: 500 1 0 0 0 0 0 0 500 1 0 0 0 0 0 0
@disk
Filesystem     1024-blocks   Used Available Capacity Mounted on
/dev/sda1             1000    250       750      25% /
";

    #[test]
    fn parses_proc_output() {
        let s = parse(OUTPUT).unwrap();
        assert_eq!((s.cpu_total, s.cpu_idle), (1000, 800));
        assert_eq!(s.load1, 0.42);
        assert_eq!((s.mem_total, s.mem_available), (2048 * 1024, 1024 * 1024));
        // Loopback is not counted.
        assert_eq!((s.net_rx, s.net_tx), (1500, 2500));
        assert_eq!((s.disk_total, s.disk_used), (1000 * 1024, 250 * 1024));
    }

    #[test]
    fn other_systems_are_not_parsed() {
        assert_eq!(parse(""), None);
        assert_eq!(parse("@stat\n@load\n@mem\n@net\n@disk\n"), None);
    }

    #[test]
    fn rates_need_two_samples() {
        let a = parse(OUTPUT).unwrap();
        let first = stats(&a, None);
        assert_eq!(first.cpu_percent, None);
        assert_eq!(first.rx_per_sec, None);
        assert_eq!(first.mem_used, 1024 * 1024);

        let mut b = a.clone();
        b.cpu_total += 100;
        b.cpu_idle += 75;
        b.net_rx += 2000;
        b.net_tx += 4000;
        let second = stats(&b, Some((&a, Duration::from_secs(2))));
        assert_eq!(second.cpu_percent, Some(25.0));
        assert_eq!(second.rx_per_sec, Some(1000));
        assert_eq!(second.tx_per_sec, Some(2000));
    }

    #[test]
    fn a_counter_reset_does_not_underflow() {
        let a = parse(OUTPUT).unwrap();
        let mut b = a.clone();
        b.cpu_total = 10;
        b.net_rx = 0;
        let s = stats(&b, Some((&a, Duration::from_secs(1))));
        assert_eq!(s.cpu_percent, None);
        assert_eq!(s.rx_per_sec, Some(0));
    }
}
