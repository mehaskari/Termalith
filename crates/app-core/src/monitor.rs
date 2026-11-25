//! Server monitor bar (opt-in): one fixed, read-only command run over the
//! terminal's SSH connection. Linux reads `/proc`; macOS and FreeBSD use
//! `sysctl`, `vm_stat`, `ps` and `netstat`; other servers report `None`. The
//! UI never supplies any part of the command.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use protocol_ssh::SshSession;
use serde::Serialize;

use crate::AppError;

/// Wrapped in `sh -c` so the user's login shell (fish, csh...) does not
/// matter. Without `/proc`, the BSD tools (macOS, FreeBSD) answer instead.
const COMMAND: &str = "sh -c 'if [ -r /proc/stat ]; then \
                       echo @stat; head -n 1 /proc/stat; echo @load; cat /proc/loadavg; \
                       echo @mem; cat /proc/meminfo; echo @net; cat /proc/net/dev; \
                       else echo @bsd; uname -s; echo @cpu; ps -A -o %cpu=; \
                       echo @ncpu; sysctl -n hw.ncpu; echo @load; sysctl -n vm.loadavg; \
                       echo @memsize; sysctl -n hw.memsize 2>/dev/null || sysctl -n hw.physmem; \
                       echo @vm; vm_stat 2>/dev/null || sysctl hw.pagesize vm.stats.vm.v_free_count vm.stats.vm.v_inactive_count; \
                       echo @netstat; netstat -ibn; fi; \
                       echo @disk; LC_ALL=C df -Pk /'";

/// A stalled connection must not leave the bar waiting forever.
const SAMPLE_TIMEOUT: Duration = Duration::from_secs(5);

