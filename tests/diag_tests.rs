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

#[test]
fn test_inspect_official_sbase_and_snr() {
    use rust_ft8::demodulate::BaselineEstimator;
    let wav_path = "reference/ft8_lib/test/wav/websdr_test1.wav";
    let (audio_norm, _sr) = read_wav_file(wav_path).expect("读取 WAV 失败");

    // 未归一化的原始 16 位整型浮点数音频 (范围 [-32768, 32767])
    let audio_raw: Vec<f32> = audio_norm.iter().map(|&x| x * 32768.0).collect();

    let estimator = BaselineEstimator::new();
    let baseline_raw = estimator.estimate_baseline(&audio_raw, 200.0, 4000.0);
    let baseline_norm = estimator.estimate_baseline(&audio_norm, 200.0, 4000.0);

    let ik_freq = 1109.4f32;
    let bin = (ik_freq / baseline_raw.df).round() as usize;

    println!("\n=== 底噪基准尺度诊断 (1109.4 Hz) ===");
    println!("原始量纲 (Raw PCM): sbase[bin]={:.2} dB, xbase={:.4e}", baseline_raw.sbase[bin], baseline_raw.get_xbase(ik_freq));
    println!("归一量纲 (Norm):    sbase[bin]={:.2} dB, xbase={:.4e}", baseline_norm.sbase[bin], baseline_norm.get_xbase(ik_freq));

    use rust_ft8::demodulate::Downsampler;
    let downsampler = Downsampler::new();
    let long_fft_raw = downsampler.compute_long_fft(&audio_raw);
    println!("\n==========================================================================================================");
    println!("               websdr_test1.wav 公式 1 (xnoi) vs 公式 2 (xbase/3e6) vs WSJT-X 官方对照");
    println!("==========================================================================================================");
    println!(
        "| {:<5} | {:<25} | {:<12} | {:<12} | {:<12} |",
        "频点", "解码消息", "WSJT-X 官方", "公式1 (xnoi)", "公式2 (xbase)"
    );
    println!("|-------|---------------------------|--------------|--------------|--------------|");

    let pipeline = rust_ft8::demodulate::Ft8Pipeline::new();
    let config = rust_ft8::demodulate::DecoderConfig::default();
    let decodes = pipeline.decode(&audio_norm, &config);

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

    use rustfft::FftPlanner;
    let mut planner = FftPlanner::<f32>::new();
    let fft32 = planner.plan_fft_forward(32);

    for sig in &decodes {
        let matched = official_map.iter().find(|(ofreq, _, omsg)| {
            (ofreq - sig.freq).abs() < 10.0 || omsg.contains(&sig.message) || sig.message.contains(omsg.as_str())
        });
        let osnr_str = if let Some((_, osnr, _)) = matched { format!("{:+3} dB", osnr) } else { "N/A".to_string() };

        let xbase_raw = baseline_raw.get_xbase(sig.freq);

        // 重算该信号在 audio_raw 下的 xsig 和 xnoi
        let cd0_sig = downsampler.downsample(&long_fft_raw, sig.freq);
        let ibest_idx = (((sig.dt + 0.5) * 200.0).round() as usize).min(cd0_sig.len().saturating_sub(32 * 79));

        let mut xsig = 0.0f32;
        let mut xnoi = 0.0f32;
        for k in 0..79 {
            let i1 = ibest_idx + k * 32;
            let mut buf = [num_complex::Complex32::new(0.0, 0.0); 32];
            if i1 + 32 <= cd0_sig.len() {
                buf.copy_from_slice(&cd0_sig[i1..i1 + 32]);
            }
            fft32.process(&mut buf);
            let t = sig.tones[k] as usize;
            xsig += buf[t].norm().powi(2);
            let ios = (t + 4) % 7;
            xnoi += buf[ios].norm().powi(2);
        }

        let arg_noi = (xsig / xnoi.max(1e-12)) - 1.0;
        let mut snr_noi = if arg_noi > 0.1 { 10.0 * arg_noi.log10() - 27.0 } else { -24.0 };
        if snr_noi < -24.0 { snr_noi = -24.0; }

        let arg_base = (xsig / xbase_raw / 3.0e6) - 1.0;
        let mut snr_base = if arg_base > 0.1 { 10.0 * arg_base.log10() - 27.0 } else { -24.0 };
        if snr_base < -24.0 { snr_base = -24.0; }

        if (sig.freq - 1109.4).abs() < 5.0 {
            println!(
                "\n>>> [IK4LZH 详细数值] xsig={:.4e}, sbase={:.2} dB, xbase={:.4e}, arg_base={:.4e}, snr_base={:.2} dB, 官方参考=+16 dB <<<",
                xsig, baseline_raw.sbase[(sig.freq / baseline_raw.df).round() as usize], xbase_raw, arg_base, snr_base
            );
        }

        println!(
            "| {:<5.1} | {:<25} | {:<12} | {:<+12.0} | {:<+12.0} |",
            sig.freq, sig.message, osnr_str, snr_noi, snr_base
        );
    }
    println!("==========================================================================================================\n");
}

