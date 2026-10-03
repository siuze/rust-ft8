use rust_ft8::demodulate::{read_wav_file, DecoderConfig, StreamEvent, StreamingFt8Receiver};
use std::time::Instant;

#[test]
fn test_streaming_early_decoding_websdr_test1() {
    let wav_path = if std::path::Path::new("tests/wav/websdr_test1.wav").exists() {
        "tests/wav/websdr_test1.wav"
    } else {
        "reference/ft8_lib/test/wav/websdr_test1.wav"
    };
    let (audio, sample_rate) = read_wav_file(wav_path).expect("读取 WAV 音频失败");
    assert_eq!(sample_rate, 12000);

    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 2,
        sync_min: 1.4,
    };

    let mut receiver = StreamingFt8Receiver::new(config);

    // 每次流式推入 1 个符号 (160ms，1920 个采样点)
    let chunk_size = 1920usize;
    let mut offset = 0usize;
    let total_samples = audio.len();

    let mut preamble_seen = false;
    let mut early_decodes = Vec::new();
    let mut final_decodes = Vec::new();

    let sim_start = Instant::now();

    println!("\n==========================================================================");
    println!("             FT8 流式边收边解与提前解码实测回放 (websdr_test1.wav)");
    println!("==========================================================================");

    while offset < total_samples {
        let end = (offset + chunk_size).min(total_samples);
        let chunk = &audio[offset..end];
        let current_time_sec = (offset as f32) / 12000.0;

        let events = receiver.feed_chunk(chunk);

        for ev in events {
            match ev {
                StreamEvent::PreambleDetected { active_frequencies } => {
                    preamble_seen = true;
                    println!(
                        "[{:5.2}s] [事件 1: 前导码锁定] 检测到 {} 个活跃载波频点: {:?}",
                        current_time_sec,
                        active_frequencies.len(),
                        active_freqs_summary(&active_frequencies)
                    );
                }
                StreamEvent::EarlyDecoded(signals) => {
                    early_decodes = signals.clone();
                    println!(
                        "[{:5.2}s] [事件 2: 提前解码成功!] 距窗口结束还有 {:.2}s，第一批强信号成功输出 ({} 条):",
                        current_time_sec,
                        15.0 - current_time_sec,
                        signals.len()
                    );
                    for s in &signals {
                        println!("          {:4.0} Hz  SNR:{:+3} dB  DT:{:+.2}s  ~  {}", s.freq, s.snr, s.dt, s.message);
                    }
                }
                StreamEvent::CycleCompleted(signals) => {
                    final_decodes = signals.clone();
                    println!(
                        "[{:5.2}s] [事件 3: 全时隙扫尾完成!] 距窗口结束还有 {:.2}s，全量解码结果就绪 ({} 条):",
                        current_time_sec,
                        15.0 - current_time_sec,
                        signals.len()
                    );
                }
                StreamEvent::DecodeFinished { total_signals, audio_duration_sec, is_last_chunk } => {
                    println!(
                        "[{:5.2}s] [事件 4: 本轮解码完全结束] 录制时长={:.2}s, 总解出={} 条信号, 主动标记最后一帧={}",
                        current_time_sec, audio_duration_sec, total_signals, is_last_chunk
                    );
                }
            }
        }

        offset += chunk_size;
    }

    let elapsed = sim_start.elapsed();
    println!("==========================================================================");
    println!("流式全过程总计算耗时: {:.2}s", elapsed.as_secs_f32());
    println!("提前解码输出信号数: {} 条", early_decodes.len());
    println!("全量最终输出信号数: {} 条", final_decodes.len());
    println!("==========================================================================\n");

    assert!(preamble_seen, "必须成功捕获前导码");
    assert!(early_decodes.len() >= 10, "提前解码期至少应解出 10 条强信号");
    assert!(final_decodes.len() >= 16, "全量输出至少应解出 16 条信号");
}

#[test]
fn test_streaming_finish_with_last_chunk() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio, _) = read_wav_file(wav_path).expect("读取 WAV 音频失败");

    let mut receiver = StreamingFt8Receiver::with_window_offset(DecoderConfig::default(), -0.9);

    // 仅模拟录制了前 13.0 秒 (156000 采样点)
    let slice_len = 156000.min(audio.len());
    let mut got_finish = false;
    let mut decoded_count = 0;

    // 传入 chunk，并且显式标记 is_last = true
    receiver.feed_chunk_with_callback_ext(&audio[..slice_len], true, |ev| {
        match ev {
            rust_ft8::demodulate::StreamDecodedEvent::CycleCompleted { all_signals, .. } => {
                decoded_count = all_signals.len();
            }
            rust_ft8::demodulate::StreamDecodedEvent::DecodeFinished { total_signals, is_last_chunk, .. } => {
                got_finish = true;
                assert!(is_last_chunk);
                assert_eq!(total_signals, decoded_count);
            }
            _ => {}
        }
    });

    assert!(got_finish, "必须收到 DecodeFinished 事件");
    assert!(decoded_count >= 16, "提前结束解码应能顺利解出至少 16 条信号");
    assert!(receiver.is_cycle_finished(), "接收器状态必须已完成");
}

fn active_freqs_summary(freqs: &[f32]) -> Vec<u32> {
    freqs.iter().take(6).map(|&f| f.round() as u32).collect()
}
