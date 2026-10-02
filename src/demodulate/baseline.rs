//! FT8 全局频响底噪基线估计模块 (完全对标 WSJT-X get_spectrum_baseline.f90 / baseline.f90)
//!
//! 在全频带 (0 ~ 3500 Hz) 上计算多时隙平均加窗功率谱，
//! 采用 10% 分位数下包络线与 4 阶正交多项式拟合出平滑的信道物理背景底噪基准 sbase，
//! 从而彻底免受密集 FT8 信号邻信道干扰 (ACI) 的污染，提供与 WSJT-X 官方完全一致的标准 SNR 评估基线。

use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

pub const NFFT1: usize = 3840;
pub const NH1: usize = NFFT1 / 2; // 1920
pub const NST: usize = NFFT1 / 2; // 1920 步长 (50% 重叠)
pub const NF: usize = 93;         // 93 帧覆盖 15 秒

/// 全局背景谱底噪估计上下文
pub struct BaselineEstimator {
    fft: Arc<dyn Fft<f32>>,
    window: [f32; NFFT1],
}

/// 拟合生成的平滑底噪谱对象
#[derive(Debug, Clone)]
pub struct BaselineSpectrum {
    pub sbase: Vec<f32>,
    pub df: f32,
    pub ia: usize,
    pub ib: usize,
}

impl BaselineEstimator {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(NFFT1);

        // 生成 Nuttall 4 项余弦窗 (对标 WSJT-X nuttal_window.f90)
        let mut window = [0.0f32; NFFT1];
        let a0 = 0.3635819f32;
        let a1 = -0.4891775f32;
        let a2 = 0.1365995f32;
        let a3 = -0.0106411f32;

        let mut sum_win = 0.0f32;
        for i in 0..NFFT1 {
            let theta = 2.0 * PI * (i as f32) / (NFFT1 as f32);
            let w = a0 + a1 * theta.cos() + a2 * (2.0 * theta).cos() + a3 * (3.0 * theta).cos();
            window[i] = w;
            sum_win += w;
        }

        // 窗函数能量归一化 (对标 WSJT-X: window = window/sum * NSPS*2/300.0)
        let fac = (1920.0 * 2.0 / 300.0) / sum_win;
        for w in window.iter_mut() {
            *w *= fac;
        }

        Self { fft, window }
    }

    /// 对整段 15 秒音频估计物理背景底噪谱 (对标 get_spectrum_baseline.f90)
    pub fn estimate_baseline(&self, audio: &[f32], nfa: f32, nfb: f32) -> BaselineSpectrum {
        let df = 12000.0 / (NFFT1 as f32); // 3.125 Hz
        let mut savg = vec![0.0f32; NH1];
        let mut buf = vec![Complex32::new(0.0, 0.0); NFFT1];

        for j in 0..NF {
            let ia = j * NST;
            let _ib = ia + NFFT1;
            if ia >= audio.len() {
                break;
            }

            for k in 0..NFFT1 {
                let idx = ia + k;
                if idx < audio.len() {
                    buf[k] = Complex32::new(audio[idx] * self.window[k], 0.0);
                } else {
                    buf[k] = Complex32::new(0.0, 0.0);
                }
            }

            self.fft.process(&mut buf);

            for i in 0..NH1 {
                savg[i] += buf[i].norm_sqr();
            }
        }

        // 确定分析频带范围
        let mut fa = nfa.max(100.0);
        let mut fb = nfb.min(4910.0);
        if fa >= fb {
            fa = 100.0;
            fb = 3500.0;
        }

        let ia = ((fa / df).round() as usize).max(1);
        let ib = ((fb / df).round() as usize).min(NH1 - 1);

        // 转换为对数谱 (dB) (对标 baseline.f90)
        let mut s_db = vec![0.0f32; NH1];
        for i in ia..=ib {
            s_db[i] = 10.0 * savg[i].max(1e-12).log10();
        }

        // 分 10 段提取 10% 分位数下包络线数据点
        let nseg = 10usize;
        let nlen = (ib - ia + 1) / nseg;
        let i0 = (ib - ia + 1) as f64 / 2.0;

        let mut xs = Vec::with_capacity(1000);
        let mut ys = Vec::with_capacity(1000);

        for n in 0..nseg {
            let ja = ia + n * nlen;
            let jb = if n == nseg - 1 { ib } else { ja + nlen - 1 };
            if ja >= jb {
                continue;
            }

            let mut seg_vals: Vec<f32> = (ja..=jb).map(|idx| s_db[idx]).collect();
            seg_vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let p10_idx = ((seg_vals.len() as f32) * 0.10).round() as usize;
            let base = seg_vals[p10_idx.min(seg_vals.len().saturating_sub(1))];

            for idx in ja..=jb {
                if s_db[idx] <= base && xs.len() < 1000 {
                    xs.push((idx - ia) as f64 - i0);
                    ys.push(s_db[idx] as f64);
                }
            }
        }

        // 4 阶多项式最小二乘拟合 (5 个参数: a0..a4)
        let coeffs = polyfit_order4(&xs, &ys);

        // 重构全频带平滑底噪 sbase
        let mut sbase = vec![0.0f32; NH1];
        for i in 0..NH1 {
            let t = (i as f64) - (ia as f64) - i0;
            let val = coeffs[0]
                + t * (coeffs[1] + t * (coeffs[2] + t * (coeffs[3] + t * coeffs[4])))
                + 0.65; // WSJT-X 经验偏移常数 +0.65 dB
            sbase[i] = val as f32;
        }

        BaselineSpectrum {
            sbase,
            df,
            ia,
            ib,
        }
    }
}

