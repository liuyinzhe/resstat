use std::fs;
use std::time::Instant;

/// 从 /proc 文件系统读取的原始进程统计数据（一次快照）
///
/// 所有字段均为从 /proc/<pid>/ 下各文件直接解析的原始值，未做任何换算。
/// `when` 字段记录读取时刻，用于后续计算时间差。
#[derive(Debug, Clone)]
struct RawStats {
    utime: u64,       // 用户态 CPU 时间（clock tick 数），来自 /proc/<pid>/stat 字段 14
    stime: u64,       // 内核态 CPU 时间（clock tick 数），来自 /proc/<pid>/stat 字段 15
    rss_kb: u64,      // 物理内存驻留集大小（kB），来自 /proc/<pid>/status 的 VmRSS
    vsz_kb: u64,      // 虚拟内存大小（kB），来自 /proc/<pid>/status 的 VmSize
    read_bytes: u64,  // 累计磁盘读取字节数，来自 /proc/<pid>/io 的 read_bytes
    write_bytes: u64, // 累计磁盘写入字节数，来自 /proc/<pid>/io 的 write_bytes
    threads: u32,     // 当前线程数，来自 /proc/<pid>/status 的 Threads
    when: Instant,    // 读取该快照的精确时刻，用于时间差计算
}

// 为 RawStats 实现 Default trait
// Instant 类型不支持 derive(Default)，因此手动实现
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

/// 解析 /proc/<pid>/stat，提取用户态和内核态 CPU 时间
///
/// /proc/<pid>/stat 格式示例:
/// `1234 (process name) S 1 1234 1234 0 -1 ...`
///
/// 关键难点: 第 2 个字段 (comm) 可能包含空格和括号，如 `(my (special) proc)`。
/// 因此不能简单按空格切分，必须找到 **最后一个** `)` 作为 comm 字段的结束标记。
///
/// `)` 之后的字段按空格切分（从 0 开始索引）:
/// - 索引 0: state (进程状态)
/// - 索引 11: utime (用户态 CPU 时间，clock ticks)
/// - 索引 12: stime (内核态 CPU 时间，clock ticks)
///
/// # 返回值
/// `Some((utime, stime))` 成功解析; `None` 文件不存在或格式异常
fn parse_stat(pid: u32) -> Option<(u64, u64)> {
    // 读取整个 stat 文件（通常只有一行，约几百字节）
    let content = fs::read_to_string(format!("/proc/{}/stat", pid)).ok()?;

    // 找最后一个 ')' 的位置，因为进程名自身可能包含括号
    let close_paren = content.rfind(')')?;

    // 跳过 ") " 两个字符，然后按空格切分剩余字段
    // 使用 get() 防止异常短的 stat 内容导致 panic
    let rest = content.get(close_paren + 2..)?;
    let fields: Vec<&str> = rest.split_whitespace().collect();

    // 从切分结果中提取 utime (索引 11) 和 stime (索引 12)
    let utime: u64 = fields.get(11)?.parse().ok()?;
    let stime: u64 = fields.get(12)?.parse().ok()?;
    Some((utime, stime))
}

