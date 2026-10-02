//! FT8 流式边收边解与提前解码流水线 (Streaming & Early Decoding)
//!
//! # 物理层时序设计与通信事实
//! FT8 协议周期为 15.00 秒，发射机有效发射时间为前 12.64 秒 (共 79 个符号，每符号 160 ms)。
//! 12.64s ~ 15.00s 为长达 2.36 秒的无线电静默空闲期。
//!
//! 79 个符号分布：
//! - 0 ~ 6 (0.00s ~ 1.12s): Costas 前导同步码 (7 符号)
//! - 7 ~ 35 (1.12s ~ 5.76s): Payload 数据段 1 (29 符号)
//! - 36 ~ 42 (5.76s ~ 6.88s): Costas 中导同步码 (7 符号)
//! - 43 ~ 71 (6.88s ~ 11.52s): Payload 数据段 2 (29 符号)
//! - 72 ~ 78 (11.52s ~ 12.64s): Costas 尾导同步码 (7 符号)
//!
//! 核心创新机制：
//! 1. 前导快速锁定 (t ≈ 1.6s): 检测并缓存活跃频点；
//! 2. 提前擦除解码 (t = 11.52s): 全部 58 个有效数据符号全部收齐，尾导码作为 Erasure 提前解出强信号；
//! 3. 扫尾完成 (t = 12.64s ~ 12.8s): 补齐尾导码深挖弱信号，比 15s 窗口提前 2.2 秒零延迟交付！

use super::downsample::NMAX;
use super::extract::DecodedSignal;
use super::message::Ft8DecodedMessage;
use super::pipeline::{DecoderConfig, Ft8Pipeline};

/// 流式接收器原始状态事件
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 阶段 1 (t ≈ 1.6s)：前导码已就位，锁定空中活跃频点
    PreambleDetected { active_frequencies: Vec<f32> },
    /// 阶段 2 (t ≈ 11.52s)：有效数据已全部收齐，提前输出强信号结果 (比 15s 提前 3.2 秒！)
    EarlyDecoded(Vec<DecodedSignal>),
    /// 阶段 3 (t ≈ 12.64s ~ 12.8s)：发射全部结束，微弱信号消减扫尾完成，输出全量最终结果 (比 15s 提前 2.2 秒！)
    CycleCompleted(Vec<DecodedSignal>),
    /// 阶段 4：本轮时隙解码完全结束通知 (含提前结束/自然结束标记)
    DecodeFinished {
        total_signals: usize,
        audio_duration_sec: f32,
        is_last_chunk: bool,
    },
}

/// 完整结构化流式接收器事件 (含 DT 真实窗口基准校正与 9 项完整结构化字段)
#[derive(Debug, Clone)]
pub enum StreamDecodedEvent {
    /// 阶段 1 (t ≈ 1.44s)：前导码已就位，快速锁定空中活跃载频
    PreambleDetected {
        active_frequencies: Vec<f32>,
        time_sec: f32,
    },
    /// 阶段 2 (t ≈ 11.36s ~ 11.52s)：有效数据已全部收齐，提前输出强信号结果
    EarlyDecoded {
        signals: Vec<Ft8DecodedMessage>,
        time_sec: f32,
    },
    /// 阶段 3 (t ≈ 12.5s ~ 12.8s)：发射全部结束，微弱信号消减扫尾完成，全量最终结果交付
    CycleCompleted {
        all_signals: Vec<Ft8DecodedMessage>,
        time_sec: f32,
    },
    /// 阶段 4：本轮时隙解码完全结束通知 (所有 Pass 与消减计算均已完成，可安全转入下一时隙)
    DecodeFinished {
        total_signals: usize,
        audio_duration_sec: f32,
        is_last_chunk: bool,
    },
}

/// 流式边收边解 FT8 接收机
pub struct StreamingFt8Receiver {
    pipeline: Ft8Pipeline,
    config: DecoderConfig,
    audio_buffer: Vec<f32>,
    samples_in_cycle: usize,
    preamble_fired: bool,
    early_fired: bool,
    cycle_fired: bool,
    finished_fired: bool,
    last_completed_count: usize,
    early_results: Vec<DecodedSignal>,
    window_start_offset: f32,
}

impl StreamingFt8Receiver {
    /// 创建流式接收机实例 (默认 window_start_offset = 0.0)
    pub fn new(config: DecoderConfig) -> Self {
        Self::with_window_offset(config, 0.0)
    }

    /// 创建带时间窗口起始点偏移的流式接收机实例
    ///
    /// # 参数
    /// - `config`: 解调参数
    /// - `window_start_offset`: 录音起始点相对于真实 15 秒时隙起点的偏移 (秒)
    ///   - 例如：调用者以当前时间窗口 -0.9s 作为录音开始，则传入 `-0.9`；
    ///   - 各个解码信号的 DT 将自动以此校正为相对真实时间窗口的时间延迟。
    pub fn with_window_offset(config: DecoderConfig, window_start_offset: f32) -> Self {
        Self {
            pipeline: Ft8Pipeline::new(),
            config,
            audio_buffer: vec![0.0f32; NMAX],
            samples_in_cycle: 0,
            preamble_fired: false,
            early_fired: false,
            cycle_fired: false,
            finished_fired: false,
            last_completed_count: 0,
            early_results: Vec::new(),
            window_start_offset,
        }
    }