impl BaselineSpectrum {
    /// 计算指定频率处的物理参考噪声功率 xbase (对标 ft8_decode.f90 L201)
    pub fn get_xbase(&self, freq: f32) -> f32 {
        let bin = (freq / self.df).round() as usize;
        let clamped_bin = bin.clamp(self.ia, self.ib);
        let sbase_db = self.sbase[clamped_bin];
        10.0_f32.powf(0.1 * (sbase_db - 40.0))
    }
}

/// 4 阶多项式正规方程组最小二乘求解 (纯 Rust 高斯消元)
fn polyfit_order4(xs: &[f64], ys: &[f64]) -> [f64; 5] {
    if xs.len() < 5 {
        return [0.0; 5];
    }

    let nterms = 5;
    let mut sumx = [0.0f64; 10]; // x^0 到 x^8
    let mut sumy = [0.0f64; 5];  // y*x^0 到 y*x^4

    for i in 0..xs.len() {
        let x = xs[i];
        let y = ys[i];

        let mut xp = 1.0f64;
        for p in 0..9 {
            sumx[p] += xp;
            xp *= x;
        }

        let mut yxp = y;
        for p in 0..nterms {
            sumy[p] += yxp;
            yxp *= x;
        }
    }

    // 构建 5x5 正规方程矩阵 A 和右侧向量 b
    let mut a = [[0.0f64; 5]; 5];
    let mut b = [0.0f64; 5];
    for r in 0..nterms {
        for c in 0..nterms {
            a[r][c] = sumx[r + c];
        }
        b[r] = sumy[r];
    }

    // 带主元高斯消元法求解
    for i in 0..nterms {
        let mut max_row = i;
        let mut max_val = a[i][i].abs();
        for r in (i + 1)..nterms {
            if a[r][i].abs() > max_val {
                max_val = a[r][i].abs();
                max_row = r;
            }
        }

        if max_val < 1e-12 {
            continue;
        }

        a.swap(i, max_row);
        b.swap(i, max_row);

        for r in (i + 1)..nterms {
            let factor = a[r][i] / a[i][i];
            for c in i..nterms {
                a[r][c] -= factor * a[i][c];
            }
            b[r] -= factor * b[i];
        }
    }

    // 回代求解
    let mut coeffs = [0.0f64; 5];
    for i in (0..nterms).rev() {
        if a[i][i].abs() > 1e-12 {
            let mut sum = b[i];
            for c in (i + 1)..nterms {
                sum -= a[i][c] * coeffs[c];
            }
            coeffs[i] = sum / a[i][i];
        }
    }

    coeffs
}

impl Default for BaselineEstimator {
    fn default() -> Self {
        Self::new()
    }
}