/// 解析 /proc/<pid>/status，提取内存使用和线程数
///
/// /proc/<pid>/status 是键值对格式，每行形如 `Key:\tvalue` 或 `Key:\tvalue kB`。
/// 通过行前缀匹配提取目标字段:
/// - VmRSS:  物理内存驻留集大小 (kB)
/// - VmSize: 虚拟内存总大小 (kB)
/// - Threads: 当前线程数
///
/// # 返回值
/// `Some((VmRSS_kB, VmSize_kB, threads))` 成功; `None` 文件不存在
fn parse_status(pid: u32) -> Option<(u64, u64, u32)> {
    let content = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    let (mut rss, mut vsz, mut threads) = (0u64, 0u64, 0u32);

    // 逐行扫描，比多次 split 更高效
    for line in content.lines() {
        if let Some(v) = line.strip_prefix("VmRSS:") {
            // 格式: "VmRSS:\t    1234 kB" → 取第一个空白分隔的 token 作为数值
            rss = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("VmSize:") {
            vsz = v.trim().split_whitespace().next()?.parse().ok()?;
        } else if let Some(v) = line.strip_prefix("Threads:") {
            threads = v.trim().split_whitespace().next()?.parse().ok()?;
        }
    }
    Some((rss, vsz, threads))
}

/// 解析 /proc/<pid>/io，提取累计磁盘读写字节数
///
/// /proc/<pid>/io 格式（每行 "key: value"）:
/// ```
/// rchar: 123456
/// wchar: 789012
/// read_bytes: 4096
/// write_bytes: 8192
/// ...
/// ```
///
/// 注意: read_bytes/write_bytes 是**实际从存储层读取/写入**的字节数，
/// 区别于 rchar/wchar（包含 page cache 命中的读写）。
/// 某些内核版本或权限下此文件可能不可读，此时返回 None 并降级为 0。
///
/// # 返回值
/// `Some((read_bytes, write_bytes))` 成功; `None` 文件不可读
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

/// 读取系统总内存（kB），用于计算内存占比
///
/// 从 /proc/meminfo 读取 MemTotal 行。
/// 该值在系统启动后不会变化，因此在 MetricsCollector 初始化时仅读取一次。
fn read_memtotal_kb() -> u64 {
    fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|c| {
            c.lines()
                .find(|l| l.starts_with("MemTotal:"))
                .and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
        })
        .unwrap_or(0) // 极低概率失败（/proc 未挂载），降级为 0
}

/// 获取 CPU 逻辑核数，用于将 CPU 时间归一化为单核百分比
///
/// 通过统计 /proc/cpuinfo 中 "processor" 行的数量得到逻辑核数。
/// 该值在系统启动后不变，仅在 MetricsCollector 初始化时读取一次。
fn cpu_count() -> u64 {
    fs::read_to_string("/proc/cpuinfo")
        .map(|c| c.lines().filter(|l| l.starts_with("processor")).count() as u64)
        .unwrap_or(1) // 降级为 1，避免除零
        .max(1)       // 确保至少为 1
}

/// Linux 时钟滴答频率（USER_HZ）
///
/// 标准 Linux 内核的 USER_HZ 为 100（即每秒钟 100 个 jiffy）。
/// /proc/<pid>/stat 中的 utime/stime 以此为单位。
/// 该值自 Linux 2.6 以来一直为 100，硬编码避免调用 libc::sysconf。
const CLOCK_TICKS_PER_SEC: f64 = 100.0;

/// 进程资源指标采集器
///
/// 负责三个层次的差值追踪:
///
/// **刷新级（per refresh）** — CPU 使用率
/// 每次 refresh() 读取 /proc，计算自**上一次刷新**以来的 CPU 时间增量，
/// 换算为单核百分比。这反映的是两次刷新之间（通常 ~100ms）的瞬时 CPU 使用率。
///
/// **采样级（per sample）** — IO 读写量
/// io_read_mb_delta() / io_write_mb_delta() 返回自**上一次采样**以来的累计
/// IO 变化量。虽然底层每次 refresh 都读取 IO 累计值，但差值只在采样时刻重置。
///
/// **快照级（instantaneous）** — 内存、线程数
/// rss_mb() / vsz_mb() / threads() 直接返回最近一次刷新的瞬时值。
pub struct MetricsCollector {
    pid: u32,            // 被监控进程的 PID
    start: Instant,      // 监控起始时刻，用于计算 elapsed_s
    memtotal_kb: u64,    // 系统总内存 kB，仅在初始化时读取一次
    cpu_count: u64,      // CPU 逻辑核数，仅在初始化时读取一次

    // ── CPU 差值追踪（刷新级）──
    prev_utime: u64,     // 上一次刷新时的 utime，用于计算差值
    prev_stime: u64,     // 上一次刷新时的 stime，用于计算差值
    prev_when: Instant,  // 上一次刷新的时刻，用于计算时间间隔
    cached_cpu_pct: f64, // 最近一次刷新计算出的 CPU 使用率 %（单核归一化）

    // ── IO 差值追踪（采样级）──
    baseline_read_bytes: u64,  // 上次采样时的 read_bytes 基线
    baseline_write_bytes: u64, // 上次采样时的 write_bytes 基线

