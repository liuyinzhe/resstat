use crate::metrics::MetricsCollector; // /proc 数据采集器，管理各指标的差值计算
use crate::output::{self, SampleRow}; // CSV 行结构体与格式化函数
use chrono::Local; // 本地时间戳
use std::io::Write;
use std::process::Child; // 子进程句柄，提供 try_wait() 非阻塞检查
use std::time::{Duration, Instant}; // 高精度计时

/// 运行主监控循环
///
/// # 参数
/// - `child`: 已启动的子进程句柄，用于检测进程退出和获取退出码
/// - `sample_interval`: 采样/输出间隔（秒），每隔该时长输出一行 CSV
/// - `refresh_interval`: 刷新/采集间隔（秒），每隔该时长重新读取 /proc
/// - `output`: CSV 输出目标（stdout 或文件）
/// - `pid`: 子进程 PID，用于拼装 /proc/<pid>/ 路径
///
/// # 返回值
/// 子进程的退出码；如果子进程没有退出码则返回 1
///
/// # 循环逻辑
/// 循环以 refresh_interval 为节奏运行，每次迭代做三件事:
/// 1. **刷新** — 读取 /proc 更新最新指标快照，计算 CPU% 差值
/// 2. **采样** — 如果距离上次采样已过 sample_interval，输出一行 CSV
/// 3. **检查退出** — 非阻塞检测子进程是否已退出，退出时输出最后一行并返回
///
/// 采样时刻锚定在循环启动时间点，避免因循环耗时而产生累积漂移。
pub fn run_monitor<W: Write>(
    mut child: Child,
    sample_interval: f64,
    refresh_interval: f64,
    output: &mut W,
    pid: u32,
) -> std::io::Result<i32> {
    // 将秒转为 Duration 类型，方便后续时间运算
    let sample_dur = Duration::from_secs_f64(sample_interval);
    let refresh_dur = Duration::from_secs_f64(refresh_interval);

    // 记录监控起始时刻，作为所有 elapsed 计算的基准
    let start = Instant::now();

    // 首次采样在 t = sample_interval 时触发（不在 t=0 采样，因为此时还没有有效差值）
    let mut next_sample_at = sample_dur;

    // 初始化数据采集器: 读取 /proc 获取基线值，计算系统总内存和 CPU 核数
    let mut collector = MetricsCollector::new(pid);

    // 写入 CSV 表头
    output::write_header(output)?;

    // 从采集器构建一行 CSV 采样数据的辅助闭包
    // 避免正常采样和进程退出时重复写相同的 9 字段构造代码
    let build_row = |collector: &mut MetricsCollector| SampleRow {
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

    // ---- 主循环 ----
    loop {
        // 记录本次循环开始时刻，用于后续精确计算 sleep 时长
        let loop_start = Instant::now();

        // === 步骤 1: 刷新 ===
        // 重新读取 /proc/<pid>/{stat,status,io}，计算自上次刷新以来的 CPU 使用率差值
        // 如果 /proc 读取失败（进程已退出等原因），保留上一次的值不变
        collector.refresh();

        // === 步骤 2: 采样输出 ===
        // 用 start.elapsed() 而非简单计数，确保采样时间锚定在起始点，不产生累积漂移
        if start.elapsed() >= next_sample_at {
            output::write_row(output, &build_row(&mut collector))?;
            // 立即 flush 确保数据写入底层设备（管道/文件），不会因缓冲延迟
            output.flush()?;

            // 推进下次采样时刻
            next_sample_at += sample_dur;
        }

        // === 步骤 3: 检查子进程是否退出 ===
        // try_wait() 是非阻塞调用:
        // - Ok(Some(status)) → 进程已退出，输出最后一行采样数据并返回退出码
        // - Ok(None)         → 进程仍在运行，继续循环
        // - Err(e)           → 系统调用失败（如权限问题），向上传播错误
        match child.try_wait() {
            Ok(Some(status)) => {
                // 子进程退出时输出最终采样行，确保不丢失最后一段数据
                output::write_row(output, &build_row(&mut collector))?;
                output.flush()?;
                // 透传子进程退出码；若无退出码（被信号杀死）则返回 1
                return Ok(status.code().unwrap_or(1));
            }
            Ok(None) => {} // 进程仍在运行，什么也不做
            Err(e) => return Err(e),
        }

        // === 步骤 4: 精确休眠 ===
        // 补偿本循环已消耗的时间，使实际循环周期精确等于 refresh_interval
        // 例如: refresh=100ms, 本循环消耗了 2ms → 休眠 98ms
        let work_time = loop_start.elapsed();
        if work_time < refresh_dur {
            std::thread::sleep(refresh_dur - work_time);
        }
        // 如果 work_time ≥ refresh_dur（系统负载高），不休眠，立即进入下一轮
    }
}
