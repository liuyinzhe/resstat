use chrono::{DateTime, Local};
use std::io::Write;

pub const CSV_HEADER: &str = "timestamp,elapsed_s,cpu_pct,rss_mb,vsz_mb,mem_pct,io_read_mb,io_write_mb,threads";

pub struct SampleRow {
    pub timestamp: DateTime<Local>,
    pub elapsed_s: f64,
    pub cpu_pct: f64,
    pub rss_mb: f64,
    pub vsz_mb: f64,
    pub mem_pct: f64,
    pub io_read_mb: f64,
    pub io_write_mb: f64,
    pub threads: u32,
}

pub fn write_header<W: Write>(w: &mut W) -> std::io::Result<()> {
    writeln!(w, "{}", CSV_HEADER)
}

pub fn write_row<W: Write>(w: &mut W, row: &SampleRow) -> std::io::Result<()> {
    writeln!(
        w,
        "{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.3},{:.3},{}",
        row.timestamp.format("%Y-%m-%dT%H:%M:%S%.3f"),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_header() {
        let mut buf = Vec::new();
        write_header(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("timestamp,"));
        assert!(s.contains("cpu_pct"));
        assert!(s.contains("threads"));
    }

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
        assert_eq!(s.trim().split(',').count(), 9);
    }
}
