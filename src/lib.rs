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

/// 顶层离线解码函数 (模式一：完全阻塞同步解码，一次性返回所有结果)
///
/// # 参数
/// - `audio`: 12000 Hz 单声道浮点音频切片 (约 12~15 秒)
/// - `config`: 解调参数配置 (可传 `&DecoderConfig::default()`)
/// - `window_start_offset`: 录音起始点相对于真实时间窗口的偏差 (秒)
///   例如：调用者以当前时间窗口 -0.9s 作为录音开始时间，传入 `-0.9`，
///   函数会自动将各个信号的 DT 校准为相对于真实时间窗口的时间延迟。
/// - `sequence_id`: 请求序列号 (uint64)，透传至每个解码消息对象中
pub fn decode_audio(
    audio: &[f32],
    config: &DecoderConfig,
    window_start_offset: f32,
    sequence_id: u64,
) -> Vec<Ft8DecodedMessage> {
    let pipeline = Ft8Pipeline::new();
    pipeline.decode_structured(audio, config, window_start_offset, sequence_id)
}

/// 顶层离线解码函数 (模式二：实时增量回调，异步优先流出最早解出信号)
///
/// 在多轮消减解调过程中，一旦有新的有效信号解调完成，**立即**通过 `callback` 句柄传给调用方；
/// 调用者可即时将消息推送至 UI 或队列，无需等待整轮减法全部跑完。
///
/// # 参数
/// - `audio`: 12000 Hz 单声道浮点音频切片
/// - `config`: 解调参数配置
/// - `window_start_offset`: 录音起始点时间偏移 (秒)
/// - `sequence_id`: 请求序列号 (uint64)
/// - `callback`: 回调闭包或函数句柄
///
/// # 返回值
/// 返回全量解码完成后的所有消息列表
pub fn decode_audio_with_callback<F>(
    audio: &[f32],
    config: &DecoderConfig,
    window_start_offset: f32,
    sequence_id: u64,
    callback: F,
) -> Vec<Ft8DecodedMessage>
where
    F: FnMut(&Ft8DecodedMessage),
{
    let pipeline = Ft8Pipeline::new();
    pipeline.decode_with_callback(audio, config, window_start_offset, sequence_id, callback)
}



