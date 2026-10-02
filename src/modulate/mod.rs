//! FT8 调制与音频合成模块
//!
//! 包括：
//! - 79 音调符号序列生成 (`tones`)
//! - GFSK 连续相位音频波形合成与 WAV 生成 (`synth`)

pub mod synth;
pub mod tones;

pub use synth::{encode_message_to_audio, fast_erf, gfsk_pulse, synth_ft8_audio, write_wav_file};
pub use tones::{encode_message_to_tones, ft8_bits_to_tones, ft8_payload_to_tones};

