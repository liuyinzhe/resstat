// 引入三个核心模块
mod metrics;   // /proc 文件系统解析与资源指标计算
mod monitor;   // 定时采样循环，管理刷新/输出节奏
mod output;    // CSV 格式化输出

use clap::Parser;
use std::fs::File;
use std::io::{self, Write};
use std::process::{Command, ExitCode};

/// 命令行参数定义（使用 clap derive 宏自动生成解析器）
#[derive(Parser, Debug)]
#[command(name = "resstat", about = "Linux 进程资源采样工具 — 启动子进程并按间隔采集 CPU/内存/IO/线程数据，输出 CSV")]
struct Args {
    /// 采样/输出间隔，单位秒（默认 1 秒）
    /// 每隔该时长输出一行 CSV 记录
    #[arg(short = 's', long = "sample", default_value = "1")]
    sample: f64,

    /// 数据刷新/采集间隔，单位秒（默认 0.1 秒）
    /// 每隔该时长重新读取 /proc 文件系统，必须 ≤ 采样间隔
    #[arg(short = 'r', long = "refresh", default_value = "0.1")]
    refresh: f64,

    /// CSV 输出文件路径（默认输出到标准输出 stdout）
    #[arg(short = 'o', long = "output")]
    output: Option<String>,

    /// 要监控的命令及其参数，必须放在 -- 之后
    /// 例如: resstat -s 1 -r 0.1 -- sleep 10
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

/// 程序入口
///
/// 执行流程:
/// 1. 解析命令行参数
/// 2. 校验参数合法性（刷新间隔 > 0，采样间隔 ≥ 刷新间隔，命令不为空）
/// 3. 启动子进程
/// 4. 创建输出写入器（文件或标准输出）
/// 5. 进入监控循环，直至子进程退出
/// 6. 以子进程的退出码退出
fn main() -> std::io::Result<ExitCode> {
    // ---- 步骤 1: 解析命令行参数 ----
    let args = Args::parse();

    // ---- 步骤 2: 参数校验 ----
    // 刷新间隔必须大于 0，否则无法进行任何数据采集
    if args.refresh <= 0.0 {
        eprintln!("error: refresh interval must be > 0");
        return Ok(ExitCode::from(2)); // 退出码 2 表示参数错误
    }
    // 采样间隔必须 ≥ 刷新间隔，否则采样没有足够的数据支撑
    if args.sample < args.refresh {
        eprintln!(
            "error: sample interval ({}) must be >= refresh interval ({})",
            args.sample, args.refresh
        );
        return Ok(ExitCode::from(2));
    }

    // 分离命令名和命令参数
    let cmd_name = &args.command[0];
    let cmd_args = &args.command[1..];

    // ---- 步骤 3: 启动子进程 ----
    // 使用操作系统原生的进程创建机制，子进程与 resstat 并行运行
    let child = match Command::new(cmd_name).args(cmd_args).spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: failed to spawn '{}': {}", cmd_name, e);
            return Ok(ExitCode::from(1)); // 退出码 1 表示启动失败
        }
    };

    // 获取子进程的 PID，后续通过 /proc/<pid>/ 读取其资源使用数据
    let pid = child.id();

    // ---- 步骤 4: 创建输出写入器 ----
    // 使用 trait object 实现多态: 既可以写入文件也可以写入标准输出
    let mut writer: Box<dyn Write> = match &args.output {
        Some(path) => match File::create(path) {
            Ok(f) => Box::new(f),
            Err(e) => {
                eprintln!("error: cannot open output file '{}': {}", path, e);
                return Ok(ExitCode::from(1));
            }
        },
        None => Box::new(io::stdout()), // 默认输出到标准输出
    };

    // ---- 步骤 5: 进入监控循环 ----
    // run_monitor 会持续运行直到子进程退出，返回子进程的退出码
    let exit_code = monitor::run_monitor(child, args.sample, args.refresh, &mut writer, pid)?;

    // ---- 步骤 6: 以子进程的退出码退出 ----
    // 透传退出码，使 resstat 的行为对调用者透明
    Ok(ExitCode::from(exit_code as u8))
}
