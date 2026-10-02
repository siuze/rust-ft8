//! FT8 命令行编码与音频生成工具 (对标 WSJT-X ft8code / gen_ft8)
//!
//! 用法:
//!   ft8_encode "<message>" <output_wav> [--freq 1000.0] [--sample-rate 12000]

use rust_ft8::modulate::{encode_message_to_tones, synth_ft8_audio, write_wav_file};
use std::env;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("用法: ft8_encode \"<message>\" <output_wav> [选项]");
        eprintln!("示例: ft8_encode \"CQ BD4SUR OM99\" test.wav --freq 1000.0");
        eprintln!("选项:");
        eprintln!("  --freq <Hz>         基准载波音频频率 (默认: 1000.0 Hz)");
        eprintln!("  --sample-rate <Hz>  音频采样率 (默认: 12000 Hz)");
        std::process::exit(1);
    }

    let message = &args[1];
    let output_path = &args[2];
    let mut freq = 1000.0f32;
    let mut sample_rate = 12000u32;

    let mut i = 3;
    while i < args.len() {
        match args[i].as_str() {
            "--freq" => {
                if i + 1 < args.len() {
                    freq = args[i + 1].parse().unwrap_or(freq);
                    i += 1;
                }
            }
            "--sample-rate" => {
                if i + 1 < args.len() {
                    sample_rate = args[i + 1].parse().unwrap_or(sample_rate);
                    i += 1;
                }
            }
            _ => {
                eprintln!("未知选项: {}", args[i]);
            }
        }
        i += 1;
    }

    println!("-------------------------------------------------------");
    println!("FT8 纯 Rust 编码器");
    println!("待编码消息: '{}'", message);
    println!("载波频率:   {:.1} Hz", freq);
    println!("采样率:     {} Hz", sample_rate);

    // 1. 消息打包、CRC 计算、LDPC(174, 91) 编码与 Costas 同步序列插入
    let tones = match encode_message_to_tones(message) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("错误: 无法编码消息: {}", e);
            std::process::exit(1);
        }
    };

    print!("79 音调序列: ");
    for t in &tones {
        print!("{}", t);
    }
    println!();

    // 2. GFSK 连续相位音频波形合成 (15 秒完整时隙)
    let audio = synth_ft8_audio(&tones, freq, sample_rate as usize, 0.5, true);
    println!("音频生成成功: 采样点数 = {} ({:.2}s)", audio.len(), (audio.len() as f32) / (sample_rate as f32));

    // 3. 写入 16-bit PCM WAV 文件
    if let Err(e) = write_wav_file(output_path, &audio, sample_rate) {
        eprintln!("错误: 写入 WAV 失败: {}", e);
        std::process::exit(1);
    }

    println!("WAV 文件已写入: {}", output_path);
    println!("-------------------------------------------------------");
}
