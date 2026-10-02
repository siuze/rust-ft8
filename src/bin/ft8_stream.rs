//! FT8 流式与提前解码命令行工具
//!
//! 模拟声卡边录音边解调，在 1.6s 预锁定频点，11.52s 提前解码输出强信号，并在发射结束即刻全量扫尾。
//! 用法:
//!   ft8_stream <wav_file> [--chunk-ms 160]

use rust_ft8::demodulate::{read_wav_file, DecoderConfig, StreamEvent, StreamingFt8Receiver};
use std::env;
use std::time::Instant;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("用法: ft8_stream <wav_file> [--chunk-ms 160]");
        std::process::exit(1);
    }

    let wav_path = &args[1];
    let mut chunk_ms = 160usize;

    let mut i = 2;
    while i < args.len() {
        if args[i] == "--chunk-ms" && i + 1 < args.len() {
            chunk_ms = args[i + 1].parse().unwrap_or(chunk_ms);
            i += 1;
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

    println!("==========================================================================");
    println!("             FT8 流式边收边解与提前解码实测回放 ({})", wav_path);
    println!("==========================================================================");

    let config = DecoderConfig::default();
    let mut receiver = StreamingFt8Receiver::new(config);
    let chunk_size = (sample_rate as usize * chunk_ms) / 1000;

    let total_start = Instant::now();
    let mut total_proc_time = std::time::Duration::ZERO;

    for (chunk_idx, chunk) in audio.chunks(chunk_size).enumerate() {
        let stream_time = (chunk_idx * chunk_size) as f32 / sample_rate as f32;

        let t_chunk_start = Instant::now();
        let events = receiver.feed_chunk(chunk);
        total_proc_time += t_chunk_start.elapsed();

        for event in events {
            match event {
                StreamEvent::PreambleDetected { active_frequencies } => {
                    let display_freqs: Vec<u32> = active_frequencies.iter().take(8).map(|&f| f.round() as u32).collect();
                    println!(
                        "[{:5.2}s] [事件 1: 前导码锁定] 快速初筛检测到 {} 个活跃载波频点: {:?}",
                        stream_time, active_frequencies.len(), display_freqs
                    );
                }
                StreamEvent::EarlyDecoded(signals) => {
                    println!(
                        "[{:5.2}s] [事件 2: 提前解码成功!] 距窗口结束还有 {:.2}s，第一批强信号成功出炉 ({} 条):",
                        stream_time, 15.0 - stream_time, signals.len()
                    );
                    for sig in &signals {
                        println!(
                            "         {:4.0} Hz  SNR:{:+3} dB  DT:{:+5.2}s  ~  {}",
                            sig.freq, sig.snr, sig.dt, sig.message
                        );
                    }
                }
                StreamEvent::CycleCompleted(all_signals) => {
                    println!(
                        "[{:5.2}s] [事件 3: 全时隙扫尾完成!] 距窗口结束还有 {:.2}s，全量解码结果就绪 ({} 条):",
                        stream_time, 15.0 - stream_time, all_signals.len()
                    );
                    for sig in &all_signals {
                        println!(
                            "  [全量] {:4.0} Hz  SNR:{:+3} dB  DT:{:+5.2}s  ~  {}",
                            sig.freq, sig.snr, sig.dt, sig.message
                        );
                    }
                }
            }
        }
    }

    println!("==========================================================================");
    println!("回放总推流耗时: {:.2}s | 算法纯计算总开销: {:.2}s", total_start.elapsed().as_secs_f32(), total_proc_time.as_secs_f32());
    println!("==========================================================================");
}