    /// 查询当前时隙周期是否已经完成了最终解码
    pub fn is_cycle_finished(&self) -> bool {
        self.finished_fired
    }

    /// 获取当前时隙周期已解出的最新信号总数
    pub fn last_signals_count(&self) -> usize {
        self.last_completed_count
    }

    /// 重置周期状态机 (通常在 15.0 秒时隙交界处调用)
    pub fn reset_cycle(&mut self) {
        self.audio_buffer.fill(0.0);
        self.samples_in_cycle = 0;
        self.preamble_fired = false;
        self.early_fired = false;
        self.cycle_fired = false;
        self.finished_fired = false;
        self.last_completed_count = 0;
        self.early_results.clear();
    }

    /// 带有 is_last (是否为本时隙最后一包音频) 标志的流式音频喂入接口
    ///
    /// # 参数
    /// - `chunk`: 采样率 12000 Hz 的单声道 f32 音频切片
    /// - `is_last`: 若为 true，代表本轮音频流已结束，接收机将立即对已有音频执行扫尾解码并发出 DecodeFinished 通知
    pub fn feed_chunk_ext(&mut self, chunk: &[f32], is_last: bool) -> Vec<StreamEvent> {
        let mut events = Vec::new();
        let chunk_len = chunk.len();

        let write_start = self.samples_in_cycle.min(NMAX);
        let write_end = (write_start + chunk_len).min(NMAX);
        let copy_count = write_end - write_start;

        if copy_count > 0 {
            self.audio_buffer[write_start..write_end].copy_from_slice(&chunk[..copy_count]);
        }
        self.samples_in_cycle += chunk_len;

        // 里程碑 1：前导码阶段完成 (t >= 1.6s, 即 19200 采样点)
        if !self.preamble_fired && self.samples_in_cycle >= 19200 {
            self.preamble_fired = true;
            let active_freqs = self.detect_active_frequencies();
            if !active_freqs.is_empty() {
                events.push(StreamEvent::PreambleDetected {
                    active_frequencies: active_freqs,
                });
            }
        }

        // 里程碑 2：提前解码时机 (t >= 11.52s, 即 138240 采样点，第 72 符号到达)
        if !self.early_fired && self.samples_in_cycle >= 138240 && !is_last {
            self.early_fired = true;
            let mut early_cfg = self.config.clone();
            early_cfg.passes = 1;
            let decodes = self.pipeline.decode(&self.audio_buffer, &early_cfg);
            self.early_results = decodes.clone();
            if !decodes.is_empty() {
                events.push(StreamEvent::EarlyDecoded(decodes));
            }
        }

        let current_time_sec = (self.samples_in_cycle as f32) / 12000.0;

        // 里程碑 3：信号发射全部结束时刻 (t >= 12.64s, 即 151680 采样点) 或 主动标记为最后一帧
        if !self.cycle_fired && (self.samples_in_cycle >= 151680 || is_last) {
            self.cycle_fired = true;
            let final_decodes = self.pipeline.decode(&self.audio_buffer, &self.config);
            self.last_completed_count = final_decodes.len();
            events.push(StreamEvent::CycleCompleted(final_decodes));

            // 里程碑 4：本轮解码完全结束通知
            self.finished_fired = true;
            events.push(StreamEvent::DecodeFinished {
                total_signals: self.last_completed_count,
                audio_duration_sec: current_time_sec,
                is_last_chunk: is_last,
            });
        } else if is_last && !self.finished_fired {
            // 若此前已触发过 CycleCompleted，但现在收到了最后一帧标记
            self.finished_fired = true;
            events.push(StreamEvent::DecodeFinished {
                total_signals: self.last_completed_count,
                audio_duration_sec: current_time_sec,
                is_last_chunk: true,
            });
        }

        // 周期超时自动轮转 (15.0s, 即 180,000 点)
        if self.samples_in_cycle >= NMAX {
            self.reset_cycle();
        }

        events
    }

    /// 流式喂入音频切片，返回原始事件列表 (保持向后兼容)
    pub fn feed_chunk(&mut self, chunk: &[f32]) -> Vec<StreamEvent> {
        self.feed_chunk_ext(chunk, false)
    }

    /// 流式喂入最后一包音频切片，并强制触发扫尾解码与结束通知
    pub fn feed_chunk_last(&mut self, chunk: &[f32]) -> Vec<StreamEvent> {
        self.feed_chunk_ext(chunk, true)
    }

