use crate::metrics::MetricsCollector;
use crate::output::{self, SampleRow};
use chrono::Local;
use std::io::Write;
use std::process::Child;
use std::time::{Duration, Instant};

/// Run the sampling loop. Takes spawned child, sample/refresh intervals, output writer, and child PID.
/// Returns the child's exit code, or 1 if the child had no exit code.
pub fn run_monitor<W: Write>(
    mut child: Child,
    sample_interval: f64,
    refresh_interval: f64,
    output: &mut W,
    pid: u32,
) -> std::io::Result<i32> {
    let sample_dur = Duration::from_secs_f64(sample_interval);
    let refresh_dur = Duration::from_secs_f64(refresh_interval);
    let start = Instant::now();
    // First sample fires at t=sample_interval (anchored to start for zero drift)
    let mut next_sample_at = sample_dur;
    let mut collector = MetricsCollector::new(pid);

    output::write_header(output)?;

    loop {
        let loop_start = Instant::now();

        // Refresh: re-read /proc, compute CPU% delta since last refresh
        collector.refresh();

        // Sample: emit CSV row if interval has elapsed
        if start.elapsed() >= next_sample_at {
            let row = SampleRow {
                timestamp: Local::now(),
                elapsed_s: collector.elapsed_s(),
                cpu_pct: collector.cpu_pct(),
                rss_mb: collector.rss_mb(),
                vsz_mb: collector.vsz_mb(),
                mem_pct: collector.mem_pct(),
                io_read_mb: collector.io_read_mb_delta(),
                io_write_mb: collector.io_write_mb_delta(),
                threads: collector.threads(),
            };
            output::write_row(output, &row)?;
            output.flush()?;
            next_sample_at += sample_dur;
        }

        // Check if child exited
        match child.try_wait() {
            Ok(Some(status)) => {
                // Emit final sample with latest data
                let row = SampleRow {
                    timestamp: Local::now(),
                    elapsed_s: collector.elapsed_s(),
                    cpu_pct: collector.cpu_pct(),
                    rss_mb: collector.rss_mb(),
                    vsz_mb: collector.vsz_mb(),
                    mem_pct: collector.mem_pct(),
                    io_read_mb: collector.io_read_mb_delta(),
                    io_write_mb: collector.io_write_mb_delta(),
                    threads: collector.threads(),
                };
                output::write_row(output, &row)?;
                output.flush()?;
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {} // still running
            Err(e) => return Err(e),
        }

        // Sleep for remainder of refresh interval
        let work_time = loop_start.elapsed();
        if work_time < refresh_dur {
            std::thread::sleep(refresh_dur - work_time);
        }
    }
}
