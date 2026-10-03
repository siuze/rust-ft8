//! FT8 完整 3-Pass 信号消减解调流水线 (对标 WSJT-X ft8_decode.f90)
//!
//! 实现：
//! 1. 粗同步候选检测 (Costas 图案相关峰搜索)
//! 2. 候选信号基带抽取与精细时频对齐
//! 3. 多符号相干 LLR 提取与混合 LDPC 译码
//! 4. 强信号时域重构与残差消减，激活多 Pass 弱信号深挖

use num_complex::Complex32;
use rayon::prelude::*;
use super::baseline::BaselineEstimator;
use super::downsample::{Downsampler, NMAX, NFFT2};
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
    /// 消减重搜轮数 (推荐 2 轮或 3 轮对标 WSJT-X)
    pub passes: usize,
    /// 同步检测初始门限 (推荐 1.4 ~ 1.6)
    pub sync_min: f32,
    /// 深度搜索模式：放开 OSD 搜索深度与多符号相干通道，大幅提升微弱信号检出率
    pub deep_search: bool,
    /// 是否启用频漂跟踪 (Drift Rate) 与动态调频波形消减
    pub enable_drift: bool,
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            nfa: 100.0,
            nfb: 3500.0,
            passes: 2,
            sync_min: 1.4,
            deep_search: false,
            enable_drift: true,
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

    /// 获取粗同步搜索器引用
    pub fn searcher(&self) -> &SyncSearcher {
        &self.searcher
    }


    /// 对 15 秒（12000 Hz，约 180,000 采样点）音频数据执行 3-Pass 消减解调
    pub fn decode(&self, audio: &[f32], config: &DecoderConfig) -> Vec<DecodedSignal> {
        self.run_pipeline(audio, config, 0.0, 0, None::<fn(&crate::demodulate::message::Ft8DecodedMessage)>).0
    }

    /// 核心消减解码主循环，支持每轮 Pass 即时向外部回调流出最早解出信号
    fn run_pipeline<F>(
        &self,
        audio: &[f32],
        config: &DecoderConfig,
        window_start_offset: f32,
        sequence_id: u64,
        mut callback: Option<F>,
    ) -> (Vec<DecodedSignal>, Vec<crate::demodulate::message::Ft8DecodedMessage>)
    where
        F: FnMut(&crate::demodulate::message::Ft8DecodedMessage),
    {
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
            let t_pass_start = std::time::Instant::now();
            let sync_thresh = match ipass {
                1 => config.sync_min + 0.2, // 第 1 轮门限略高，优先锁定强信号
                2 => config.sync_min,       // 第 2 轮标准门限
                _ => config.sync_min - 0.1, // 第 3 轮深搜残差
            };

            // 1. 在当前残差音频中搜索候选信号
            let t_find = std::time::Instant::now();
            let candidates = self.searcher.find_candidates(
                &working_audio,
                config.nfa,
                config.nfb,
                sync_thresh,
                300,
            );
            let d_find = t_find.elapsed().as_secs_f32();

            if candidates.is_empty() {
                break;
            }

            // 2. 计算当前残差音频的 192000 点全长 FFT
            let t_fft = std::time::Instant::now();
            let long_fft = self.downsampler.compute_long_fft(&working_audio);
            let d_fft = t_fft.elapsed().as_secs_f32();

            // 3. 候选信号并行提取与译码 (Rayon 多核并行，压满 CPU 核心)
            let t_dec = std::time::Instant::now();
            let decoded_candidates: Vec<(DecodedSignal, isize)> = candidates
                .par_iter()
                .map_init(
                    || (vec![Complex32::new(0.0, 0.0); NFFT2], vec![Complex32::new(0.0, 0.0); NFFT2]),
                    |(cd0_buf, cd1_buf), cand| {
                        self.downsampler.downsample_to_slice(&long_fft, cand.freq, cd0_buf);
                        let (_ibest_init, delf, _, _) = self.searcher.fine_sync_with_drift(cd0_buf, cand.dt, false);
                        let (cd0, f1) = if delf.abs() < 0.05 {
                            (&*cd0_buf, cand.freq)
                        } else {
                            let f1 = cand.freq + delf;
                            self.downsampler.downsample_to_slice(&long_fft, f1, cd1_buf);
                            (&*cd1_buf, f1)
                        };
                        let (ibest, _, drift, sync_pow) = self.searcher.fine_sync_with_drift(cd0, cand.dt, config.enable_drift);
                        let xbase = baseline.get_xbase(f1);
                        self.extractor
                            .extract_and_decode(cd0, ibest, f1, drift, sync_pow, xbase, config.deep_search)
                            .map(|sig| (sig, ibest))
                    },
                )
                .flatten()
                .collect();
            let d_dec = t_dec.elapsed().as_secs_f32();

            // 4. 提取本轮新解出信号 (兼顾文本去重与时频近邻冲突抑制)
            let mut new_signals = Vec::new();
            for (sig, ibest) in decoded_candidates {
                // 完全相同文本去重
                if decoded_all.iter().any(|d| d.message == sig.message) {
                    continue;
                }
                // 时频近邻冲突抑制 (|Δf| < 8.0 Hz 且 |Δt| < 0.20s)
                let conflict_idx = decoded_all.iter().position(|d| {
                    (d.freq - sig.freq).abs() < 8.0 && (d.dt - sig.dt).abs() < 0.20
                });
                let mut is_new = false;
                if let Some(c_idx) = conflict_idx {
                    if sig.hard_errors < decoded_all[c_idx].hard_errors || sig.snr > decoded_all[c_idx].snr {
                        decoded_all[c_idx] = sig.clone();
                        new_signals.push((sig.clone(), ibest));
                        is_new = true;
                    }
                } else {
                    decoded_all.push(sig.clone());
                    new_signals.push((sig.clone(), ibest));
                    is_new = true;
                }

                // 若有新解出的有效信号，且注册了即时回调，立即构造成结构化消息通知外部 (无需等待后续 Pass 和消减计算)！
                if is_new {
                    if let Some(ref mut cb) = callback {
                        let struct_msg = crate::demodulate::message::Ft8DecodedMessage::parse_with_sequence(
                            sig.dt,
                            sig.snr,
                            sig.freq,
                            sig.drift,
                            &sig.message,
                            window_start_offset,
                            sequence_id,
                        );
                        cb(&struct_msg);
                    }
                }
            }
            let pass_new_decodes = new_signals.len();

            // 若本轮为最后一轮或新解出信号为 0，无需再重构波形进行消减
            let is_last_pass = ipass >= config.passes || pass_new_decodes == 0 || (ipass >= 2 && pass_new_decodes <= 1);

            let t_sub = std::time::Instant::now();
            if !is_last_pass && !new_signals.is_empty() {
                let sub_waves: Vec<(isize, Vec<f32>)> = new_signals
                    .par_iter()
                    .map_init(
                        || self.subtracter.create_buffers(),
                        |bufs, (sig, ibest)| {
                            self.subtracter.reconstruct_waveform_with_buf(
                                &sig.tones,
                                sig.freq,
                                sig.drift,
                                *ibest,
                                &working_audio,
                                bufs,
                            )
                        },
                    )
                    .collect();

                // 在主线程统一扣除各路信号
                for (nstart, wave) in sub_waves {
                    for (i, &rec) in wave.iter().enumerate() {
                        let j = nstart + (i as isize);
                        if j >= 0 && (j as usize) < working_audio.len() {
                            working_audio[j as usize] -= rec;
                        }
                    }
                }
            }
            let d_sub = t_sub.elapsed().as_secs_f32();
            let d_total = t_pass_start.elapsed().as_secs_f32();

            eprintln!(
                "Pass {}: 总耗时={:.2}s | 搜候选={:.2}s ({}个) | 192kFFT={:.2}s | 并行译码={:.2}s (解出{}条) | 消减={:.2}s",
                ipass, d_total, d_find, candidates.len(), d_fft, d_dec, pass_new_decodes, d_sub
            );

            if is_last_pass {
                break;
            }
        }

        // 按载波频率升序排列
        decoded_all.sort_by(|a, b| a.freq.partial_cmp(&b.freq).unwrap());
        let structured_all = decoded_all
            .iter()
            .map(|sig| {
                crate::demodulate::message::Ft8DecodedMessage::parse_with_sequence(
                    sig.dt,
                    sig.snr,
                    sig.freq,
                    sig.drift,
                    &sig.message,
                    window_start_offset,
                    sequence_id,
                )
            })
            .collect();

        (decoded_all, structured_all)
    }

    /// 模式一：完全阻塞式同步解码，一次性返回所有结果
    ///
    /// # 参数
    /// - `audio`: 12000 Hz 单声道浮点音频切片 (约 15 秒)
    /// - `config`: 解调参数配置
    /// - `window_start_offset`: 录音起始点相对于真实 15 秒时隙起点的偏移 (秒)
    /// - `sequence_id`: uint64 请求序列号，将被透传至每个解码消息中
    pub fn decode_structured(
        &self,
        audio: &[f32],
        config: &DecoderConfig,
        window_start_offset: f32,
        sequence_id: u64,
    ) -> Vec<crate::demodulate::message::Ft8DecodedMessage> {
        self.run_pipeline(audio, config, window_start_offset, sequence_id, None::<fn(&crate::demodulate::message::Ft8DecodedMessage)>).1
    }

    /// 模式二：实时增量回调解码 (异步流式优先模式)
    ///
    /// 无论多轮消减进行到何种阶段，一旦解调出新信号，**立即**通过 `callback` 句柄流出最早解码数据；
    /// 调用者无需等待全部 Pass 计算与波形消减结束即可优先处理空中最早到来的有效报文。
    ///
    /// # 参数
    /// - `audio`: 12000 Hz 单声道浮点音频切片
    /// - `config`: 解调参数配置
    /// - `window_start_offset`: 录音起始点时间偏移 (秒)
    /// - `sequence_id`: uint64 请求序列号
    /// - `callback`: 回调闭包/函数句柄，接收到刚解出的消息引用
    ///
    /// # 返回值
    /// 返回全部 Passes 完成后的全量结构化消息列表 (按频率升序排列)
    pub fn decode_with_callback<F>(
        &self,
        audio: &[f32],
        config: &DecoderConfig,
        window_start_offset: f32,
        sequence_id: u64,
        callback: F,
    ) -> Vec<crate::demodulate::message::Ft8DecodedMessage>
    where
        F: FnMut(&crate::demodulate::message::Ft8DecodedMessage),
    {
        self.run_pipeline(audio, config, window_start_offset, sequence_id, Some(callback)).1
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
