//! FT8 完整 3-Pass 信号消减解调流水线 (对标 WSJT-X ft8_decode.f90)
//!
//! 实现：
//! 1. 粗同步候选检测 (Costas 图案相关峰搜索)
//! 2. 候选信号基带抽取与精细时频对齐
//! 3. 多符号相干 LLR 提取与混合 LDPC 译码
//! 4. 强信号时域重构与残差消减，激活多 Pass 弱信号深挖

use super::baseline::BaselineEstimator;
use super::downsample::{Downsampler, NMAX};
use super::extract::{DecodedSignal, SymbolExtractor};
use super::subtract::SignalSubtracter;
use super::sync::SyncSearcher;

/// FT8 解调器配置
#[derive(Debug, Clone)]
pub struct DecoderConfig {
    /// 搜索最低频率 (Hz)
    pub nfa: f32,
    /// 搜索最高频率 (Hz)
    pub nfb: f32,
    /// 消减重搜轮数 (推荐 3 轮对标 WSJT-X)
    pub passes: usize,
    /// 同步检测初始门限 (推荐 1.4 ~ 1.6)
    pub sync_min: f32,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            nfa: 100.0,
            nfb: 3500.0,
            passes: 3,
            sync_min: 1.4,
        }
    }
}

/// FT8 完整解调流水线
pub struct Ft8Pipeline {
    downsampler: Downsampler,
    searcher: SyncSearcher,
    extractor: SymbolExtractor,
    subtracter: SignalSubtracter,
    baseline_estimator: BaselineEstimator,
}

impl Ft8Pipeline {
    pub fn new() -> Self {
        Self {
            downsampler: Downsampler::new(),
            searcher: SyncSearcher::new(),
            extractor: SymbolExtractor::new(),
            subtracter: SignalSubtracter::new(),
            baseline_estimator: BaselineEstimator::new(),
        }
    }

    /// 对 15 秒（12000 Hz，约 180,000 采样点）音频数据执行 3-Pass 消减解调
    pub fn decode(&self, audio: &[f32], config: &DecoderConfig) -> Vec<DecodedSignal> {
        let mut working_audio = vec![0.0f32; NMAX];
        let copy_len = audio.len().min(NMAX);

        // 严格 1:1 对标 WSJT-X: 将输入音频统一至 16-bit PCM 整型浮点量纲 ([-32768, 32767])
        let max_val = audio.iter().fold(0.0f32, |acc, &x| acc.max(x.abs()));
        let scale = if max_val <= 2.0 && max_val > 1e-6 {
            32768.0
        } else {
            1.0
        };
        for i in 0..copy_len {
            working_audio[i] = audio[i] * scale;
        }

        // 预先计算整段音频的物理背景底噪谱 sbase (对标 WSJT-X sync8.f90 L44 / ft8_decode.f90 L201)
        let baseline = self.baseline_estimator.estimate_baseline(&working_audio, config.nfa, config.nfb);

        let mut decoded_all: Vec<DecodedSignal> = Vec::new();

        for ipass in 1..=config.passes {
            let sync_thresh = match ipass {
                1 => config.sync_min + 0.2, // 第 1 轮门限略高，优先锁定强信号
                2 => config.sync_min,       // 第 2 轮标准门限
                _ => config.sync_min - 0.1, // 第 3 轮深搜残差
            };

            // 1. 在当前残差音频中搜索候选信号
            let candidates = self.searcher.find_candidates(
                &working_audio,
                config.nfa,
                config.nfb,
                sync_thresh,
                400,
            );

            if candidates.is_empty() {
                continue;
            }

            // 2. 计算当前残差音频的 192000 点全长 FFT
            let long_fft = self.downsampler.compute_long_fft(&working_audio);

            let mut pass_new_decodes = 0;

            // 3. 逐个候选解调
            for cand in &candidates {
                // 提取 200 Hz 基带信号
                let cd0_init = self.downsampler.downsample(&long_fft, cand.freq);

                // 精细对齐时间与频偏 (delf)
                let (_ibest_init, delf, _) = self.searcher.fine_sync(&cd0_init, cand.dt);
                let f1 = cand.freq + delf;

                // 关键对齐：使用修正后的精确中心频率 f1 重新切片基带，确保 8 音调严格正交对齐 FFT 频点 (对标 WSJT-X ft8b.f90 L140)
                let cd0 = self.downsampler.downsample(&long_fft, f1);
                let (ibest, _, sync_pow) = self.searcher.fine_sync(&cd0, cand.dt);

                // 计算当前载频处的物理噪声基线 xbase (对标 ft8_decode.f90 L201)
                let xbase = baseline.get_xbase(f1);

                // 提取多符号能量并送入译码器
                if let Some(sig) = self.extractor.extract_and_decode(&cd0, ibest, f1, sync_pow, xbase) {
                    // 确认消息是否新出现 (对标 WSJT-X ft8_decode.f90: 仅按消息文本去重，支持近邻与同频弱信号检出)
                    let is_dup = decoded_all.iter().any(|d| d.message == sig.message);

                    if !is_dup {
                        decoded_all.push(sig.clone());
                        pass_new_decodes += 1;

                        // 从时域残差音频中精确扣除该信号 (基于 200 Hz 精确采样点 ibest)
                        self.subtracter.subtract_signal(
                            &mut working_audio,
                            &sig.tones,
                            sig.freq,
                            ibest,
                        );
                    }
                }
            }

            // 若本轮未新解出任何信号，提前终止消减循环
            if pass_new_decodes == 0 {
                break;
            }
        }

        // 按载波频率升序排列
        decoded_all.sort_by(|a, b| a.freq.partial_cmp(&b.freq).unwrap());
        decoded_all
    }
}

