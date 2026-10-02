//! FT8 粗同步与候选信号搜索模块 (完全对标 WSJT-X sync8.f90 / sync8d.f90)
//!
//! 在 100 ~ 3500 Hz 范围内搜索 7x7 Costas 同步图案，
//! 计算时频相关二维谱，经 40% 分位数基线归一化后，检出候选信号的时延 (DT)、频偏 (DF) 及同步度量 (Sync)。

use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::f32::consts::PI;
use std::sync::Arc;

use crate::constants::COSTAS_PATTERN;

pub const NMAX: usize = 15 * 12000; // 180000
pub const NSPS: usize = 1920;
pub const NFFT1: usize = 3840;      // 2 * NSPS
pub const NH1: usize = NFFT1 / 2;    // 1920
pub const NSTEP: usize = NSPS / 4;   // 480
pub const NHSYM: usize = NMAX / NSTEP - 3; // 372
pub const JZ: isize = 62;           // 最大搜索时延 +/- 2.5 秒对应的步数

/// 候选信号描述
#[derive(Debug, Clone, Copy)]
pub struct Candidate {
    /// 载波频率 (Hz)
    pub freq: f32,
    /// 相对时隙 0.5s 的时间偏差 (秒)
    pub dt: f32,
    /// 同步质量度量 (Costas 信噪比)
    pub sync: f32,
}

/// 粗同步搜索上下文
pub struct SyncSearcher {
    fft_3840: Arc<dyn Fft<f32>>,
    cos_table: [[Complex32; 32]; 7],
}

impl SyncSearcher {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_3840 = planner.plan_fft_forward(NFFT1);

        // 预计算 200 Hz 下 7 个 Costas 音调对应的 32 点复数波形
        let mut cos_table = [[Complex32::new(0.0, 0.0); 32]; 7];
        for i in 0..7 {
            let tone = COSTAS_PATTERN[i] as f32;
            let dphi = 2.0 * PI * tone / 32.0;
            for j in 0..32 {
                let phi = (j as f32) * dphi;
                cos_table[i][j] = Complex32::new(phi.cos(), phi.sin());
            }
        }

