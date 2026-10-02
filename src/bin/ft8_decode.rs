//! FT8 命令行解码工具 (对标 WSJT-X jt9 / ft8_decode)
//!
//! 用法:
//!   ft8_decode <wav_file> [--passes 3] [--sync-min 1.4] [--nfa 100] [--nfb 3500]

use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};
use std::env;
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: ft8_decode <wav_file> [选项]");
        eprintln!("选项:");
        eprintln!("  --passes <N>        消减重搜轮数 (默认: 3)");
        eprintln!("  --sync-min <val>    同步检测门限 (默认: 1.4)");
        eprintln!("  --nfa <Hz>          最低搜索频率 (默认: 100.0)");
        eprintln!("  --nfb <Hz>          最高搜索频率 (默认: 3500.0)");
        std::process::exit(1);
    }

    let wav_path = &args[1];
    let mut config = DecoderConfig::default();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--passes" => {
                if i + 1 < args.len() {
                    config.passes = args[i + 1].parse().unwrap_or(config.passes);
                    i += 1;
                }
            }
            "--sync-min" => {
                if i + 1 < args.len() {
                    config.sync_min = args[i + 1].parse().unwrap_or(config.sync_min);
                    i += 1;
                }
            }
            "--nfa" => {
                if i + 1 < args.len() {
                    config.nfa = args[i + 1].parse().unwrap_or(config.nfa);
                    i += 1;
                }
            }
            "--nfb" => {
                if i + 1 < args.len() {
                    config.nfb = args[i + 1].parse().unwrap_or(config.nfb);
                    i += 1;
                }
            }
            _ => {
                eprintln!("未知选项: {}", args[i]);
            }
        }
        i += 1;
    }

    let (audio, sample_rate) = match read_wav_file(wav_path) {
        Ok(res) => res,
        Err(e) => {
            eprintln!("错误: 无法读取 WAV 文件 '{}': {}", wav_path, e);
            std::process::exit(1);
        }
    };

    if sample_rate != 12000 {
        eprintln!("警告: 音频采样率为 {} Hz (推荐 12000 Hz)", sample_rate);
    }

    let pipeline = Ft8Pipeline::new();
    let start = Instant::now();
    let decodes = pipeline.decode(&audio, &config);
    let elapsed = start.elapsed();

    for sig in &decodes {
        println!(
            "000000 {:+3} {:+4.1} {:4.0} ~  {}",
            sig.snr, sig.dt, sig.freq, sig.message
        );
    }

    eprintln!(
        "-------------------------------------------------------"
    );
    eprintln!(
        "解调完成: 耗时 {:.2}s，成功解码出 {} 条信号",
        elapsed.as_secs_f32(),
        decodes.len()
    );
}
