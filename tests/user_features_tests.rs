//! 针对用户三大需求新特性的全面测试验证套件
//! 1. 编码器 API (生成音调序列与合成音频波形，带 HASH 码处理)
//! 2. 结构化解码器 API (正常解码与流式解码、回调句柄、9项完整字段提取、-0.9s 窗口偏移真实 DT 校正)
//! 3. DXCC 呼号国家地区查询、英文转中文、经纬度与大圆距离计算

use rust_ft8::demodulate::{
    read_wav_file, DecoderConfig, Ft8DecodedMessage, Ft8Pipeline, QsoStage,
    StreamDecodedEvent, StreamingFt8Receiver,
};
use rust_ft8::dxcc::{
    country_en_to_cn, great_circle_bearing, great_circle_distance, grid_distance, grid_to_latlon,
    latlon_to_grid, lookup_callsign_country,
};
use rust_ft8::modulate::{encode_message_to_audio, encode_message_to_tones};

#[test]
fn test_encoder_tones_and_audio_api() {
    // 1. 标准消息编码
    let text1 = "CQ BG5VDH OL02";
    let tones1 = encode_message_to_tones(text1).expect("音调编码应成功");
    assert_eq!(tones1.len(), 79);
    for &t in &tones1 {
        assert!(t <= 7, "每个音调符号必须在 0..=7 之间");
    }

    // 2. 文本直接合成 12000 Hz 音频
    let audio1 = encode_message_to_audio(text1, 1000.0, 12000, 0.5, true).expect("音频合成应成功");
    assert_eq!(audio1.len(), 180000, "完整 15 秒应为 180000 采样点");

    // 3. 带 HASH 码的消息编码测试
    // A: 纯十六进制哈希码 <ED3C6B>
    let text_hash_hex = "<ED3C6B> VR2XYZ -10";
    let tones_hex = encode_message_to_tones(text_hash_hex).expect("纯十六进制哈希应编码成功");
    assert_eq!(tones_hex.len(), 79);

    // B: 复合呼号哈希码 <ED3C6B>[BG5VDH]
    let text_hash_bracket = "<ED3C6B>[BG5VDH] VR2XYZ -10";
    let tones_bracket = encode_message_to_tones(text_hash_bracket).expect("复合哈希呼号应编码成功");
    assert_eq!(tones_bracket.len(), 79);

    // C: 尖括号呼号 <BG5VDH>
    let text_hash_call = "<BG5VDH> VR2XYZ -10";
    let tones_call = encode_message_to_tones(text_hash_call).expect("尖括号呼号应编码成功");
    assert_eq!(tones_call.len(), 79);
}

#[test]
fn test_structured_decoder_with_window_offset() {
    let wav_path = if std::path::Path::new("tests/wav/websdr_test1.wav").exists() {
        "tests/wav/websdr_test1.wav"
    } else {
        "reference/ft8_lib/test/wav/websdr_test1.wav"
    };
    let (audio, sample_rate) = read_wav_file(wav_path).expect("读取测试音频失败");
    assert_eq!(sample_rate, 12000);

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 2,
        sync_min: 1.4,
    };

    // 假设调用者提前 0.9s 开始录音，传入 window_start_offset = -0.9
    let offset = -0.9f32;
    let mut messages: Vec<Ft8DecodedMessage> = Vec::new();

    // 测试回调句柄接口
    pipeline.decode_with_callback(&audio, &config, offset, |msg| {
        messages.push(msg);
    });

    assert!(!messages.is_empty(), "应成功解出信号");
    println!("\n解出 {} 条结构化信号 (已应用 -0.9s 窗口时间校正):", messages.len());

    let mut found_cq = false;
    let mut found_directed = false;

    for m in &messages {
        println!(
            "[{:5.0}Hz] DT:{:+5.2}s SNR:{:+3}dB | 发送方: {:8} 归属: {:10} ({:6}) | 接收方: {:8} | 阶段: {:?} 网格: {}",
            m.freq, m.dt, m.snr, m.sender_callsign, m.country, m.country_cn, m.receiver_callsign, m.qso_stage, m.grid
        );

        // 验证 9 项字段完整性
        assert!(!m.sender_callsign.is_empty(), "发送方呼号不应为空");
        if m.qso_stage == QsoStage::BroadcastCq {
            found_cq = true;
            assert_eq!(m.receiver_callsign, "", "CQ 广播接收方应留空");
        } else {
            found_directed = true;
        }

        // 验证国家查询
        if m.sender_callsign.starts_with("BG") || m.sender_callsign.starts_with("BD") || m.sender_callsign.starts_with("BA") {
            assert_eq!(m.country, "China");
            assert_eq!(m.country_cn, "中国");
        }
    }

    assert!(found_cq, "应检出 CQ 广播消息");
    assert!(found_directed, "应检出定向呼叫或报告消息");
}