        Self {
            fft_3840,
            cos_table,
        }
    }

    /// 在整段 15 秒音频中搜索 FT8 候选信号
    ///
    /// # 参数
    /// - `audio`: 180,000 采样点音频数据
    /// - `nfa`: 最低搜索频率 (Hz，默认 100)
    /// - `nfb`: 最高搜索频率 (Hz，默认 3500)
    /// - `sync_min`: 最小同步阈值 (通常 1.3 ~ 1.6)
    /// - `max_cand`: 最大返回候选数
    pub fn find_candidates(
        &self,
        audio: &[f32],
        nfa: f32,
        nfb: f32,
        sync_min: f32,
        max_cand: usize,
    ) -> Vec<Candidate> {
        let df = 12000.0 / (NFFT1 as f32); // 3.125 Hz
        let tstep = (NSTEP as f32) / 12000.0; // 0.040s

        // 1. 滑动计算 NHSYM = 372 个 3840 点 FFT 功率谱
        let mut s = vec![vec![0.0f32; NH1]; NHSYM];
        let mut buf = vec![Complex32::new(0.0, 0.0); NFFT1];
        let fac = 1.0 / 300.0;

        for j in 0..NHSYM {
            let ia = j * NSTEP;
            let ib = (ia + NSPS).min(audio.len());
            for k in 0..NFFT1 {
                if k < (ib - ia) {
                    buf[k] = Complex32::new(fac * audio[ia + k], 0.0);
                } else {
                    buf[k] = Complex32::new(0.0, 0.0);
                }
            }

            self.fft_3840.process(&mut buf);

            for i in 0..NH1 {
                s[j][i] = buf[i].norm_sqr();
            }
        }

        let ia = ((nfa / df).round() as usize).max(1);
        let ib = ((nfb / df).round() as usize).min(NH1 - 16);
        if ia >= ib {
            return Vec::new();
        }

        let nssy = 4usize; // 每符号步数 (1920 / 480)
        let nfos = 2usize; // 每音调频偏对应的 FFT 频点数 (6.25 / 3.125)
        let jstrt = (0.5 / tstep).round() as isize; // 12

        let num_bins = ib - ia + 1;
        let mut red = vec![0.0f32; num_bins];
        let mut jpeak = vec![0isize; num_bins];
        let mut red2 = vec![0.0f32; num_bins];
        let mut jpeak2 = vec![0isize; num_bins];

        let mlag = 10isize;
        let mlag2 = JZ;

        // 2. 搜索 Costas 图案相关峰
        for (bin_idx, i) in (ia..=ib).enumerate() {
            let mut sync_row = vec![0.0f32; (2 * JZ + 1) as usize];

            for j in -JZ..=JZ {
                let mut ta = 0.0f32;
                let mut tb = 0.0f32;
                let mut tc = 0.0f32;
                let mut t0a = 0.0f32;
                let mut t0b = 0.0f32;
                let mut t0c = 0.0f32;

                for n in 0..7 {
                    let tone_offset = nfos * (COSTAS_PATTERN[n] as usize);
                    let m = j + jstrt + (nssy * n) as isize;

                    if m >= 0 && (m as usize) < NHSYM {
                        let mu = m as usize;
                        ta += s[mu][i + tone_offset];
                        for t in 0..7 {
                            t0a += s[mu][i + nfos * t];
                        }
                    }

                    let mb = m + (nssy * 36) as isize;
                    if mb >= 0 && (mb as usize) < NHSYM {
                        let mbu = mb as usize;
                        tb += s[mbu][i + tone_offset];
                        for t in 0..7 {
                            t0b += s[mbu][i + nfos * t];
                        }
                    }

                    let mc = m + (nssy * 72) as isize;
                    if mc >= 0 && (mc as usize) < NHSYM {
                        let mcu = mc as usize;
                        tc += s[mcu][i + tone_offset];
                        for t in 0..7 {
                            t0c += s[mcu][i + nfos * t];
                        }
                    }
                }

                // 3 个 Costas 块相关
                let t_abc = ta + tb + tc;
                let t0_abc = (t0a + t0b + t0c - t_abc) / 6.0;
                let sync_abc = if t0_abc > 1e-6 { t_abc / t0_abc } else { 0.0 };

                // 2 个 Costas 块相关 (B + C 块，适应延迟开始的发射)
                let t_bc = tb + tc;
                let t0_bc = (t0b + t0c - t_bc) / 6.0;
                let sync_bc = if t0_bc > 1e-6 { t_bc / t0_bc } else { 0.0 };

                let val = sync_abc.max(sync_bc);
                let row_idx = (j + JZ) as usize;
                sync_row[row_idx] = val;
            }

            // 在 [-10, 10] 范围找峰值
            let mut max1 = 0.0f32;
            let mut jp1 = 0isize;
            for j in -mlag..=mlag {
                let v = sync_row[(j + JZ) as usize];
                if v > max1 {
                    max1 = v;
                    jp1 = j;
                }
            }
            red[bin_idx] = max1;
            jpeak[bin_idx] = jp1;

            // 在 [-62, 62] 范围找峰值
            let mut max2 = 0.0f32;
            let mut jp2 = 0isize;
            for j in -mlag2..=mlag2 {
                let v = sync_row[(j + JZ) as usize];
                if v > max2 {
                    max2 = v;
                    jp2 = j;
                }
            }
            red2[bin_idx] = max2;
            jpeak2[bin_idx] = jp2;
        }

        // 3. 计算 40% 分位数基线并归一化 (完全对标 WSJT-X 算法)
        let base1 = calc_percentile(&red, 0.40);
        let base2 = calc_percentile(&red2, 0.40);

        let mut raw_candidates: Vec<Candidate> = Vec::new();

        for (bin_idx, i) in (ia..=ib).enumerate() {
            let r1 = if base1 > 1e-6 { red[bin_idx] / base1 } else { 0.0 };
            if r1 >= sync_min {
                raw_candidates.push(Candidate {
                    freq: (i as f32) * df,
                    dt: ((jpeak[bin_idx] as f32) - 0.5) * tstep,
                    sync: r1,
                });
            }

            if jpeak2[bin_idx] != jpeak[bin_idx] {
                let r2 = if base2 > 1e-6 { red2[bin_idx] / base2 } else { 0.0 };
                if r2 >= sync_min {
                    raw_candidates.push(Candidate {
                        freq: (i as f32) * df,
                        dt: ((jpeak2[bin_idx] as f32) - 0.5) * tstep,
                        sync: r2,
                    });
                }
            }
        }

        // 4. 去重：在 4 Hz 和 0.04 秒范围内的近邻峰仅保留最高者
        raw_candidates.sort_by(|a, b| b.sync.partial_cmp(&a.sync).unwrap());
        let mut pruned: Vec<Candidate> = Vec::new();

        for cand in raw_candidates {
            let is_dupe = pruned.iter().any(|p| {
                (p.freq - cand.freq).abs() < 4.0 && (p.dt - cand.dt).abs() < 0.04
            });
            if !is_dupe {
                pruned.push(cand);
                if pruned.len() >= max_cand {
                    break;
                }
            }
        }

        pruned
    }

    /// 在 200 Hz 复数基带信号中精细对其时间与频率 (对标 sync8d.f90)
    pub fn fine_sync(
        &self,
        cd0: &[Complex32],
        init_dt: f32,
    ) -> (usize, f32, f32) {
        let fs2 = 200.0f32;
        let np2 = 2812usize;

        // 1. 粗时延搜索 (+/- 10 个 200 Hz 采样点 = +/- 50ms)
        let i0 = (((init_dt + 0.5) * fs2).round() as isize).max(10);
        let mut smax = 0.0f32;
        let mut ibest = i0 as usize;

        for idt in (i0 - 10)..=(i0 + 10) {
            if idt >= 0 {
                let sync = self.calc_sync8d(cd0, idt as usize, 0.0, np2);
                if sync > smax {
                    smax = sync;
                    ibest = idt as usize;
                }
            }
        }

        // 2. 精细频偏搜索 (+/- 2.5 Hz，步长 0.5 Hz)
        let mut delf_best = 0.0f32;
        smax = 0.0;
        for ifr in -5..=5 {
            let delf = (ifr as f32) * 0.5;
            let sync = self.calc_sync8d(cd0, ibest, delf, np2);
            if sync > smax {
                smax = sync;
                delf_best = delf;
            }
        }

        // 3. 微调时延 (+/- 4 点)
        let mut final_ibest = ibest;
        smax = 0.0;
        for idt in -4..=4 {
            let idx = (ibest as isize + idt).max(0) as usize;
            let sync = self.calc_sync8d(cd0, idx, delf_best, np2);
            if sync > smax {
                smax = sync;
                final_ibest = idx;
            }
        }

        (final_ibest, delf_best, smax)
    }

    fn calc_sync8d(&self, cd0: &[Complex32], i0: usize, delf: f32, np2: usize) -> f32 {
        let mut sync = 0.0f32;
        let dt2 = 1.0 / 200.0f32;
        let dphi = 2.0 * PI * delf * dt2;

        for i in 0..7 {
            let i1 = i0 + i * 32;
            let i2 = i1 + 36 * 32;
            let i3 = i1 + 72 * 32;

            let mut z1 = Complex32::new(0.0, 0.0);
            let mut z2 = Complex32::new(0.0, 0.0);
            let mut z3 = Complex32::new(0.0, 0.0);

            for j in 0..32 {
                let twk = Complex32::new(((j as f32) * dphi).cos(), ((j as f32) * dphi).sin());
                let ref_val = self.cos_table[i][j] * twk;

                if i1 + j < np2 && i1 + j < cd0.len() {
                    z1 += cd0[i1 + j] * ref_val.conj();
                }
                if i2 + j < np2 && i2 + j < cd0.len() {
                    z2 += cd0[i2 + j] * ref_val.conj();
                }
                if i3 + j < np2 && i3 + j < cd0.len() {
                    z3 += cd0[i3 + j] * ref_val.conj();
                }
            }

            sync += z1.norm_sqr() + z2.norm_sqr() + z3.norm_sqr();
        }

        sync
    }
}

fn calc_percentile(arr: &[f32], pct: f32) -> f32 {
    if arr.is_empty() {
        return 1.0;
    }
    let mut sorted = arr.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() as f32) * pct).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

impl Default for SyncSearcher {
    fn default() -> Self {
        Self::new()
    }
}
