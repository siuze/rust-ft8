pub mod constants;
pub mod crc;
pub mod demodulate;
pub mod dxcc;
pub mod hash;
pub mod ldpc;
pub mod modulate;
pub mod pack;

// 常用顶层便利导出
pub use demodulate::message::{Ft8DecodedMessage, MessageType, QsoStage};
pub use demodulate::pipeline::{read_wav_file, DecoderConfig, Ft8Pipeline};
pub use demodulate::streaming::{StreamDecodedEvent, StreamEvent, StreamingFt8Receiver};
pub use dxcc::{
    country_en_to_cn, great_circle_bearing, great_circle_distance, grid_distance, grid_to_latlon,
    latlon_to_grid, lookup_callsign_country, DxccDatabase, DxccEntity,
};
pub use modulate::{
    encode_message_to_audio, encode_message_to_tones, synth_ft8_audio, write_wav_file,
};

/// 顶层极简离线解码函数
///
/// # 参数
/// - `audio`: 12000 Hz 单声道浮点音频切片 (至少约 12~15 秒)
/// - `config`: 解调参数配置 (可传 `&DecoderConfig::default()`)
/// - `window_start_offset`: 录音起始点相对于真实时间窗口的偏差 (秒)
///   例如：调用者以当前时间窗口 -0.9s 作为录音开始时间，传入 `-0.9`，
///   函数会自动将各个信号的 DT 校准为相对于真实时间窗口的时间延迟。
pub fn decode_audio(
    audio: &[f32],
    config: &DecoderConfig,
    window_start_offset: f32,
) -> Vec<Ft8DecodedMessage> {
    let pipeline = Ft8Pipeline::new();
    pipeline.decode_structured(audio, config, window_start_offset)
}


