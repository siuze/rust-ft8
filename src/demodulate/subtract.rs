//! FT8 已解码信号时域消减模块 (对标 WSJT-X subtractft8.f90)
//!
//! 在多轮解码流程中，对已正确解码的强信号进行时域复包络重构并从音频流中扣除，
//! 消除强信号对弱信号的频谱泄漏与旁瓣压制，实现深信噪比与密集通联环境下的多信号并发解出。

use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

use crate::constants::{NSPS, NUM_SYMBOLS};
use crate::modulate::gfsk_pulse;

pub const NMAX: usize = 15 * 12000;       // 180,000
pub const NFRAME: usize = NSPS * NUM_SYMBOLS; // 1920 * 79 = 151,680
pub const NFILT: usize = 4000;

pub struct SignalSubtracter {
    fft_180k_forward: Arc<dyn Fft<f32>>,
    fft_180k_inverse: Arc<dyn Fft<f32>>,
    filter_freq: Vec<Complex32>,
    end_correction: [f32; NFILT / 2 + 1],
    pulse_scaled: Vec<f32>,
    scratch_len: usize,
}

/// 工作线程复用缓存，彻底消除多信号并发重构时的重复堆分配
pub struct SubtractionBuffers {
    pub camp: Vec<Complex32>,
    pub scratch: Vec<Complex32>,
    pub dphi: Vec<f32>,
    pub cref: Vec<Complex32>,
}

impl SignalSubtracter {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_180k_forward = planner.plan_fft_forward(NMAX);
        let fft_180k_inverse = planner.plan_fft_inverse(NMAX);
        let scratch_len = fft_180k_forward
            .get_inplace_scratch_len()
            .max(fft_180k_inverse.get_inplace_scratch_len());

        // 构造 NFILT = 4000 点余弦平方低通平滑窗
        let mut window = [0.0f32; NFILT + 1];
        let mut sumw = 0.0f32;
        let half = (NFILT / 2) as isize;

        for (idx, j) in (-half..=half).enumerate() {
            let w = ((PI * (j as f32) / (NFILT as f32)).cos()).powi(2);
            window[idx] = w;
            sumw += w;
        }

        // 归一化并循环位移使中心对齐 0 频 (Fortran cshift 正位移向左移动元素，对应 rotate_left)
        let mut cw = vec![Complex32::new(0.0, 0.0); NMAX];
        for (i, &w) in window.iter().enumerate() {
            cw[i] = Complex32::new(w / sumw, 0.0);
        }
        cw.rotate_left((NFILT / 2 + 1) % NMAX);

        // 变换到频域
        fft_180k_forward.process(&mut cw);
        let fac = 1.0 / (NMAX as f32);
        for c in cw.iter_mut() {
            *c *= fac;
        }

        // 边界校正系数
        let mut end_correction = [0.0f32; NFILT / 2 + 1];
        for j in 0..=NFILT / 2 {
            let mut partial_sum = 0.0f32;
            for k in j..=NFILT / 2 {
                let idx = (k as isize + half) as usize;
                if idx < window.len() {
                    partial_sum += window[idx];
                }
            }
            let denom = 1.0 - (partial_sum / sumw);
            end_correction[j] = if denom > 0.01 { 1.0 / denom } else { 1.0 };
        }

        // 预计算 GFSK pulse 表与 dphi 缩放系数，消除每次消减时的 5760 次 erf() 计算
        let pulse = gfsk_pulse(NSPS, 2.0);
        let dphi_peak = 2.0 * PI / (NSPS as f32);
        let pulse_scaled: Vec<f32> = pulse.iter().map(|&p| dphi_peak * p).collect();