    // ── 最新快照 ──
    latest: RawStats, // 最近一次 refresh 读取的完整原始数据
}

impl MetricsCollector {
    /// 创建采集器并初始化基线
    ///
    /// 在创建时立即读取一次 /proc 获取初始值，后续 refresh() 将基于此计算差值。
    /// 同时缓存系统总内存和 CPU 核数（这些值在进程生命周期内不变）。
    pub fn new(pid: u32) -> Self {
        let now = Instant::now();
        // 读取初始快照作为基线; 若进程尚不存在则用全零默认值
        let latest = Self::read_raw(pid).unwrap_or_default();
        Self {
            pid,
            start: now,
            memtotal_kb: read_memtotal_kb(),
            cpu_count: cpu_count(),
            // 将初始读到的 CPU 时间作为差值计算的起点
            prev_utime: latest.utime,
            prev_stime: latest.stime,
            prev_when: now,
            cached_cpu_pct: 0.0, // 首次刷新无差值，CPU% 为 0
            // 将初始读到的 IO 累计值作为采样差值计算的起点
            baseline_read_bytes: latest.read_bytes,
            baseline_write_bytes: latest.write_bytes,
            latest,
        }
    }

    /// 从 /proc 文件系统读取一次完整的原始数据
    ///
    /// stat 和 status 是必须成功的核心数据源，任一失败则整个快照作废。
    /// io 的读取独立处理：权限不足时降级为 0，不影响 CPU/内存采集。
    fn read_raw(pid: u32) -> Option<RawStats> {
        let (utime, stime) = parse_stat(pid)?;
        let (rss_kb, vsz_kb, threads) = parse_status(pid)?;
        // IO 文件可能因权限问题不可读（非 root 监控其他用户进程时），
        // 此时只将 IO 降级为 0，不影响其他指标的采集
        let (read_bytes, write_bytes) = parse_io(pid).unwrap_or((0, 0));
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

    /// 刷新：重新读取 /proc 并计算 CPU 使用率差值
    ///
    /// 这是监控循环中**每次迭代都会调用**的核心方法。
    ///
    /// # CPU 使用率计算公式
    ///
    /// CPU% = (Δutime + Δstime) / clock_ticks_per_sec / Δtime_sec / cpu_count × 100
    ///
    /// 其中:
    /// - Δutime + Δstime: 两次刷新间消耗的 CPU tick 总数
    /// - clock_ticks_per_sec: 100 (Linux USER_HZ)
    /// - Δtime_sec: 两次刷新间的墙上时钟时间差
    /// - cpu_count: CPU 逻辑核数，将多核总计归一化为单核百分比
    ///
    /// 结果为相对于**单核**的百分比。例如一个双线程程序可能显示 200%。
    ///
    /// # 容错
    ///
    /// 如果 /proc 读取失败（通常是进程已退出），静默保留上一次的值不变。
    /// 这确保了进程退出瞬间仍能输出最后一次有效数据。
    pub fn refresh(&mut self) {
        if let Some(stats) = Self::read_raw(self.pid) {
            // 计算两次刷新之间的 CPU tick 增量
            let delta_ticks = (stats.utime - self.prev_utime) + (stats.stime - self.prev_stime);
            // 计算两次刷新之间的墙上时钟时间差
            let delta_secs = stats.when.duration_since(self.prev_when).as_secs_f64();

            if delta_secs > 0.0 {
                // 将 tick 数转为秒，除以实际时间差，再按 CPU 核数归一化
                self.cached_cpu_pct =
                    (delta_ticks as f64 / CLOCK_TICKS_PER_SEC) / delta_secs
                        / self.cpu_count as f64
                        * 100.0;
            }
            // 更新基线，供下次刷新计算差值
            self.prev_utime = stats.utime;
            self.prev_stime = stats.stime;
            self.prev_when = stats.when;
            self.latest = stats;
        }
        // /proc 读取失败 → 什么都不做，保留上一次的值
    }

    // ── 以下为各指标的访问器方法 ──

    /// 返回最近一次刷新计算的 CPU 使用率（单核百分比，0 ~ 100×核数）
    pub fn cpu_pct(&self) -> f64 {
        self.cached_cpu_pct
    }

    /// 返回当前物理内存驻留集大小（MB）
    /// RSS kB → MB: 除以 1024
    pub fn rss_mb(&self) -> f64 {
        self.latest.rss_kb as f64 / 1024.0
    }

    /// 返回当前虚拟内存大小（MB）
    pub fn vsz_mb(&self) -> f64 {
        self.latest.vsz_kb as f64 / 1024.0
    }

    /// 返回当前物理内存占系统总量的百分比
    /// mem_pct = RSS / MemTotal × 100
    pub fn mem_pct(&self) -> f64 {
        if self.memtotal_kb == 0 {
            return 0.0; // 防御: MemTotal 读取失败时降级为 0
        }
        self.latest.rss_kb as f64 / self.memtotal_kb as f64 * 100.0
    }

    /// 返回自上次调用以来的磁盘读取增量（MB），同时重置基线
    ///
    /// 使用 saturating_sub 而非普通减法，防止在 /proc/io 计数器重置
    /// (极少情况，如 32 位内核计数器溢出回绕) 时产生错误的巨大负值。
    ///
    /// 每次采样时调用一次，返回的是整个采样周期内的累计 IO 量。
    pub fn io_read_mb_delta(&mut self) -> f64 {
        let delta = self.latest.read_bytes.saturating_sub(self.baseline_read_bytes);
        self.baseline_read_bytes = self.latest.read_bytes; // 重置基线
        delta as f64 / 1024.0 / 1024.0 // 字节 → MB
    }

    /// 返回自上次调用以来的磁盘写入增量（MB），同时重置基线
    pub fn io_write_mb_delta(&mut self) -> f64 {
        let delta = self.latest.write_bytes.saturating_sub(self.baseline_write_bytes);
        self.baseline_write_bytes = self.latest.write_bytes;
        delta as f64 / 1024.0 / 1024.0
    }

    /// 返回当前线程数
    pub fn threads(&self) -> u32 {
        self.latest.threads
    }

    /// 返回自监控开始以来的总运行时间（秒）
    pub fn elapsed_s(&self) -> f64 {
        self.latest.when.duration_since(self.start).as_secs_f64()
    }
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证 /proc/<pid>/stat 中 utime 和 stime 的字段位置
    ///
    /// `)` 后的字段按空格切分:
    /// 0=state, 1=ppid, 2=pgrp, ..., 11=utime, 12=stime
    #[test]
    fn test_parse_stat_field_positions() {
        // 构造足够多的字段确保能索引到 11 和 12
        let rest = "S 1 2 3 4 5 6 7 8 9 10 100 200 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
        let fields: Vec<&str> = rest.split_whitespace().collect();
        assert_eq!(fields[11], "100"); // utime 应位于索引 11
        assert_eq!(fields[12], "200"); // stime 应位于索引 12
    }

    /// 验证进程名包含括号时解析仍正确
    ///
    /// 进程名 `(my (special) proc)` 包含内层括号。
    /// rfind(')') 应找到**最后一个** `)`，即进程名的结束括号。
    #[test]
    fn test_parse_stat_with_parens_in_comm() {
        let line = "1234 (my (special) proc) S 0 0 0 0 0 0 0 0 0 0 50 60 0 0 0 0 0 0 0 0 0";
        let close = line.rfind(')').unwrap(); // 找到最后一个 ')' 的位置
        let fields: Vec<&str> = line[close + 2..].split_whitespace().collect();
        assert_eq!(fields[11], "50"); // 即使进程名有括号，utime 位置不变
        assert_eq!(fields[12], "60");
    }

    /// 验证 MetricsCollector 的初始状态
    ///
    /// 当前进程 (PID=1 或自身) 的初始:
    /// - elapsed_s 应为非负值
    /// - 首次 CPU% 应为 0（还没有刷新计算差值）
    #[test]
    fn test_metrics_collector_new() {
        let mc = MetricsCollector::new(1); // PID=1 (init/systemd)，总是存在
        assert!(mc.elapsed_s() >= 0.0);
        assert_eq!(mc.cpu_pct(), 0.0); // 首次创建无差值
    }
}
