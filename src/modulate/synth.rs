//! FT8 连续相位 GFSK 音频波形合成器
//!
//! 支持标准 12000 Hz 采样率，BT = 2.0 高斯频移键控平滑脉冲整形，
//! 具备首尾符号升余弦渐变平滑，输出符合 WSJT-X 物理层标准的完整 15 秒音频时隙。

use std::f32::consts::PI;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use crate::constants::{NUM_SYMBOLS, SLOT_TIME, SYMBOL_TIME};

const GFSK_CONST_C: f32 = 5.336446; // pi * sqrt(2.0 / ln(2.0))

/// Abramowitz and Stegun 7.1.26 误差函数 erf(x) 高精度纯数学逼近
/// 最大误差 <= 1.5e-7 (单精度完全等效于硬件指令)
#[inline]
pub fn fast_erf(x: f32) -> f32 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let abs_x = x.abs();

    let p = 0.3275911;
    let a1 = 0.254829592;
    let a2 = -0.284496736;
    let a3 = 1.421413741;
    let a4 = -1.453152027;
    let a5 = 1.061405429;

    let t = 1.0 / (1.0 + p * abs_x);
    let poly = ((((a5 * t + a4) * t + a3) * t + a2) * t + a1) * t;

    sign * (1.0 - poly * (-abs_x * abs_x).exp())
}

/// 计算 GFSK 脉冲整形滤波器响应（截断长度为 3 个符号，即 3 * n_spsym）
pub fn gfsk_pulse(n_spsym: usize, symbol_bt: f32) -> Vec<f32> {
    let mut pulse = vec![0.0f32; 3 * n_spsym];
    let inv_spsym = 1.0 / (n_spsym as f32);

    for i in 0..(3 * n_spsym) {
        let t = (i as f32) * inv_spsym - 1.5;
        let arg1 = GFSK_CONST_C * symbol_bt * (t + 0.5);
        let arg2 = GFSK_CONST_C * symbol_bt * (t - 0.5);
        pulse[i] = 0.5 * (fast_erf(arg1) - fast_erf(arg2));
    }

    pulse
}

/// 合成 79 个音调符号的连续相位 GFSK 音频波形
///
/// # 参数
/// - `tones`: 79 个音调符号 (0..7)
/// - `f0`: 符号 0 的基频（Hz，通常在 200..3000 Hz 之间，如 1000.0）
/// - `sample_rate`: 采样率（Hz，推荐 12000）
/// - `delay_seconds`: 信号起始相对于时隙开头的延迟（秒，默认 0.5s）
/// - `full_slot`: 是否补全至完整 15 秒（180,000 采样点）
///
/// # 返回
/// 归一化浮点 PCM 采样点数组 (幅值范围 [-1.0, 1.0])
pub fn synth_ft8_audio(
    tones: &[u8; NUM_SYMBOLS],
    f0: f32,
    sample_rate: usize,
    delay_seconds: f32,
    full_slot: bool,
) -> Vec<f32> {
    let n_spsym = ((sample_rate as f32) * SYMBOL_TIME).round() as usize; // 1920
    let n_wave = NUM_SYMBOLS * n_spsym; // 79 * 1920 = 151680

    let symbol_bt = 2.0f32;
    let pulse = gfsk_pulse(n_spsym, symbol_bt);

    // 频偏步进 = 2*PI * tone_spacing / sample_rate = 2*PI / n_spsym
    let dphi_peak = 2.0 * PI / (n_spsym as f32);

    // 瞬时相位增量数组（扩展首尾虚拟符号）
    let total_dphi_len = n_wave + 2 * n_spsym;
    let mut dphi = vec![2.0 * PI * f0 / (sample_rate as f32); total_dphi_len];

    // 叠加每个符号的高斯脉冲
    for (i, &tone) in tones.iter().enumerate() {
        let ib = i * n_spsym;
        let tone_f = tone as f32;
        for j in 0..(3 * n_spsym) {
            dphi[ib + j] += dphi_peak * tone_f * pulse[j];
        }
    }

    // 首尾虚拟扩展符号（保持初末相位平稳连续）
    let t0 = tones[0] as f32;
    let t_last = tones[NUM_SYMBOLS - 1] as f32;
    for j in 0..(2 * n_spsym) {
        dphi[j] += dphi_peak * t0 * pulse[j + n_spsym];
        dphi[j + n_wave] += dphi_peak * t_last * pulse[j];
    }

    // 相位累加生成正弦波
    let mut signal = vec![0.0f32; n_wave];
    let mut phi = 0.0f32;
    let two_pi = 2.0 * PI;

    for k in 0..n_wave {
        signal[k] = phi.sin();
        phi = (phi + dphi[k + n_spsym]).rem_euclid(two_pi);
    }

    // 首尾符号升余弦渐变平滑 (ramp = n_spsym / 8 = 240 点 = 20ms)
    let n_ramp = n_spsym / 8;
    for i in 0..n_ramp {
        let env = (1.0 - (PI * (i as f32) / (n_ramp as f32)).cos()) * 0.5;
        signal[i] *= env;
        signal[n_wave - 1 - i] *= env;
    }

    if !full_slot {
        return signal;
    }

    // 填充至完整 15.0 秒时隙
    let total_samples = ((sample_rate as f32) * SLOT_TIME).round() as usize; // 180000
    let mut slot_signal = vec![0.0f32; total_samples];

    let delay_samples = ((delay_seconds * sample_rate as f32).round() as usize).min(total_samples);
    let copy_len = signal.len().min(total_samples.saturating_sub(delay_samples));

    slot_signal[delay_samples..(delay_samples + copy_len)].copy_from_slice(&signal[..copy_len]);

    slot_signal
}

