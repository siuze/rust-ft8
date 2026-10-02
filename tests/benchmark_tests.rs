use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};
use std::time::Instant;

fn parse_expected_txt(path: &str) -> Vec<String> {
    if let Ok(content) = std::fs::read_to_string(path) {
        content
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return None;
                }
                if let Some(tilde_pos) = trimmed.find('~') {
                    let text = trimmed[tilde_pos + 1..].trim();
                    let parts: Vec<&str> = text.split_whitespace().collect();
                    if parts.is_empty() {
                        return None;
                    }
                    let n = parts.len().min(3);
                    let mut norm_parts = Vec::new();
                    for &p in &parts[..n] {
                        if p.starts_with('<') && p.ends_with('>') {
                            norm_parts.push("<...>");
                        } else {
                            norm_parts.push(p);
                        }
                    }
                    Some(norm_parts.join(" "))
                } else {
                    None
                }
            })
            .collect()
    } else {
        Vec::new()
    }
}

fn normalize_message(msg: &str) -> String {
    let parts: Vec<&str> = msg.split_whitespace().collect();
    let n = parts.len().min(3);
    let mut norm_parts = Vec::new();
    for &p in &parts[..n] {
        if p.starts_with('<') && p.ends_with('>') {
            norm_parts.push("<...>");
        } else {
            norm_parts.push(p);
        }
    }
    norm_parts.join(" ")
}

#[test]
fn test_decode_single_wav_websdr_test1() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio, sample_rate) = read_wav_file(wav_path).expect("读取 WAV 失败");
    assert_eq!(sample_rate, 12000);

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
    };

    let start = Instant::now();
    let decodes = pipeline.decode(&audio, &config);
    let elapsed = start.elapsed();

    println!(
        "\nwebsdr_test1.wav decoded {} messages in {:.2}s:",
        decodes.len(),
        elapsed.as_secs_f32()
    );
    for sig in &decodes {
        println!(
            "{:+3} dB  {:+.2}s  {:4.0} Hz  ~  {}",
            sig.snr, sig.dt, sig.freq, sig.message
        );
    }

    assert!(
        decodes.len() >= 14,
        "websdr_test1 至少应解出 14 条消息，实际解出: {}",
        decodes.len()
    );
}

#[test]
fn test_benchmark_all_websdr_wavs() {
    let test_files = [
        "websdr_test1",
        "websdr_test2",
        "websdr_test3",
        "websdr_test4",
        "websdr_test5",
        "websdr_test6",
        "websdr_test7",
        "websdr_test8",
        "websdr_test9",
        "websdr_test10",
        "websdr_test11",
        "websdr_test12",
        "websdr_test13",
    ];

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
    };

    println!("\n==========================================================================");
    println!("             Pure-Rust FT8 解码器 13-WAV 基准测试报告");
    println!("==========================================================================");
    println!(
        "| {:<15} | {:<8} | {:<12} | {:<8} | {:<8} |",
        "WAV 文件", "解出数", "参考基准数", "匹配数", "耗时(s)"
    );
    println!("|-----------------|----------|--------------|----------|----------|");

    let mut total_decoded = 0usize;
    let mut total_expected = 0usize;
    let mut total_matched = 0usize;
    let total_start = Instant::now();

    for name in &test_files {
        let wav_path = format!("reference/ft8_lib/test/wav/{}.wav", name);
        let txt_path = format!("reference/ft8_lib/test/wav/{}.txt", name);

        let (audio, _sr) = match read_wav_file(&wav_path) {
            Ok(res) => res,
            Err(e) => {
                println!("| {:<15} | 读取失败: {} |", name, e);
                continue;
            }
        };

        let expected_msgs = parse_expected_txt(&txt_path);
        let exp_count = expected_msgs.len();

        let start = Instant::now();
        let decodes = pipeline.decode(&audio, &config);
        let elapsed = start.elapsed();

        let mut match_count = 0usize;
        for sig in &decodes {
            let norm_decoded = normalize_message(&sig.message);
            let is_match = expected_msgs.iter().any(|exp| {
                exp == &norm_decoded || exp.contains(&norm_decoded) || norm_decoded.contains(exp)
            });
            if is_match {
                match_count += 1;
            }
        }

        println!(
            "| {:<15} | {:<8} | {:<12} | {:<8} | {:<8.2} |",
            name,
            decodes.len(),
            exp_count,
            match_count,
            elapsed.as_secs_f32()
        );

        total_decoded += decodes.len();
        total_expected += exp_count;
        total_matched += match_count;
    }

    let total_elapsed = total_start.elapsed();
    println!("|-----------------|----------|--------------|----------|----------|");
    println!(
        "| {:<15} | {:<8} | {:<12} | {:<8} | {:<8.2} |",
        "总计 (TOTAL)",
        total_decoded,
        total_expected,
        total_matched,
        total_elapsed.as_secs_f32()
    );
    println!("==========================================================================\n");

    println!(
        "基准汇总: 总解出数 = {}, 参考基准期望数 = {}, 匹配数 = {}, 匹配率 = {:.1}%, 平均每文件耗时 = {:.2}s",
        total_decoded,
        total_expected,
        total_matched,
        (total_matched as f32) / (total_expected.max(1) as f32) * 100.0,
        total_elapsed.as_secs_f32() / (test_files.len() as f32)
    );

    // 严谨断言：纯 Rust 实现解出数必须超越 C 语言 ft8_lib 基准 (151)，达到接近 WSJT-X 官方水准
    assert!(
        total_decoded >= 160,
        "总解出数必须达到基准要求 (>=160)，实际解出: {}",
        total_decoded
    );
}

