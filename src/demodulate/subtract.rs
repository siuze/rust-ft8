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
}

impl SignalSubtracter {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_180k_forward = planner.plan_fft_forward(NMAX);
        let fft_180k_inverse = planner.plan_fft_inverse(NMAX);

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

        Self {
            fft_180k_forward,
            fft_180k_inverse,
            filter_freq: cw,
            end_correction,
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
        if audio.len() < NMAX {
            return;
        }

        // 1. 生成复数参考信号 cref
        let cref = synth_complex_ref(tones, f0);

        // 2. 解调出基带复包络 camp(i) = dd(nstart + i) * conj(cref(i))
        // 200 Hz 基带采样点 ibest 对应 12000 Hz 原始采样的精确点 ibest * 60 (对标 WSJT-X nstart = dt*12000 + 1)
        let nstart = ibest_200 * 60;
        let mut camp = vec![Complex32::new(0.0, 0.0); NMAX];

        for i in 0..NFRAME {
            let j = nstart + (i as isize);
            if j >= 0 && (j as usize) < NMAX {
                camp[i] = Complex32::new(audio[j as usize], 0.0) * cref[i].conj();
            }
        }

        // 3. 频域滤波：cfilt = IFFT( FFT(camp) * filter_freq )
        self.fft_180k_forward.process(&mut camp);
        for i in 0..NMAX {
            camp[i] *= self.filter_freq[i];
        }
        self.fft_180k_inverse.process(&mut camp);

        // 4. 首尾边界校正
        let half = NFILT / 2;
        for j in 0..=half {
            camp[j] *= self.end_correction[j];
            if NFRAME > j + 1 {
                camp[NFRAME - 1 - j] *= self.end_correction[j];
            }
        }

        // 5. 时域重构并扣除: audio[j] -= 2.0 * Re( camp[i] * cref[i] )
        for i in 0..NFRAME {
            let j = nstart + (i as isize);
            if j >= 0 && (j as usize) < NMAX {
                let reconstructed = (camp[i] * cref[i]).re * 2.0;
                audio[j as usize] -= reconstructed;
            }
        }
    }
}

/// 合成 12000 Hz 连续相位复数参考波形 cref (长度 NFRAME = 151,680)
fn synth_complex_ref(tones: &[u8; NUM_SYMBOLS], f0: f32) -> Vec<Complex32> {
    let n_spsym = NSPS; // 1920
    let n_wave = NFRAME;
    let symbol_bt = 2.0f32;
    let pulse = gfsk_pulse(n_spsym, symbol_bt);

    let dphi_peak = 2.0 * PI / (n_spsym as f32);
    let total_dphi_len = n_wave + 2 * n_spsym;
    let mut dphi = vec![2.0 * PI * f0 / 12000.0f32; total_dphi_len];

    for (i, &tone) in tones.iter().enumerate() {
        let ib = i * n_spsym;
        let tone_f = tone as f32;
        for j in 0..(3 * n_spsym) {
            dphi[ib + j] += dphi_peak * tone_f * pulse[j];
        }
    }

    let t0 = tones[0] as f32;
    let t_last = tones[NUM_SYMBOLS - 1] as f32;
    for j in 0..(2 * n_spsym) {
        dphi[j] += dphi_peak * t0 * pulse[j + n_spsym];
        dphi[j + n_wave] += dphi_peak * t_last * pulse[j];
    }

    let mut cref = vec![Complex32::new(0.0, 0.0); n_wave];
    let mut phi = 0.0f32;
    let two_pi = 2.0 * PI;

    for k in 0..n_wave {
        cref[k] = Complex32::new(phi.cos(), phi.sin());
        phi = (phi + dphi[k + n_spsym]).rem_euclid(two_pi);
    }

    cref
}

impl Default for SignalSubtracter {
    fn default() -> Self {
        Self::new()
    }
}
