use chrono::{DateTime, Local};
use std::io::Write;

/// CSV 文件表头行
///
/// 九个字段依次为:
/// 1. timestamp   — 本地时间戳 (ISO-8601 毫秒精度)
/// 2. elapsed_s   — 自子进程启动至采样时刻的秒数
/// 3. cpu_pct     — CPU 使用率百分比 (单核归一化)
/// 4. rss_mb      — 物理内存驻留集 (MB)
/// 5. vsz_mb      — 虚拟内存大小 (MB)
/// 6. mem_pct     — 物理内存占系统总量百分比
/// 7. io_read_mb  — 采样周期内磁盘读取量 (MB)
/// 8. io_write_mb — 采样周期内磁盘写入量 (MB)
/// 9. threads     — 线程数
pub const CSV_HEADER: &str =
    "timestamp,elapsed_s,cpu_pct,rss_mb,vsz_mb,mem_pct,io_read_mb,io_write_mb,threads";

/// 一条采样数据行
///
/// 在每次采样时刻由 monitor 模块创建，包含当前时刻所有资源指标的快照。
/// 所有浮点字段在输出时由 write_row 控制格式精度。
pub struct SampleRow {
    pub timestamp: DateTime<Local>, // 本地时间戳
    pub elapsed_s: f64,             // 已运行秒数
    pub cpu_pct: f64,               // CPU 使用率 %
    pub rss_mb: f64,                // 物理内存 MB
    pub vsz_mb: f64,                // 虚拟内存 MB
    pub mem_pct: f64,               // 内存占比 %
    pub io_read_mb: f64,            // 磁盘读取增量 MB
    pub io_write_mb: f64,           // 磁盘写入增量 MB
    pub threads: u32,               // 线程数
}

/// 写入 CSV 表头行
///
/// 泛型参数 `W: Write` 使得该函数同时支持:
/// - 标准输出: `io::stdout()`
/// - 文件: `File::create(path)`
/// - 内存缓冲区: `Vec<u8>` (用于测试)
pub fn write_header<W: Write>(w: &mut W) -> std::io::Result<()> {
    writeln!(w, "{}", CSV_HEADER)
}

/// 写入一条 CSV 数据行
///
/// 格式精度说明:
/// - 时间戳: 毫秒精度 (%.3f)
/// - CPU/内存百分比类: 1 位小数 (.1)
/// - IO 量 (MB): 3 位小数 (.3)，因为多数场景下 IO 量级较小
/// - 线程数: 整数
pub fn write_row<W: Write>(w: &mut W, row: &SampleRow) -> std::io::Result<()> {
    writeln!(
        w,
        // 格式字符串: 依次输出 9 个字段，逗号分隔
        "{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.3},{:.3},{}",
        row.timestamp.format("%Y-%m-%dT%H:%M:%S%.3f"), // 如 2026-05-27T10:00:01.000
        row.elapsed_s,
        row.cpu_pct,
        row.rss_mb,
        row.vsz_mb,
        row.mem_pct,
        row.io_read_mb,
        row.io_write_mb,
        row.threads,
    )
}

// ── 单元测试 ──

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证表头包含所有必要字段名
    #[test]
    fn test_write_header() {
        let mut buf = Vec::new(); // 内存缓冲区，不产生实际文件
        write_header(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("timestamp,")); // 第一个字段
        assert!(s.contains("cpu_pct"));       // 关键字段存在
        assert!(s.contains("threads"));       // 最后一个字段存在
    }

    /// 验证每条数据行恰好包含 9 列，与表头匹配
    #[test]
    fn test_write_row_has_nine_columns() {
        let row = SampleRow {
            timestamp: Local::now(),
            elapsed_s: 1.5,
            cpu_pct: 50.0,
            rss_mb: 100.0,
            vsz_mb: 200.0,
            mem_pct: 2.5,
            io_read_mb: 0.5,
            io_write_mb: 0.1,
            threads: 4,
        };
        let mut buf = Vec::new();
        write_row(&mut buf, &row).unwrap();
        let s = String::from_utf8(buf).unwrap();
        // 去尾部换行后按逗号切分，应有 9 列
        assert_eq!(s.trim().split(',').count(), 9);
    }
}