        Self {
            fft_180k_forward,
            fft_180k_inverse,
            filter_freq: cw,
            end_correction,
            pulse_scaled,
            scratch_len,
        }
    }

    /// 为工作线程创建一组专用的重构工作缓存（单个线程只初始化一次，后续零堆分配）
    pub fn create_buffers(&self) -> SubtractionBuffers {
        SubtractionBuffers {
            camp: vec![Complex32::new(0.0, 0.0); NMAX],
            scratch: vec![Complex32::new(0.0, 0.0); self.scratch_len],
            dphi: vec![0.0f32; NFRAME + 2 * NSPS],
            cref: vec![Complex32::new(0.0, 0.0); NFRAME],
        }
    }

    /// 从当前音频数据 `audio` 中扣除指定的已解码信号
    pub fn subtract_signal(
        &self,
        audio: &mut [f32],
        tones: &[u8; NUM_SYMBOLS],
        f0: f32,
        ibest_200: isize,
    ) {
        let (nstart, wave) = self.reconstruct_waveform(tones, f0, ibest_200, audio);
        for (i, &rec) in wave.iter().enumerate() {
            let j = nstart + (i as isize);
            if j >= 0 && (j as usize) < audio.len() {
                audio[j as usize] -= rec;
            }
        }
    }

    /// 并发安全的信号波形重构（包装函数）
    pub fn reconstruct_waveform(
        &self,
        tones: &[u8; NUM_SYMBOLS],
        f0: f32,
        ibest_200: isize,
        audio_snapshot: &[f32],
    ) -> (isize, Vec<f32>) {
        let mut bufs = self.create_buffers();
        self.reconstruct_waveform_with_buf(tones, f0, 0.0, ibest_200, audio_snapshot, &mut bufs)
    }

    /// 零堆分配的高性能波形重构：复用工作线程持有的缓存，支持频漂 (drift) 联合线性调频
    pub fn reconstruct_waveform_with_buf(
        &self,
        tones: &[u8; NUM_SYMBOLS],
        f0: f32,
        drift: f32,
        ibest_200: isize,
        audio_snapshot: &[f32],
        bufs: &mut SubtractionBuffers,
    ) -> (isize, Vec<f32>) {
        let nstart = ibest_200 * 60;
        let mut reconstructed = vec![0.0f32; NFRAME];

        if audio_snapshot.len() < NMAX {
            return (nstart, reconstructed);
        }

        // 1. 合成复数参考信号 cref (复用预分配 dphi 与 cref 缓存，引入频漂补偿)
        self.synth_complex_ref_into(tones, f0, drift, &mut bufs.dphi, &mut bufs.cref);

        // 2. 解调出基带复包络 camp(i) = dd(nstart + i) * conj(cref(i))
        bufs.camp.fill(Complex32::new(0.0, 0.0));
        for i in 0..NFRAME {
            let j = nstart + (i as isize);
            if j >= 0 && (j as usize) < NMAX {
                bufs.camp[i] = Complex32::new(audio_snapshot[j as usize], 0.0) * bufs.cref[i].conj();
            }
        }

        // 3. 频域滤波：cfilt = IFFT( FFT(camp) * filter_freq )
        self.fft_180k_forward.process_with_scratch(&mut bufs.camp, &mut bufs.scratch);
        for i in 0..NMAX {
            bufs.camp[i] *= self.filter_freq[i];
        }
        self.fft_180k_inverse.process_with_scratch(&mut bufs.camp, &mut bufs.scratch);

        // 4. 首尾边界校正
        let half = NFILT / 2;
        for j in 0..=half {
            bufs.camp[j] *= self.end_correction[j];
            if NFRAME > j + 1 {
                bufs.camp[NFRAME - 1 - j] *= self.end_correction[j];
            }
        }

        // 5. 生成时域重构分量: 2.0 * Re( camp[i] * cref[i] )
        for i in 0..NFRAME {
            reconstructed[i] = (bufs.camp[i] * bufs.cref[i]).re * 2.0;
        }

        (nstart, reconstructed)
    }

    /// 合成 12000 Hz 连续相位复数参考波形，写入指定缓冲区 (支持 drift 线性调频)
    fn synth_complex_ref_into(
        &self,
        tones: &[u8; NUM_SYMBOLS],
        f0: f32,
        drift: f32,
        dphi: &mut [f32],
        cref: &mut [Complex32],
    ) {
        let n_spsym = NSPS; // 1920
        let n_wave = NFRAME;
        let base_dphi = 2.0 * PI * f0 / 12000.0f32;

        dphi.fill(base_dphi);

        for (i, &tone) in tones.iter().enumerate() {
            let ib = i * n_spsym;
            let tone_f = tone as f32;
            for j in 0..(3 * n_spsym) {
                dphi[ib + j] += tone_f * self.pulse_scaled[j];
            }
        }

        let t0 = tones[0] as f32;
        let t_last = tones[NUM_SYMBOLS - 1] as f32;
        for j in 0..(2 * n_spsym) {
            dphi[j] += t0 * self.pulse_scaled[j + n_spsym];
            dphi[j + n_wave] += t_last * self.pulse_scaled[j];
        }

        // 频漂线性相位调整
        if drift.abs() > 0.01 {
            let drift_fac = 2.0 * PI * drift / (12000.0 * (n_wave as f32));
            for k in 0..n_wave {
                dphi[k + n_spsym] += (k as f32) * drift_fac;
            }
        }

        let mut phi = 0.0f32;
        let two_pi = 2.0 * PI;

        for k in 0..n_wave {
            let (s, c) = phi.sin_cos();
            cref[k] = Complex32::new(c, s);
            phi = (phi + dphi[k + n_spsym]).rem_euclid(two_pi);
        }
    }
}

impl Default for SignalSubtracter {
    fn default() -> Self {
        Self::new()
    }
}
