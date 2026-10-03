use std::time::Instant;
use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};
use rust_ft8::ldpc::{bp_decode, osd_decode, OsdDepth, LDPC_N};

#[test]
fn test_micro_benchmarks() {
    println!("\n=======================================================");
    println!("        FT8 关键模块微基准耗时剖析 (Micro-Benchmark)");
    println!("=======================================================");

    // 1. 测试 OSD Order2 耗时
    let mut mock_llr = [0.0f32; LDPC_N];
    for i in 0..LDPC_N {
        mock_llr[i] = ((i as f32 * 0.13).sin() * 4.0) + ((i % 5) as f32 - 2.0);
    }

    let t0 = Instant::now();
    let iters_osd = 50;
    for _ in 0..iters_osd {
        let _ = osd_decode(&mock_llr, OsdDepth::Order2);
    }
    let osd_time = t0.elapsed().as_secs_f32() / (iters_osd as f32);
    println!("1. OSD Order2 (4095 测试向量) 单次耗时: {:.3} ms", osd_time * 1000.0);

    // 2. 测试 BP 译码耗时
    let t0 = Instant::now();
    let iters_bp = 500;
    for _ in 0..iters_bp {
        let _ = bp_decode(&mock_llr, 30, None);
    }
    let bp_time = t0.elapsed().as_secs_f32() / (iters_bp as f32);
    println!("2. BP 译码 (30 轮迭代) 单次耗时: {:.3} ms", bp_time * 1000.0);

    // 3. 实测典型音频的各阶段耗时
    let wav_path = "tests/wav/websdr_test5.wav";
    if let Ok((audio, _)) = read_wav_file(wav_path) {
        let pipeline = Ft8Pipeline::new();
        let config_std = DecoderConfig {
            nfa: 100.0,
            nfb: 3500.0,
            passes: 2,
            sync_min: 1.4,
            deep_search: false,
            enable_drift: true,
        };

        let t0 = Instant::now();
        let dec1 = pipeline.decode(&audio, &config_std);
        let d_std = t0.elapsed().as_secs_f32();
        println!("\n3. 标准模式 (passes=2, deep_search=false): 耗时={:.2}s, 解出={}条", d_std, dec1.len());

        let config_deep = DecoderConfig {
            nfa: 100.0,
            nfb: 3500.0,
            passes: 3,
            sync_min: 1.4,
            deep_search: true,
            enable_drift: true,
        };

        let t0 = Instant::now();
        let dec2 = pipeline.decode(&audio, &config_deep);
        let d_deep = t0.elapsed().as_secs_f32();
        println!("4. 深度模式 (passes=3, deep_search=true): 耗时={:.2}s, 解出={}条", d_deep, dec2.len());
    }
    println!("=======================================================\n");
}
