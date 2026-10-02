use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};

#[test]
fn test_snr_comparison_websdr_test1() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio, _sr) = read_wav_file(wav_path).expect("读取 WAV 失败");

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
    };

    let decodes = pipeline.decode(&audio, &config);

    // 读取官方参考结果
    let txt_path = "reference/ft8_lib/test/wav/websdr_test1.txt";
    let official_txt = std::fs::read_to_string(txt_path).unwrap_or_default();
    let mut official_map = Vec::new();
    for line in official_txt.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 6 && parts[4] == "~" {
            let snr: i32 = parts[1].parse().unwrap_or(0);
            let freq: f32 = parts[3].parse().unwrap_or(0.0);
            let msg = parts[5..parts.len().min(8)].join(" ");
            official_map.push((freq, snr, msg));
        }
    }

    println!("\n==========================================================================================");
    println!("             websdr_test1.wav 解码消息 SNR 与 WSJT-X 官方对照分析");
    println!("==========================================================================================");
    println!(
        "| {:<5} | {:<25} | {:<12} | {:<12} | {:<8} |",
        "频点", "解码消息", "WSJT-X 官方SNR", "Rust 当前SNR", "偏差 (dB)"
    );
    println!("|-------|---------------------------|--------------|--------------|----------|");

    let mut abs_diff_sum = 0;
    let mut count = 0;
    for sig in &decodes {
        let matched = official_map.iter().find(|(ofreq, _, omsg)| {
            (ofreq - sig.freq).abs() < 10.0 || omsg.contains(&sig.message) || sig.message.contains(omsg.as_str())
        });

        if let Some((_, osnr, _)) = matched {
            let diff = sig.snr - osnr;
            abs_diff_sum += diff.abs();
            count += 1;
            println!(
                "| {:<5.1} | {:<25} | {:<12} | {:<12} | {:<+8} |",
                sig.freq, sig.message, osnr, sig.snr, diff
            );
        } else {
            println!(
                "| {:<5.1} | {:<25} | {:<12} | {:<12} | {:<8} |",
                sig.freq, sig.message, "N/A", sig.snr, "-"
            );
        }
    }
    println!("==========================================================================================");
    if count > 0 {
        println!("平均绝对误差: {:.2} dB (共 {} 条匹配)", (abs_diff_sum as f32) / (count as f32), count);
    }
    println!("==========================================================================================\n");
}