#[test]
fn test_compare_websdr_test4() {
    let wav_path = "reference/ft8_lib/test/wav/websdr_test4.wav";
    let (audio, _sr) = read_wav_file(wav_path).expect("读取 websdr_test4.wav 失败");

    let pipeline = rust_ft8::demodulate::Ft8Pipeline::new();
    let config = rust_ft8::demodulate::DecoderConfig::default();
    let decodes = pipeline.decode(&audio, &config);

    let txt_path = "reference/ft8_lib/test/wav/websdr_test4.txt";
    let official_txt = std::fs::read_to_string(txt_path).unwrap_or_default();
    let mut official_map = Vec::new();
    for line in official_txt.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 6 && parts[4] == "~" {
            let snr: i32 = parts[1].parse().unwrap_or(0);
            let dt: f32 = parts[2].parse().unwrap_or(0.0);
            let freq: f32 = parts[3].parse().unwrap_or(0.0);
            let msg = parts[5..parts.len().min(8)].join(" ");
            official_map.push((freq, dt, snr, msg));
        }
    }

    println!("\n==========================================================================================");
    println!("                           websdr_test4.wav 解码比对报告");
    println!("==========================================================================================");
    println!("官方目标检出数: {}, Rust 检出数: {}", official_map.len(), decodes.len());
    println!("------------------------------------------------------------------------------------------");
    println!("| {:<5} | {:<6} | {:<5} | {:<25} | {:<10} |", "频点", "时偏", "SNR", "解码消息", "状态");
    println!("|-------|--------|-------|---------------------------|------------|");

    let mut matched_cnt = 0;
    for (ofreq, odt, osnr, omsg) in &official_map {
        let matched = decodes.iter().find(|d| {
            (d.freq - ofreq).abs() < 10.0 || d.message.contains(omsg.as_str()) || omsg.contains(&d.message)
        });
        if let Some(d) = matched {
            matched_cnt += 1;
            println!("| {:<5.0} | {:<+6.1} | {:<+5} | {:<25} | 已解出 (Rust SNR: {:+2}) |", ofreq, odt, osnr, omsg, d.snr);
        } else {
            println!("| {:<5.0} | {:<+6.1} | {:<+5} | {:<25} | **漏检** |", ofreq, odt, osnr, omsg);
        }
    }

    println!("==========================================================================================");
    println!("检出率: {}/{} ({:.1}%)\n", matched_cnt, official_map.len(), (matched_cnt as f32 / official_map.len() as f32) * 100.0);

    println!("\n=== 漏检信号诊断分析 ===");
    let searcher = rust_ft8::demodulate::SyncSearcher::new();
    let _candidates = searcher.find_candidates(&audio, 200.0, 3500.0, 1.3, 600);

    println!("\n>>> 追踪 1141.0 Hz (dt=+0.2s) 单点解码流程 <<<");
    let downsampler = rust_ft8::demodulate::Downsampler::new();
    let long_fft = downsampler.compute_long_fft(&audio);
    let cd0_init = downsampler.downsample(&long_fft, 1141.0);
    let (ibest_init, delf, sync_fine) = searcher.fine_sync(&cd0_init, 0.2);
    println!("精同步 1: ibest={}, delf={:.2} Hz, sync={:.2}", ibest_init, delf, sync_fine);

    let f1 = 1141.0 + delf;
    let cd0_fine = downsampler.downsample(&long_fft, f1);
    let dt_approx = (ibest_init as f32 / 200.0) - 0.5;
    let (ibest_final, _delf2, sync_final) = searcher.fine_sync(&cd0_fine, dt_approx);
    let dt_final = (ibest_final as f32 / 200.0) - 0.5;
    println!("精同步 2: f1={:.2} Hz, ibest={}, dt={:+.2} s, sync={:.2}", f1, ibest_final, dt_final, sync_final);

    let extractor = rust_ft8::demodulate::SymbolExtractor::new();
    let extract_res = extractor.extract_and_decode(&cd0_fine, ibest_final, f1, sync_final, 1.0);
    if let Some(sig) = extract_res {
        println!("解码成功: [{}] (freq={:.1}, dt={:+.2}, snr={:+})", sig.message, sig.freq, sig.dt, sig.snr);
    } else {
        println!("解调/译码失败 (可能是精同步失败或 CRC 校验未过)");
    }
}
