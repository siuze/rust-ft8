//! FT8 基带下采样模块 (对标 WSJT-X ft8_downsample.f90)
//!
//! 将 12000 Hz 音频经 192000 点全长 FFT 变换至频域，
//! 对任意候选载频 f0 截取其基带信号并加余弦窗后，经 3200 点 IFFT 变换回时域，
//! 输出 200 Hz 采样率（每符号 32 个采样点，共 3200 个复数采样）的解析基带信号。

use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

pub const NMAX: usize = 15 * 12000; // 180000
pub const NFFT1: usize = 192000;     // 192000 / 60 = 3200
pub const NFFT2: usize = 3200;       // 200 Hz 下 16 秒 = 3200 点
pub const NSPS: usize = 1920;        // 12000 Hz 下每符号点数
pub const NSPS2: usize = 32;         // 200 Hz 下每符号点数 (1920 / 60)

/// 下采样器上下文结构体（预规划 FFT 计划与窗函数，避免重复分配）
pub struct Downsampler {
    fft1_forward: Arc<dyn Fft<f32>>,
    fft2_inverse: Arc<dyn Fft<f32>>,
    taper: [f32; 101],
    norm_fac: f32,
}

impl Downsampler {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft1_forward = planner.plan_fft_forward(NFFT1);
        let fft2_inverse = planner.plan_fft_inverse(NFFT2);

        let mut taper = [0.0f32; 101];
        for i in 0..=100 {
            taper[i] = 0.5 * (1.0 + ((i as f32) * PI / 100.0).cos());
        }

        let norm_fac = 1.0 / ((NFFT1 as f32) * (NFFT2 as f32)).sqrt();

        Self {
            fft1_forward,
            fft2_inverse,
            taper,
            norm_fac,
        }
    }

    /// 对 12000 Hz 原始时域音频信号计算 192000 点全长前向 FFT
    /// 输入 `audio` 至少包含 15 秒（180,000 点）数据
    pub fn compute_long_fft(&self, audio: &[f32]) -> Vec<Complex32> {
        let mut cx = vec![Complex32::new(0.0, 0.0); NFFT1];
        let copy_len = audio.len().min(NMAX);
        for i in 0..copy_len {
            cx[i] = Complex32::new(audio[i], 0.0);
        }
        self.fft1_forward.process(&mut cx);
        cx
    }

    /// 从预先计算的全长频域数据 `long_fft` 中，提取指定载频 `f0` 的 200 Hz 基带信号
    /// 输出 3200 点复数基带序列 `c1`
    pub fn downsample(&self, long_fft: &[Complex32], f0: f32) -> Vec<Complex32> {
        let mut c1 = vec![Complex32::new(0.0, 0.0); NFFT2];
        self.downsample_to_slice(long_fft, f0, &mut c1);
        c1
    }

    /// 零堆分配的高性能下采样版本，将结果写入调用者提供的 `out` 切片（长度至少为 3200）
    pub fn downsample_to_slice(&self, long_fft: &[Complex32], f0: f32, out: &mut [Complex32]) {
        assert!(out.len() >= NFFT2, "缓冲区长度必须至少为 3200");
        let df = 12000.0 / (NFFT1 as f32); // 0.0625 Hz
        let baud = 12000.0 / (NSPS as f32); // 6.25 Hz

        let i0 = (f0 / df).round() as isize;
        let ft = f0 + 8.5 * baud;
        let it = ((ft / df).round() as isize).min((NFFT1 / 2) as isize);
        let fb = f0 - 1.5 * baud;
        let ib = ((fb / df).round() as isize).max(1);

        out[..NFFT2].fill(Complex32::new(0.0, 0.0));
        if ib > it {
            return;
        }

        let mut k = 0usize;
        for i in ib..=it {
            if i >= 0 && (i as usize) < long_fft.len() {
                out[k] = long_fft[i as usize];
            }
            k += 1;
        }

        // 两端施加 100 点余弦平滑锥度
        if k > 100 {
            for i in 0..=100 {
                out[i] *= self.taper[100 - i];
            }
            for i in 0..=100 {
                if k > i + 1 {
                    out[k - 1 - i] *= self.taper[i];
                }
            }
        }

        // 循环位移，使 f0 对准直流 0 Hz (对应 Fortran cshift)
        let shift = (i0 - ib) as usize % NFFT2;
        out[..NFFT2].rotate_left(shift);

        // 3200 点 IFFT 逆变换回时域
        self.fft2_inverse.process(&mut out[..NFFT2]);

        // 幅度归一化
        for s in out[..NFFT2].iter_mut() {
            *s *= self.norm_fac;
        }
    }
}

impl Default for Downsampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_downsampler_init() {
        let ds = Downsampler::new();
        let dummy_audio = vec![0.0f32; 180000];
        let long_fft = ds.compute_long_fft(&dummy_audio);
        assert_eq!(long_fft.len(), 192000);

        let baseband = ds.downsample(&long_fft, 1000.0);
        assert_eq!(baseband.len(), 3200);
    }
}
