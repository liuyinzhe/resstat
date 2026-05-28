use std::fs;
use std::time::Instant;

/// Raw process statistics read from /proc at a point in time.
#[derive(Debug, Clone)]
struct RawStats {
    utime: u64,
    stime: u64,
    rss_kb: u64,
    vsz_kb: u64,
    read_bytes: u64,
    write_bytes: u64,
    threads: u32,
    when: Instant,
}

impl Default for RawStats {
    fn default() -> Self {
        Self {
            utime: 0,
            stime: 0,
            rss_kb: 0,
            vsz_kb: 0,
            read_bytes: 0,
            write_bytes: 0,
            threads: 0,
            when: Instant::now(),
        }
    }
}

/// Parse /proc/<pid>/stat. Returns (utime, stime) in clock ticks.
/// Format: pid (comm) state ... utime(14) stime(15) ...
/// After the closing ')' of comm, fields are space-separated.
fn parse_stat(pid: u32) -> Option<(u64, u64)> {
    let content = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;
    let close_paren = content.rfind(')')?;
    let fields: Vec<&str> = content[close_paren + 2..].split_whitespace().collect();
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((utime, stime))
}

/// Parse /proc/<pid>/status. Returns (VmRSS_kB, VmSize_kB, threads).
fn parse_status(pid: u32) -> Option<(u64, u64, u32)> {
    let content = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    let (mut rss, mut vsz, mut threads) = (0u64, 0u64, 0u32);
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            rss = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("VmSize:") {
            vsz = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("Threads:") {
            threads = v.trim().split_whitespace().next()?.parse().ok()?;
        }
    }
    Some((rss, vsz, threads))
}

/// Parse /proc/<pid>/io. Returns (read_bytes, write_bytes).
fn parse_io(pid: u32) -> Option<(u64, u64)> {
    let content = fs::read_to_string(format!("/proc/{}/io", pid)).ok()?;
    let (mut read_bytes, mut write_bytes) = (0u64, 0u64);
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("read_bytes:") {
            read_bytes = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("write_bytes:") {
            write_bytes = v.trim().split_whitespace().next()?.parse().ok()?;
        }
    }
    Some((read_bytes, write_bytes))
}

fn read_memtotal_kb() -> u64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0)
}

fn cpu_count() -> u64 {
    fs::read_to_string("/proc/cpuinfo")
        .map(|c| c.lines().filter(|l| l.starts_with("processor")).count() as u64)
        .unwrap_or(1)
        .max(1)
}

const CLOCK_TICKS_PER_SEC: f64 = 100.0;

pub struct MetricsCollector {
    pid: u32,
    start: Instant,
    memtotal_kb: u64,
    cpu_count: u64,

    // CPU delta tracking (per refresh)
    prev_utime: u64,
    prev_stime: u64,
    prev_when: Instant,
    cached_cpu_pct: f64,

    // IO delta tracking (per sample — accumulates across refreshes)
    baseline_read_bytes: u64,
    baseline_write_bytes: u64,

    // Latest snapshot from most recent refresh
    latest: RawStats,
}

impl MetricsCollector {
    pub fn new(pid: u32) -> Self {
        let now = Instant::now();
        let latest = Self::read_raw(pid).unwrap_or_default();
        Self {
            pid,
            start: now,
            memtotal_kb: read_memtotal_kb(),
            cpu_count: cpu_count(),
            prev_utime: latest.utime,
            prev_stime: latest.stime,
            prev_when: now,
            cached_cpu_pct: 0.0,
            baseline_read_bytes: latest.read_bytes,
            baseline_write_bytes: latest.write_bytes,
            latest,
        }
    }

    fn read_raw(pid: u32) -> Option<RawStats> {
        let (utime, stime) = parse_stat(pid)?;
        let (rss_kb, vsz_kb, threads) = parse_status(pid)?;
        let (read_bytes, write_bytes) = parse_io(pid)?;
        Some(RawStats {
            utime,
            stime,
            rss_kb,
            vsz_kb,
            read_bytes,
            write_bytes,
            threads,
            when: Instant::now(),
        })
    }

    /// Re-read /proc and compute CPU% delta since last refresh.
    /// Called at every refresh tick. If /proc read fails, keeps previous values.
    pub fn refresh(&mut self) {
        if let Some(stats) = Self::read_raw(self.pid) {
            let delta_ticks = (stats.utime - self.prev_utime) + (stats.stime - self.prev_stime);
            let delta_secs = stats.when.duration_since(self.prev_when).as_secs_f64();
            if delta_secs > 0.0 {
                self.cached_cpu_pct =
                    (delta_ticks as f64 / CLOCK_TICKS_PER_SEC) / delta_secs
                        / self.cpu_count as f64
                        * 100.0;
            }
            self.prev_utime = stats.utime;
            self.prev_stime = stats.stime;
            self.prev_when = stats.when;
            self.latest = stats;
        }
    }

    pub fn cpu_pct(&self) -> f64 {
        self.cached_cpu_pct
    }

    pub fn rss_mb(&self) -> f64 {
        self.latest.rss_kb as f64 / 1024.0
    }

    pub fn vsz_mb(&self) -> f64 {
        self.latest.vsz_kb as f64 / 1024.0
    }

    pub fn mem_pct(&self) -> f64 {
        if self.memtotal_kb == 0 {
            return 0.0;
        }
        self.latest.rss_kb as f64 / self.memtotal_kb as f64 * 100.0
    }

    /// Returns IO read delta since last call (in MB), resets baseline.
    pub fn io_read_mb_delta(&mut self) -> f64 {
        let delta = self.latest.read_bytes.saturating_sub(self.baseline_read_bytes);
        self.baseline_read_bytes = self.latest.read_bytes;
        delta as f64 / 1024.0 / 1024.0
    }

    /// Returns IO write delta since last call (in MB), resets baseline.
    pub fn io_write_mb_delta(&mut self) -> f64 {
        let delta = self.latest.write_bytes.saturating_sub(self.baseline_write_bytes);
        self.baseline_write_bytes = self.latest.write_bytes;
        delta as f64 / 1024.0 / 1024.0
    }

    pub fn threads(&self) -> u32 {
        self.latest.threads
    }

    pub fn elapsed_s(&self) -> f64 {
        self.latest.when.duration_since(self.start).as_secs_f64()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stat_field_positions() {
        // Simulate fields after ") " in /proc/<pid>/stat
        // state ppid pgrp session tty_nr tpgid flags minflt cminflt majflt cmajflt utime stime ...
        let rest = "S 1 2 3 4 5 6 7 8 9 10 100 200 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let fields: Vec<&str> = rest.split_whitespace().collect();
        assert_eq!(fields[11], "100"); // utime at index 11
        assert_eq!(fields[12], "200"); // stime at index 12
    }

    #[test]
    fn test_parse_stat_with_parens_in_comm() {
        // Comm field contains ")". rfind(')') finds the LAST one.
        let line = "1234 (my (special) proc) S 0 0 0 0 0 0 0 0 0 0 50 60 0 0 0 0 0 0 0 0 0";
        let close = line.rfind(')').unwrap();
        let fields: Vec<&str> = line[close + 2..].split_whitespace().collect();
        assert_eq!(fields[11], "50");
        assert_eq!(fields[12], "60");
    }

    #[test]
    fn test_metrics_collector_new() {
        let mc = MetricsCollector::new(1);
        assert!(mc.elapsed_s() >= 0.0);
        assert_eq!(mc.cpu_pct(), 0.0);
    }
}
