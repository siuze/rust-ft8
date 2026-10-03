use rust_ft8::demodulate::{read_wav_file, DecoderConfig, Ft8Pipeline};

#[test]
fn distinct_messages_in_nearby_time_frequency_bins_survive() {
    let config = DecoderConfig {
        nfa: 100.0,
        nfb: 3500.0,
        passes: 3,
        sync_min: 1.4,
        deep_search: true,
        enable_drift: true,
    };
    for (wav, expected) in [
        (
            "websdr_test9",
            ["CQ PY1SX GG87", "K6DRY YO9HP -15"],
        ),
        (
            "191111_110615",
            ["G1XJM HA7JIV JN97", "ET3RFG/R IN3ADG -23"],
        ),
    ] {
        let path = format!("tests/wav/{wav}.wav");
        let (audio, _) = read_wav_file(&path).expect("benchmark WAV must be present");
        let decoded = Ft8Pipeline::new().decode(&audio, &config);
        for text in expected {
            assert!(
                decoded.iter().any(|signal| signal.message == text),
                "{wav}: missing {text}"
            );
        }
    }
}