/// What one run of the command reports (counters since boot).
#[derive(Debug, Clone, PartialEq)]
struct Sample {
    cpu_total: u64,
    cpu_idle: u64,
    /// BSD: CPU use now, from `ps` (no tick counters to diff).
    cpu_now: Option<f32>,
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

/// CPU use in percent of all cores: the sum of `ps -o %cpu` over the core count.
fn parse_ps_cpu(text: &str, cores: u32) -> Option<f32> {
    let sum: f32 = text
        .lines()
        .filter_map(|l| l.trim().replace(',', ".").parse::<f32>().ok())
        .sum();
    (cores > 0).then(|| (sum / cores as f32).clamp(0.0, 100.0))
}

/// `vm.loadavg`: `{ 1.23 1.10 0.98 }`.
fn parse_loadavg(text: &str) -> Option<f32> {
    text.split_whitespace()
        .find(|w| w.chars().next().is_some_and(|c| c.is_ascii_digit()))?
        .replace(',', ".")
        .parse()
        .ok()
}

/// Free memory in bytes: macOS `vm_stat` (free, inactive and speculative
/// pages) or FreeBSD `sysctl` names (free and inactive pages).
fn parse_free_memory(text: &str) -> Option<u64> {
    let number = |l: &str| -> Option<u64> {
        l.rsplit([':', ' '])
            .find(|w| !w.is_empty())?
            .trim_end_matches('.')
            .parse()
            .ok()
    };
    if let Some(head) = text.lines().next().filter(|l| l.contains("page size of")) {
        let page: u64 = head
            .split("page size of")
            .nth(1)?
            .split_whitespace()
            .next()?
            .parse()
            .ok()?;
        let pages: u64 = text
            .lines()
            .filter(|l| {
                l.starts_with("Pages free:")
                    || l.starts_with("Pages inactive:")
                    || l.starts_with("Pages speculative:")
            })
            .filter_map(number)
            .sum();
        return Some(pages * page);
    }
    let value = |key: &str| text.lines().find(|l| l.starts_with(key)).and_then(number);
    let page = value("hw.pagesize:")?;
    Some((value("vm.stats.vm.v_free_count:")? + value("vm.stats.vm.v_inactive_count:")?) * page)
}

/// Received and sent bytes of all interfaces but loopback (`netstat -ibn`,
/// one `<Link#…>` row per interface; some rows have no address column).
fn parse_netstat(text: &str) -> Option<(u64, u64)> {
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next()?.split_whitespace().collect();
    let ibytes = header.iter().position(|h| *h == "Ibytes")?;
    let obytes = header.iter().position(|h| *h == "Obytes")?;
    let mut totals = (0, 0);
    for line in lines {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 || !cols[2].starts_with("<Link") || cols[0].starts_with("lo") {
            continue;
        }
        let shift = header.len().saturating_sub(cols.len());
        let get = |i: usize| cols.get(i.checked_sub(shift)?)?.parse::<u64>().ok();
        totals.0 += get(ibytes)?;
        totals.1 += get(obytes)?;
    }
    Some(totals)
}

fn parse_bsd(text: &str) -> Option<Sample> {
    let cores: u32 = section(text, "ncpu")?.trim().parse().ok()?;
    let mem_total: u64 = section(text, "memsize")?.trim().parse().ok()?;
    let (net_rx, net_tx) = parse_netstat(section(text, "netstat")?)?;
    let (disk_total, disk_used) = parse_disk(section(text, "disk")?)?;
    Some(Sample {
        cpu_total: 0,
        cpu_idle: 0,
        cpu_now: parse_ps_cpu(section(text, "cpu")?, cores),
        load1: parse_loadavg(section(text, "load")?)?,
        mem_total,
        mem_available: parse_free_memory(section(text, "vm")?)?.min(mem_total),
        net_rx,
        net_tx,
        disk_total,
        disk_used,
    })
}

fn parse(text: &str) -> Option<Sample> {
    if section(text, "bsd").is_some() {
        return parse_bsd(text);
    }
    let (cpu_total, cpu_idle) = parse_cpu(section(text, "stat")?)?;
    let mem = section(text, "mem")?;
    let mem_total = meminfo(mem, "MemTotal")?;
    let (net_rx, net_tx) = parse_net(section(text, "net")?)?;
    let (disk_total, disk_used) = parse_disk(section(text, "disk")?)?;
    Some(Sample {
        cpu_total,
        cpu_idle,
        cpu_now: None,
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
    let cpu_percent = now.cpu_now.or_else(|| {
        before.and_then(|(old, _)| {
            let total = now.cpu_total.checked_sub(old.cpu_total)?;
            let idle = now.cpu_idle.checked_sub(old.cpu_idle)?;
            (total > 0).then(|| (100.0 * (1.0 - idle as f32 / total as f32)).clamp(0.0, 100.0))
        })
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
    /// Current values for terminal `id`; `None` where the server is neither
    /// Linux nor a BSD (macOS, FreeBSD) or does not allow running commands.
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

    const MACOS: &str = "@bsd
Darwin
@cpu
 12.5
  3,5
 0.0
@ncpu
8
@load
{ 1.52 1.40 1.31 }
@memsize
17179869184
@vm
Mach Virtual Memory Statistics: (page size of 16384 bytes)
Pages free:                               10000.
Pages active:                            300000.
Pages inactive:                           20000.
Pages speculative:                         5000.
@netstat
Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll
lo0        16384 <Link#1>                          500     0      7000      500     0      7000     0
lo0        16384 127           127.0.0.1           500     -      7000      500     -      7000     -
en0        1500  <Link#6>    aa:bb:cc:dd:ee:ff    1000     0    200000      900     0    100000     0
en0        1500  192.168.1     192.168.1.5        1000     -    200000      900     -    100000     -
utun0      1380  <Link#9>                           10     0      1000       12     0      2000     0
@disk
Filesystem     1024-blocks      Used Available Capacity  Mounted on
/dev/disk3s1s1   194314280  13000000 100000000    12%    /
";

    const FREEBSD: &str = "@bsd
FreeBSD
@cpu
 50.0
@ncpu
2
@load
{ 0.10 0.05 0.01 }
@memsize
4294967296
@vm
hw.pagesize: 4096
vm.stats.vm.v_free_count: 100000
vm.stats.vm.v_inactive_count: 50000
@netstat
Name    Mtu Network       Address              Ipkts Ierrs Idrop     Ibytes    Opkts Oerrs     Obytes  Coll
vtnet0 1500 <Link#1>      52:54:00:12:34:56     3000     0     0    4000000     2000     0    1000000     0
lo0   16384 <Link#2>      lo0                     10     0     0        800       10     0        800     0
@disk
Filesystem 1024-blocks    Used   Avail Capacity  Mounted on
/dev/vtbd0p2  20000000 5000000 13000000    28%    /
";

    #[test]
    fn parses_macos_output() {
        let s = parse(MACOS).unwrap();
        assert_eq!(s.cpu_now, Some(2.0), "16 % over 8 cores");
        assert_eq!(s.load1, 1.52);
        assert_eq!(s.mem_total, 17_179_869_184);
        assert_eq!(s.mem_available, 35_000 * 16_384);
        // Loopback left out; utun0 has no address column.
        assert_eq!((s.net_rx, s.net_tx), (201_000, 102_000));
        assert_eq!(
            (s.disk_total, s.disk_used),
            (194_314_280 * 1024, 13_000_000 * 1024)
        );
        let first = stats(&s, None);
        assert_eq!(
            first.cpu_percent,
            Some(2.0),
            "BSD CPU needs no second sample"
        );
        assert_eq!(first.rx_per_sec, None);
    }

    #[test]
    fn parses_freebsd_output() {
        let s = parse(FREEBSD).unwrap();
        assert_eq!(s.cpu_now, Some(25.0));
        assert_eq!(s.load1, 0.10);
        assert_eq!(s.mem_available, 150_000 * 4096);
        assert_eq!((s.net_rx, s.net_tx), (4_000_000, 1_000_000));
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