/// 将单声道浮点音频数据写入标准 16-bit PCM WAV 文件
pub fn write_wav_file<P: AsRef<Path>>(
    path: P,
    samples: &[f32],
    sample_rate: u32,
) -> std::io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    let num_samples = samples.len() as u32;
    let bits_per_sample: u16 = 16;
    let num_channels: u16 = 1;
    let byte_rate = sample_rate * (bits_per_sample as u32 / 8) * (num_channels as u32);
    let block_align = num_channels * (bits_per_sample / 8);
    let subchunk2_size = num_samples * (bits_per_sample as u32 / 8) * (num_channels as u32);
    let chunk_size = 36 + subchunk2_size;

    // RIFF 头
    writer.write_all(b"RIFF")?;
    writer.write_all(&chunk_size.to_le_bytes())?;
    writer.write_all(b"WAVE")?;

    // fmt 子块
    writer.write_all(b"fmt ")?;
    writer.write_all(&16u32.to_le_bytes())?; // Subchunk1Size (16 for PCM)
    writer.write_all(&1u16.to_le_bytes())?;  // AudioFormat (1 for PCM)
    writer.write_all(&num_channels.to_le_bytes())?;
    writer.write_all(&sample_rate.to_le_bytes())?;
    writer.write_all(&byte_rate.to_le_bytes())?;
    writer.write_all(&block_align.to_le_bytes())?;
    writer.write_all(&bits_per_sample.to_le_bytes())?;

    // data 子块
    writer.write_all(b"data")?;
    writer.write_all(&subchunk2_size.to_le_bytes())?;

    // 16-bit PCM 采样点
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let pcm16 = (clamped * 32767.0).round() as i16;
        writer.write_all(&pcm16.to_le_bytes())?;
    }

    writer.flush()?;
    Ok(())
}

/// 根据文本消息直接编码并合成 12000 Hz 连续相位 GFSK 音频波形
///
/// # 参数
/// - `message`: FT8 消息文本 (如 "CQ BD4SUR OM99" 或 "<ED3C6B>[BG5VDH] VR2XXX -10")
/// - `f0`: 载波音频起始频率 (Hz，如 1000.0)
/// - `sample_rate`: 音频采样率 (推荐 12000)
/// - `delay_seconds`: 相对时隙起始的发射时间偏移 (通常为 0.5s)
/// - `full_slot`: 是否补全为完整 15 秒 (180,000 点) 数组
pub fn encode_message_to_audio(
    message: &str,
    f0: f32,
    sample_rate: usize,
    delay_seconds: f32,
    full_slot: bool,
) -> Result<Vec<f32>, String> {
    let tones = crate::modulate::encode_message_to_tones(message)?;
    Ok(synth_ft8_audio(&tones, f0, sample_rate, delay_seconds, full_slot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::SAMPLE_RATE;

    #[test]
    fn test_erf_accuracy() {
        // 与已知 erf 值对比
        assert!((fast_erf(0.0) - 0.0).abs() < 1e-6);
        assert!((fast_erf(1.0) - 0.84270079).abs() < 1e-4);
        assert!((fast_erf(-1.0) - (-0.84270079)).abs() < 1e-4);
        assert!((fast_erf(2.0) - 0.99532226).abs() < 1e-4);
    }

    #[test]
    fn test_synth_ft8_length() {
        let tones = [0u8; NUM_SYMBOLS];
        let audio = synth_ft8_audio(&tones, 1000.0, SAMPLE_RATE, 0.5, true);
        assert_eq!(audio.len(), 180000, "完整 15 秒应为 180000 采样点");

        let active_audio = synth_ft8_audio(&tones, 1000.0, SAMPLE_RATE, 0.5, false);
        assert_eq!(active_audio.len(), 151680, "有效符号区应为 151680 采样点");
    }
}
