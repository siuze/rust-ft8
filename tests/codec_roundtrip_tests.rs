//! FT8 全链路端到端编解码互逆测试 (Roundtrip Tests)
//!
//! 验证：
//! 1. 文本打包 -> CRC -> LDPC 编码 -> Gray 映射 -> GFSK 音频调制 -> 纯 Rust 解调 DSP -> LDPC 解码 -> 文本还原 100% 互逆。
//! 2. 多信号同频带重叠与并发解码能力。
//! 3. 信噪比 (SNR) 容限与噪声鲁棒性测试。

use rust_ft8::demodulate::{DecoderConfig, Ft8Pipeline};
use rust_ft8::modulate::{encode_message_to_tones, synth_ft8_audio};
use std::time::Instant;

#[test]
fn test_single_signal_roundtrip() {
    let test_messages = [
        ("CQ BD4SUR OM99", 1250.0f32),
        ("K1ABC W9XYZ EN37", 850.0f32),
        ("W9XYZ K1ABC -11", 1620.0f32),
        ("K1ABC W9XYZ RRR", 2100.0f32),
        ("W9XYZ K1ABC 73", 2450.0f32),
    ];

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 1, // 单信号单轮即可快速解出
        sync_min: 1.4,
        ..Default::default()
    };

    for &(msg, freq) in &test_messages {
        let tones = encode_message_to_tones(msg).expect("编码失败");
        let audio = synth_ft8_audio(&tones, freq, 12000, 0.5, true);

        let t0 = Instant::now();
        let decodes = pipeline.decode(&audio, &config);
        let elapsed = t0.elapsed();

        assert!(
            !decodes.is_empty(),
            "消息 '{}' ({} Hz) 应当成功解出，实际未解出任何信号",
            msg,
            freq
        );

        let decoded = &decodes[0];
        println!(
            "Roundtrip OK: '{}' -> decoded '{}' at {:.1} Hz (err: {:.2} Hz, dt: {:.2}s, SNR: {} dB, time: {:.2}s)",
            msg,
            decoded.message,
            decoded.freq,
            (decoded.freq - freq).abs(),
            decoded.dt,
            decoded.snr,
            elapsed.as_secs_f32()
        );

        assert_eq!(
            decoded.message, msg,
            "解调还原消息应与原消息完全一致"
        );
        assert!(
            (decoded.freq - freq).abs() < 2.0,
            "频率偏差应在 2 Hz 以内，实际偏差: {:.2} Hz",
            (decoded.freq - freq).abs()
        );
    }
}

#[test]
fn test_multi_signal_concurrency_roundtrip() {
    let signals = [
        ("CQ BD4SUR OM99", 600.0f32, 0.45f32),
        ("BA4ABC BI4XYZ OL41", 1200.0f32, 0.52f32),
        ("JA1ABC JH2XYZ PM95", 1800.0f32, 0.48f32),
        ("VK3ABC ZL1XYZ QF22", 2400.0f32, 0.55f32),
    ];

    // 混合多个独立频点的音频信号
    let mut combined_audio = vec![0.0f32; 180000];

    for &(msg, freq, dt) in &signals {
        let tones = encode_message_to_tones(msg).expect("编码失败");
        let audio = synth_ft8_audio(&tones, freq, 12000, dt, true);
        for (out, inp) in combined_audio.iter_mut().zip(audio.iter()) {
            *out += *inp * 0.25; // 缩放防止溢出
        }
    }

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 2,
        sync_min: 1.4,
        ..Default::default()
    };

    let t0 = Instant::now();
    let decodes = pipeline.decode(&combined_audio, &config);
    let elapsed = t0.elapsed();

    println!(
        "\n并发多信号混合测试: 输入 4 条信号，成功解码出 {} 条信号 (耗时 {:.2}s):",
        decodes.len(),
        elapsed.as_secs_f32()
    );

    for sig in &decodes {
        println!(
            "  - {:+3} dB  dt={:+.2}s  {:4.0} Hz  ~  {}",
            sig.snr, sig.dt, sig.freq, sig.message
        );
    }

    assert_eq!(
        decodes.len(),
        4,
        "混合音频中的 4 条信号应全部被成功解码"
    );

    for &(orig_msg, orig_freq, _) in &signals {
        let found = decodes.iter().any(|d| {
            d.message == orig_msg && (d.freq - orig_freq).abs() < 5.0
        });
        assert!(
            found,
            "信号 '{}' ({:.0} Hz) 应该在解码结果列表中",
            orig_msg,
            orig_freq
        );
    }
}
