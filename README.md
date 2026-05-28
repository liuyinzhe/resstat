# resstat

Linux process resource sampler — spawns a command and samples its CPU, memory, IO, and thread usage at configurable intervals, outputting CSV.

## Build

```bash
cargo build --release
```

Binary: `./target/release/resstat`

## Usage

```
resstat [-s <seconds>] [-r <seconds>] [-o <file>] -- <command> [args...]
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `-s, --sample` | `1` | Sample/output interval in seconds |
| `-r, --refresh` | `0.1` | Data collection interval in seconds (must be ≤ sample) |
| `-o, --output` | stdout | Output file path |

### Examples

```bash
# Baseline: sample sleep every second, refresh /proc every 100ms
resstat -- sleep 10

# Custom intervals: sample every 2s, refresh every 500ms
resstat -s 2 -r 0.5 -- ffmpeg -i input.mp4 output.avi

# Save to file
resstat -s 1 -r 0.1 -o stats.csv -- python train.py
```

## Output

CSV sent to stdout (or `-o <file>`):

```
timestamp,elapsed_s,cpu_pct,rss_mb,vsz_mb,mem_pct,io_read_mb,io_write_mb,threads
2026-05-27T10:00:01.000,1.0,12.5,45.2,128.0,0.3,0.000,4.100,3
2026-05-27T10:00:02.000,2.0,15.1,46.0,128.5,0.4,0.100,4.100,3
```

| Column | Unit | Description |
|--------|------|-------------|
| `timestamp` | — | ISO-8601 wall clock at sample moment |
| `elapsed_s` | s | Seconds since child process start |
| `cpu_pct` | % | CPU usage as percentage of one core |
| `rss_mb` | MB | Resident Set Size |
| `vsz_mb` | MB | Virtual memory size |
| `mem_pct` | % | RSS / system total memory × 100 |
| `io_read_mb` | MB | Bytes read from storage (delta from previous sample) |
| `io_write_mb` | MB | Bytes written to storage (delta from previous sample) |
| `threads` | — | Thread count at sample moment |

Exit code mirrors the child process.

### How refresh and sample interact

- `--refresh` controls how often `/proc/<pid>/*` is polled. CPU% is computed as the delta between consecutive refresh reads.
- `--sample` controls how often a CSV row is emitted. The row captures the latest refresh snapshot. IO values are deltas accumulated across the sample interval.

## Platform

**Linux only.** Reads directly from `/proc/<pid>/{stat,status,io}` and `/proc/{meminfo,cpuinfo}`. No system dependencies beyond a standard Linux kernel with `/proc` mounted.

## License

MIT