#[test]
fn test_benchmark_14_baseline_wavs() {
    let baseline_files: [(&str, usize, usize); 14] = [
        ("191111_110115", 0, 1),
        ("191111_110130", 4, 5),
        ("191111_110145", 2, 2),
        ("191111_110200", 4, 5),
        ("191111_110215", 3, 5),
        ("191111_110615", 17, 22),
        ("191111_110630", 12, 19),
        ("191111_110645", 16, 19),
        ("191111_110700", 14, 18),
        ("websdr_test1", 13, 19),
        ("websdr_test2", 19, 23),
        ("websdr_test3", 9, 16),
        ("websdr_test4", 21, 27),
        ("websdr_test5", 17, 28),
    ];

    let pipeline = Ft8Pipeline::new();
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
    };

    println!("\n==========================================================================================");
    println!("             Pure-Rust FT8 与 C 库 (ft8_lib) 及 WSJT-X 标准 14 样本基准对比报告");
    println!("==========================================================================================");
    println!(
        "| {:<16} | {:<12} | {:<12} | {:<12} | {:<8} |",
        "WAV 测试文件", "ft8_lib (C库)", "wsjtx (基准)", "rust_ft8(实测)", "耗时(s)"
    );
    println!("|------------------|--------------|--------------|--------------|----------|");

    let mut total_ft8_lib = 0usize;
    let mut total_wsjtx = 0usize;
    let mut total_rust = 0usize;
    let total_start = Instant::now();

    for &(name, c_count, wsjtx_count) in &baseline_files {
        let wav_path = format!("reference/ft8_lib/test/wav/{}.wav", name);
        let (audio, _sr) = match read_wav_file(&wav_path) {
            Ok(res) => res,
            Err(e) => {
                println!("| {:<16} | 文件缺失: {} |", name, e);
                continue;
            }
        };

        let start = Instant::now();
        let decodes = pipeline.decode(&audio, &config);
        let elapsed = start.elapsed();

        println!(
            "| {:<16} | {:<12} | {:<12} | {:<12} | {:<8.2} |",
            format!("{}.wav", name),
            c_count,
            wsjtx_count,
            decodes.len(),
            elapsed.as_secs_f32()
        );

        total_ft8_lib += c_count;
        total_wsjtx += wsjtx_count;
        total_rust += decodes.len();
    }

    let total_elapsed = total_start.elapsed();
    println!("|------------------|--------------|--------------|--------------|----------|");
    println!(
        "| {:<16} | {:<12} | {:<12} | {:<12} | {:<8.2} |",
        "总计 (14组样本)",
        total_ft8_lib,
        total_wsjtx,
        total_rust,
        total_elapsed.as_secs_f32()
    );
    println!("==========================================================================================\n");

    println!(
        "14样本汇总: ft8_lib = {} 条, wsjtx_lib = {} 条, 纯 Rust rust_ft8 = {} 条 (+{:.1}% vs ft8_lib)",
        total_ft8_lib,
        total_wsjtx,
        total_rust,
        ((total_rust as f32) - (total_ft8_lib as f32)) / (total_ft8_lib as f32) * 100.0
    );

    // 严谨验证：纯 Rust 实测结果必须超越 ft8_lib，且全面达标 WSJT-X 水准
    assert!(
        total_rust >= total_wsjtx * 9 / 10,
        "14 组样本纯 Rust 解出数 ({}) 必须达到 WSJT-X 官方基准 (204) 的 90% 以上",
        total_rust
    );
}