    /// 带有 is_last 标志的结构化流式喂流接口
    pub fn feed_chunk_structured_ext(&mut self, chunk: &[f32], is_last: bool) -> Vec<StreamDecodedEvent> {
        let raw_events = self.feed_chunk_ext(chunk, is_last);
        let current_time_sec = (self.samples_in_cycle as f32) / 12000.0;
        let offset = self.window_start_offset;

        raw_events
            .into_iter()
            .map(|ev| match ev {
                StreamEvent::PreambleDetected { active_frequencies } => {
                    StreamDecodedEvent::PreambleDetected {
                        active_frequencies,
                        time_sec: current_time_sec,
                    }
                }
                StreamEvent::EarlyDecoded(signals) => {
                    let structured: Vec<Ft8DecodedMessage> = signals
                        .into_iter()
                        .map(|s| Ft8DecodedMessage::parse(s.dt, s.snr, s.freq, &s.message, offset))
                        .collect();
                    StreamDecodedEvent::EarlyDecoded {
                        signals: structured,
                        time_sec: current_time_sec,
                    }
                }
                StreamEvent::CycleCompleted(signals) => {
                    let structured: Vec<Ft8DecodedMessage> = signals
                        .into_iter()
                        .map(|s| Ft8DecodedMessage::parse(s.dt, s.snr, s.freq, &s.message, offset))
                        .collect();
                    StreamDecodedEvent::CycleCompleted {
                        all_signals: structured,
                        time_sec: current_time_sec,
                    }
                }
                StreamEvent::DecodeFinished {
                    total_signals,
                    audio_duration_sec,
                    is_last_chunk,
                } => StreamDecodedEvent::DecodeFinished {
                    total_signals,
                    audio_duration_sec,
                    is_last_chunk,
                },
            })
            .collect()
    }

    /// 流式喂入音频切片，返回带真实窗口 DT 校正与 9 项完整结构化字段的事件列表 (默认 is_last=false)
    pub fn feed_chunk_structured(&mut self, chunk: &[f32]) -> Vec<StreamDecodedEvent> {
        self.feed_chunk_structured_ext(chunk, false)
    }

    /// 支持带 is_last 标志和自定义回调句柄 (Callback) 的流式喂流接口
    pub fn feed_chunk_with_callback_ext<F>(&mut self, chunk: &[f32], is_last: bool, mut callback: F)
    where
        F: FnMut(StreamDecodedEvent),
    {
        let events = self.feed_chunk_structured_ext(chunk, is_last);
        for ev in events {
            callback(ev);
        }
    }

    /// 支持用户自定义回调句柄 (Callback) 的流式喂流接口 (默认 is_last=false)
    ///
    /// # 示例
    /// ```no_run
    /// use rust_ft8::demodulate::{DecoderConfig, StreamingFt8Receiver, StreamDecodedEvent};
    ///
    /// let mut receiver = StreamingFt8Receiver::with_window_offset(DecoderConfig::default(), -0.9);
    /// let chunk = [0.0f32; 1920]; // 160ms 音频帧
    ///
    /// receiver.feed_chunk_with_callback(&chunk, |event| {
    ///     match event {
    ///         StreamDecodedEvent::EarlyDecoded { signals, time_sec } => {
    ///             println!("提前输出 {} 条强信号 (t={:.2}s)", signals.len(), time_sec);
    ///         }
    ///         StreamDecodedEvent::CycleCompleted { all_signals, .. } => {
    ///             println!("全量完成 {} 条信号", all_signals.len());
    ///         }
    ///         StreamDecodedEvent::DecodeFinished { total_signals, is_last_chunk, .. } => {
    ///             println!("解码结束通知：共解出 {} 条信号 (最后一帧={})", total_signals, is_last_chunk);
    ///         }
    ///         _ => {}
    ///     }
    /// });
    /// ```
    pub fn feed_chunk_with_callback<F>(&mut self, chunk: &[f32], callback: F)
    where
        F: FnMut(StreamDecodedEvent),
    {
        self.feed_chunk_with_callback_ext(chunk, false, callback);
    }

    /// 主动强制结束本轮时隙解码流程 (即使尚未输入完整 15 秒音频)，返回事件列表
    pub fn finish(&mut self) -> Vec<StreamDecodedEvent> {
        self.feed_chunk_structured_ext(&[], true)
    }

    /// 主动强制结束本轮时隙解码流程并通过回调输出事件
    pub fn finish_with_callback<F>(&mut self, callback: F)
    where
        F: FnMut(StreamDecodedEvent),
    {
        self.feed_chunk_with_callback_ext(&[], true, callback);
    }

    /// 快速粗扫描当前音频流中的活跃载波频点
    fn detect_active_frequencies(&self) -> Vec<f32> {
        let candidates = self.pipeline.searcher().find_candidates(
            &self.audio_buffer,
            self.config.nfa,
            self.config.nfb,
            self.config.sync_min + 0.3,
            60,
        );
        let mut freqs: Vec<f32> = candidates.into_iter().map(|c| c.freq).collect();
        freqs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        freqs.dedup_by(|a, b| (*a - *b).abs() < 10.0);
        freqs
    }
}