impl Default for Ft8Pipeline {
    fn default() -> Self {
        Self::new()
    }
}

/// 读取 16-bit PCM WAV 音频并转换为 12000 Hz 单声道浮点数组
pub fn read_wav_file<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<(Vec<f32>, u32)> {
    use std::fs::File;
    use std::io::Read;

    let mut file = File::open(path)?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header)?;

    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "不是合法的 RIFF/WAVE 文件",
        ));
    }

    let mut sample_rate = 12000u32;
    let mut channels = 1u16;
    let mut bits_per_sample = 16u16;
    let mut pcm_bytes = Vec::new();

    let mut chunk_header = [0u8; 8];
    while file.read_exact(&mut chunk_header).is_ok() {
        let chunk_id = &chunk_header[0..4];
        let chunk_size = u32::from_le_bytes(chunk_header[4..8].try_into().unwrap()) as usize;

        if chunk_id == b"fmt " {
            let mut fmt_buf = vec![0u8; chunk_size];
            file.read_exact(&mut fmt_buf)?;
            if fmt_buf.len() >= 16 {
                channels = u16::from_le_bytes(fmt_buf[2..4].try_into().unwrap());
                sample_rate = u32::from_le_bytes(fmt_buf[4..8].try_into().unwrap());
                bits_per_sample = u16::from_le_bytes(fmt_buf[14..16].try_into().unwrap());
            }
        } else if chunk_id == b"data" {
            pcm_bytes = vec![0u8; chunk_size];
            file.read_exact(&mut pcm_bytes)?;
            break;
        } else {
            // 跳过其他无用 chunk (如 LIST)
            let mut skip_buf = vec![0u8; chunk_size];
            file.read_exact(&mut skip_buf)?;
        }
    }

    // 仅支持 16-bit PCM
    if bits_per_sample != 16 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("仅支持 16-bit PCM，当前为 {}-bit", bits_per_sample),
        ));
    }

    let num_samples = pcm_bytes.len() / (2 * channels as usize);
    let mut samples = Vec::with_capacity(num_samples);

    for i in 0..num_samples {
        let byte_idx = i * 2 * channels as usize;
        let s = i16::from_le_bytes([pcm_bytes[byte_idx], pcm_bytes[byte_idx + 1]]);
        samples.push((s as f32) / 32768.0);
    }

    Ok((samples, sample_rate))
}
