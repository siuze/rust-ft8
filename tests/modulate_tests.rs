use rust_ft8::modulate::{encode_message_to_tones, synth_ft8_audio, write_wav_file};
use std::process::Command;

#[test]
fn test_encode_and_cross_decode_with_wsjtx() {
    let message = "CQ BD4SUR OM99";
    let tones = encode_message_to_tones(message).expect("编码应成功");

    // 合成 1000 Hz 基频音频，延迟 0.5 秒，生成完整 15 秒 WAV
    let audio = synth_ft8_audio(&tones, 1000.0, 12000, 0.5, true);
    let wav_path = "target/test_cq_bd4sur.wav";
    write_wav_file(wav_path, &audio, 12000).expect("写入 WAV 应成功");

    // 使用 WSJT-X 官方参考 CLI 验证交叉解码
    let cli_exe = "reference/wsjtx_lib/cli_decode.exe";
    let path_env = format!("reference/wsjtx_lib;C:\\msys64\\mingw64\\bin;{}", std::env::var("PATH").unwrap_or_default());
    let output = Command::new(cli_exe)
        .arg(wav_path)
        .env("PATH", &path_env)
        .output()
        .expect("运行 cli_decode.exe 应成功");

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("cli_decode output:\n{}", stdout);

    assert!(
        stdout.contains("CQ BD4SUR OM99"),
        "WSJT-X 参考解码器必须成功解码出由 Rust 合成的消息！实际输出: {}",
        stdout
    );
}

#[test]
fn test_encode_complex_callsign_and_cross_decode() {
    let message = "W1AW/P DL1ABC JO31";
    let tones = encode_message_to_tones(message).expect("编码应成功");

    let audio = synth_ft8_audio(&tones, 1500.0, 12000, 0.5, true);
    let wav_path = "target/test_w1aw.wav";
    write_wav_file(wav_path, &audio, 12000).expect("写入 WAV 应成功");

    let cli_exe = "reference/wsjtx_lib/cli_decode.exe";
    let path_env = format!("reference/wsjtx_lib;C:\\msys64\\mingw64\\bin;{}", std::env::var("PATH").unwrap_or_default());
    let output = Command::new(cli_exe)
        .arg(wav_path)
        .env("PATH", &path_env)
        .output()
        .expect("运行 cli_decode.exe 应成功");

    let stdout = String::from_utf8_lossy(&output.stdout);
    println!("cli_decode output:\n{}", stdout);

    assert!(
        stdout.contains("W1AW/P DL1ABC JO31"),
        "WSJT-X 参考解码器必须成功解码出复杂呼号消息！实际输出: {}",
        stdout
    );
}