#[test]
fn test_streaming_decoder_with_callback() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio, _) = read_wav_file(wav_path).expect("读取测试音频失败");

    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 1,
        sync_min: 1.4,
    };

    // 使用带 -0.9s 偏移的流式接收机
    let mut receiver = StreamingFt8Receiver::with_window_offset(config, -0.9);
    let chunk_size = 1920; // 160ms
    let mut early_count = 0;
    let mut final_count = 0;
    let mut finished_seen = false;

    for chunk in audio.chunks(chunk_size) {
        receiver.feed_chunk_with_callback(chunk, |ev| match ev {
            StreamDecodedEvent::PreambleDetected { active_frequencies, time_sec } => {
                println!("[流式 {:.2}s] 前导锁定频点: {} 个", time_sec, active_frequencies.len());
            }
            StreamDecodedEvent::EarlyDecoded { signals, time_sec } => {
                println!("[流式 {:.2}s] 提前解码成功! 收到 {} 条信号", time_sec, signals.len());
                early_count += signals.len();
                for s in &signals {
                    println!("    提前: {} (DT={:+.2}s)", s.message, s.dt);
                }
            }
            StreamDecodedEvent::CycleCompleted { all_signals, time_sec } => {
                println!("[流式 {:.2}s] 全时隙扫尾完成! 共 {} 条信号", time_sec, all_signals.len());
                final_count += all_signals.len();
            }
            StreamDecodedEvent::DecodeFinished { total_signals, audio_duration_sec, is_last_chunk } => {
                finished_seen = true;
                println!("[流式 {:.2}s] 本轮解码结束通知: 总计 {} 条信号 (最后一帧={})", audio_duration_sec, total_signals, is_last_chunk);
            }
        });
    }

    assert!(early_count > 0, "流式提前解码应至少解出 1 批信号");
    assert!(final_count > 0, "流式全时隙扫尾应产出信号");
    assert!(finished_seen, "流式解码器必须触发 DecodeFinished 事件");
}

#[test]
fn test_dxcc_translation_and_distance() {
    // 1. 呼号查询实体
    let e_china = lookup_callsign_country("BH4XYZ").expect("BH4XYZ 应为中国");
    assert_eq!(e_china.name_en, "China");
    assert_eq!(e_china.name_cn, "中国");

    let e_japan = lookup_callsign_country("JA1ABC").expect("JA1ABC 应为日本");
    assert_eq!(e_japan.name_en, "Japan");
    assert_eq!(e_japan.name_cn, "日本");

    let e_us = lookup_callsign_country("K1ABC/3").expect("K1ABC 应为美国");
    assert_eq!(e_us.name_en, "United States");
    assert_eq!(e_us.name_cn, "美国");

    // 2. 英文转中文
    assert_eq!(country_en_to_cn("China"), "中国");
    assert_eq!(country_en_to_cn("Hong Kong"), "中国香港");
    assert_eq!(country_en_to_cn("Japan"), "日本");
    assert_eq!(country_en_to_cn("Germany"), "德国");
    assert_eq!(country_en_to_cn("United States"), "美国");

    // 3. 网格与经纬度互转 (支持 4位、6位以及大小写完全混写)
    let (lat_bj, lon_bj) = grid_to_latlon("OM89").expect("OM89 解析失败");
    assert!((lat_bj - 39.5).abs() < 1.0);
    assert!((lon_bj - 117.0).abs() < 1.0);

    // 大小写混合测试
    let (lat_lower, lon_lower) = grid_to_latlon("om89").expect("小写 om89 解析失败");
    assert_eq!((lat_bj, lon_bj), (lat_lower, lon_lower));

    let (lat_6, lon_6) = grid_to_latlon("Om89aA").expect("大小写混合 6 位解析失败");
    assert!((lat_6 - lat_bj).abs() < 1.0);
    assert!((lon_6 - lon_bj).abs() < 1.0);

    let grid_back_4 = rust_ft8::dxcc::latlon_to_grid_4(lat_bj, lon_bj);
    assert_eq!(grid_back_4, "OM89");
    let grid_back_6 = latlon_to_grid(lat_bj, lon_bj);
    assert_eq!(&grid_back_6[..4], "OM89");

    // 4. 大圆距离计算 (北京 OM89 到 上海 PM01)
    let (lat_sh, lon_sh) = grid_to_latlon("pm01").expect("pm01 解析失败");
    let dist_latlon = great_circle_distance(lat_bj, lon_bj, lat_sh, lon_sh);
    let dist_grid = grid_distance("OM89", "PM01").expect("网格距离计算失败");
    assert!((dist_latlon - dist_grid).abs() < 1e-4);
    assert!(dist_latlon > 950.0 && dist_latlon < 1200.0, "两地距离约 1050km");

    // 5. 初始航向角计算 (北京到上海大致朝向东南约 140°~160°)
    let bearing = great_circle_bearing(lat_bj, lon_bj, lat_sh, lon_sh);
    assert!(bearing > 120.0 && bearing < 180.0, "实际航向: {}°", bearing);
}
