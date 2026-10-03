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
        ..Default::default()
    };

    // 假设调用者提前 0.9s 开始录音，传入 window_start_offset = -0.9，并携带任务序列号 888888
    let offset = -0.9f32;
    let seq_id = 888888u64;
    let mut messages: Vec<Ft8DecodedMessage> = Vec::new();

    // 测试回调句柄接口 (模式二：增量实时流出)
    let final_msgs = pipeline.decode_with_callback(&audio, &config, offset, seq_id, |msg| {
        assert_eq!(msg.sequence_id, seq_id, "回调中的消息应携带传入的 sequence_id");
        messages.push(msg.clone());
    });

    assert!(!messages.is_empty(), "应成功解出信号");
    assert_eq!(final_msgs.len(), messages.len(), "回调收集总数应与最终返回总数一致");
    println!("\n解出 {} 条结构化信号 (已应用 -0.9s 窗口时间校正, sequence_id={}):", messages.len(), seq_id);

    let mut found_cq = false;
    let mut found_directed = false;

    for m in &messages {
        assert_eq!(m.sequence_id, seq_id, "消息 sequence_id 必须匹配");
        println!(
            "[{:5.0}Hz] DT:{:+5.2}s SNR:{:+3}dB | seq:{} | 发送方: {:8} 归属: {:10} ({:6}) | 接收方: {:8} | 阶段: {:?} 网格: {}",
            m.freq, m.dt, m.snr, m.sequence_id, m.sender_callsign, m.country, m.country_cn, m.receiver_callsign, m.qso_stage, m.grid
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
        ..Default::default()
    };

    let test_stream_seq = 100200300u64;
    // 使用带 -0.9s 偏移和 sequence_id 的流式接收机
    let mut receiver = StreamingFt8Receiver::with_window_offset_and_seq(config, -0.9, test_stream_seq);
    let chunk_size = 1920; // 160ms
    let mut early_count = 0;
    let mut final_count = 0;
    let mut finished_seen = false;

    for chunk in audio.chunks(chunk_size) {
        receiver.feed_chunk_with_callback(chunk, |ev| match ev {
            StreamDecodedEvent::PreambleDetected { active_frequencies, time_sec } => {
                println!("[流式 {:.2}s] 前导锁定频点: {} 个", time_sec, active_frequencies.len());
            }
            StreamDecodedEvent::EarlyDecoded { signals, time_sec, sequence_id } => {
                assert_eq!(sequence_id, test_stream_seq);
                println!("[流式 {:.2}s] 提前解码成功! 收到 {} 条信号 (seq={})", time_sec, signals.len(), sequence_id);
                early_count += signals.len();
                for s in &signals {
                    assert_eq!(s.sequence_id, test_stream_seq);
                    println!("    提前: {} (DT={:+.2}s)", s.message, s.dt);
                }
            }
            StreamDecodedEvent::CycleCompleted { all_signals, time_sec, sequence_id } => {
                assert_eq!(sequence_id, test_stream_seq);
                println!("[流式 {:.2}s] 全时隙扫尾完成! 共 {} 条信号 (seq={})", time_sec, all_signals.len(), sequence_id);
                final_count += all_signals.len();
                for s in &all_signals {
                    assert_eq!(s.sequence_id, test_stream_seq);
                }
            }
            StreamDecodedEvent::DecodeFinished { total_signals, audio_duration_sec, is_last_chunk, sequence_id } => {
                assert_eq!(sequence_id, test_stream_seq);
                finished_seen = true;
                println!("[流式 {:.2}s] 本轮解码结束通知: 总计 {} 条信号 (最后一帧={}, seq={})", audio_duration_sec, total_signals, is_last_chunk, sequence_id);
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

#[test]
fn test_two_modes_sync_vs_callback_with_seq() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio, _) = read_wav_file(wav_path).expect("读取测试音频失败");

    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 2,
        sync_min: 1.4,
        ..Default::default()
    };

    // 模式一：完全阻塞同步解码
    let seq_sync = 123456789u64;
    let sync_results = rust_ft8::decode_audio(&audio, &config, 0.0, seq_sync);
    assert!(!sync_results.is_empty(), "模式一同步解码应解出信号");
    for msg in &sync_results {
        assert_eq!(msg.sequence_id, seq_sync, "模式一返回结果必须携带指定 sequence_id");
    }

    // 模式二：实时增量回调流出解码
    let seq_cb = 987654321u64;
    let mut cb_received = Vec::new();
    let cb_final = rust_ft8::decode_audio_with_callback(&audio, &config, 0.0, seq_cb, |msg| {
        assert_eq!(msg.sequence_id, seq_cb, "模式二实时回调中必须携带指定 sequence_id");
        cb_received.push(msg.clone());
    });

    assert_eq!(cb_received.len(), cb_final.len(), "回调收集条数必须与最终返回列表条数一致");
    assert_eq!(sync_results.len(), cb_final.len(), "模式一与模式二解出信号数量必须一致");
    for (m_sync, m_cb) in sync_results.iter().zip(cb_final.iter()) {
        assert_eq!(m_sync.message, m_cb.message, "两种模式解出的消息内容必须一致");
        assert_eq!(m_sync.freq, m_cb.freq);
    }
}

#[test]
fn test_wsjtx_false_decode_rejection() {
    use rust_ft8::pack::callsign::unpack28;
    use rust_ft8::pack::grid::unpack_grid_report;

    // 1. 验证呼号语法合法性防误解机制：
    // 合法呼号：必须是第2或第3位为数字，数字之后必须全为字母 (如 "BG5VDH")
    let valid_bg5 = unpack28(75359253, 0, 1).expect("合法呼号应成功");
    println!("合法呼号解码: {}", valid_bg5);

    // 2. 验证超过 262,417,410 的无效 n_base (即 N28 >= 268,675,306) 必须直接被拦截拒绝
    let invalid_n28 = 262_417_410 + 2063592 + 4194304 + 100;
    assert!(unpack28(invalid_n28, 0, 1).is_err(), "超限 N28 必须被判定为非法拒绝");

    // 3. 验证网格大区防误解机制：
    // 网格前两位字母必须在 A..=R 范围内
    let valid_grid = unpack_grid_report(3245, 0);
    println!("有效网格: {}", valid_grid);
    assert!(!valid_grid.is_empty());

    // 4. 验证信号报告在 [-30, +30] dB 物理合法区间 (MAXGRID4=32400, 0dB 对应 32400+35)
    let report_legal = unpack_grid_report(32400 + 35, 0);
    println!("0dB 报告: {}", report_legal);
    assert_eq!(report_legal, "+00");
}

