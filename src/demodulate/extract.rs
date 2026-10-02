//! FT8 符号能量提取与多符号软度量 (LLR) 译码模块 (对标 WSJT-X ft8b.f90)
//!
//! 对 200 Hz 基带信号执行 32 点 FFT 提取 79 个符号的 8-FSK 复数谱，
//! 支持 1-符号、2-符号与 3-符号相干累加度量提取 (Pass 1..4)，
//! 计算 LLR 并送入混合 LDPC 译码器，最终完成消息解包与 SNR 计算。

use num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

use crate::constants::{COSTAS_PATTERN, GRAY_MAP, NUM_SYMBOLS};
use crate::ldpc::{decode174_91, DecodeType, OsdDepth, LDPC_N};
use crate::pack::unpack77;

/// 解调解码成功结果
#[derive(Debug, Clone)]
pub struct DecodedSignal {
    /// 解码出的完整报文字符串
    pub message: String,
    /// 载波中心频率 (Hz)
    pub freq: f32,
    /// 时间偏差 (秒，以 0.5s 为基准)
    pub dt: f32,
    /// 估算的信噪比 (dB, 在 2500 Hz 带宽下)
    pub snr: i32,
    /// 79 符号音调序列 (0..7)
    pub tones: [u8; NUM_SYMBOLS],
    /// 纠正的硬判决错误数
    pub hard_errors: usize,
    /// 译码类型 (BP 或 OSD)
    pub decode_type: DecodeType,
}

pub struct SymbolExtractor {
    fft_32: Arc<dyn Fft<f32>>,
}

impl SymbolExtractor {
    pub fn new() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        let fft_32 = planner.plan_fft_forward(32);
        Self { fft_32 }
    }

    /// 从 200 Hz 基带信号中尝试解调与译码 FT8 消息
    pub fn extract_and_decode(
        &self,
        cd0: &[Complex32],
        ibest: usize,
        f1: f32,
        _sync_quality: f32,
    ) -> Option<DecodedSignal> {
        let np2 = 2812usize;

        // 1. 逐符号执行 32 点 FFT，提取 79 个符号的 8 个音调复数幅度 cs[k][tone]
        let mut cs = [[Complex32::new(0.0, 0.0); 8]; NUM_SYMBOLS];
        let mut s8 = [[0.0f32; 8]; NUM_SYMBOLS];

        for k in 0..NUM_SYMBOLS {
            let i1 = ibest + k * 32;
            let mut buf = [Complex32::new(0.0, 0.0); 32];
            if i1 + 32 <= cd0.len() && i1 + 31 < np2 {
                buf.copy_from_slice(&cd0[i1..i1 + 32]);
            }
            self.fft_32.process(&mut buf);

            for tone in 0..8 {
                let c = buf[tone] * 0.001;
                cs[k][tone] = c;
                s8[k][tone] = c.norm();
            }
        }

        // 2. 检查 21 个 Costas 同步符号吻合度
        let mut is1 = 0;
        let mut is2 = 0;
        let mut is3 = 0;
        for k in 0..7 {
            let p1 = argmax_8(&s8[k]);
            if COSTAS_PATTERN[k] == p1 {
                is1 += 1;
            }
            let p2 = argmax_8(&s8[k + 36]);
            if COSTAS_PATTERN[k] == p2 {
                is2 += 1;
            }
            let p3 = argmax_8(&s8[k + 72]);
            if COSTAS_PATTERN[k] == p3 {
                is3 += 1;
            }
        }
        let nsync = is1 + is2 + is3;
        if nsync <= 6 {
            return None; // 同步符号过差，提前剔除假峰
        }

        // 3. 多符号相干累加度量提取 (Pass 1: nsym=1, Pass 2: nsym=2, Pass 3: nsym=3)
        let mut bmeta = [0.0f32; LDPC_N];
        let mut bmetb = [0.0f32; LDPC_N];
        let mut bmetc = [0.0f32; LDPC_N];
        let mut bmetd = [0.0f32; LDPC_N];

        for nsym in 1..=3 {
            let nt = 1 << (3 * nsym); // 8, 64, 512
            for ihalf in 0..2 {
                let mut k = 0usize;
                while k < 29 {
                    let ks = if ihalf == 0 { k + 7 } else { k + 43 };
                    let mut s2 = vec![0.0f32; nt];

                    for i in 0..nt {
                        let i1 = (i / 64) & 7;
                        let i2 = (i / 8) & 7;
                        let i3 = i & 7;

                        s2[i] = match nsym {
                            1 => cs[ks][GRAY_MAP[i3] as usize].norm(),
                            2 => {
                                if ks + 1 < NUM_SYMBOLS {
                                    (cs[ks][GRAY_MAP[i2] as usize]
                                        + cs[ks + 1][GRAY_MAP[i3] as usize])
                                        .norm()
                                } else {
                                    0.0
                                }
                            }
                            3 => {
                                if ks + 2 < NUM_SYMBOLS {
                                    (cs[ks][GRAY_MAP[i1] as usize]
                                        + cs[ks + 1][GRAY_MAP[i2] as usize]
                                        + cs[ks + 2][GRAY_MAP[i3] as usize])
                                        .norm()
                                } else {
                                    0.0
                                }
                            }
                            _ => 0.0,
                        };
                    }

                    let i32 = k * 3 + ihalf * 87;
                    let ibmax = 3 * nsym - 1;

                    for ib in 0..=ibmax {
                        let bit_pos = ibmax - ib;
                        let mut max_one = f32::MIN;
                        let mut max_zero = f32::MIN;

                        for (i, &val) in s2.iter().enumerate() {
                            if ((i >> bit_pos) & 1) != 0 {
                                if val > max_one {
                                    max_one = val;
                                }
                            } else {
                                if val > max_zero {
                                    max_zero = val;
                                }
                            }
                        }

                        let bm = max_one - max_zero;
                        let out_idx = i32 + ib;
                        if out_idx < LDPC_N {
                            match nsym {
                                1 => {
                                    bmeta[out_idx] = bm;
                                    let den = max_one.max(max_zero);
                                    bmetd[out_idx] = if den > 0.0 { bm / den } else { 0.0 };
                                }
                                2 => bmetb[out_idx] = bm,
                                3 => bmetc[out_idx] = bm,
                                _ => {}
                            }
                        }
                    }

                    k += nsym;
                }
            }
        }

        // 4. 标准化与尺度缩放
        normalize_bmet(&mut bmeta);
        normalize_bmet(&mut bmetb);
        normalize_bmet(&mut bmetc);
        normalize_bmet(&mut bmetd);

        let scalefac = 2.83f32;
        let llra = scale_array(&bmeta, scalefac);
        let llrb = scale_array(&bmetb, scalefac);
        let llrc = scale_array(&bmetc, scalefac);
        let llrd = scale_array(&bmetd, scalefac);

        // 5. 软判决混合 LDPC 译码尝试 (对标 WSJT-X ft8b.f90)
        // 第一阶段：优先在全部 4 种相干组合中运行 BP 译码 (无假阳性风险)
        let passes = [llra, llrb, llrc, llrd];
        let mut decode_result = None;

        for llr_pass in &passes {
            if let Some(dec_res) = decode174_91(llr_pass, -1, OsdDepth::Order0, None) {
                if dec_res.hard_errors <= 36 && !dec_res.codeword.iter().all(|&b| b == 0) {
                    decode_result = Some(dec_res);
                    break;
                }
            }
        }

        // 第二阶段：若 BP 全部失败，使用 BP 保存的累计 LLR 回退尝试 OSD Order-2 译码 (对标 WSJT-X maxosd=2, norder=2)
        if decode_result.is_none() {
            for llr_pass in &[llra, llrb] {
                if let Some(dec_res) = decode174_91(llr_pass, 2, OsdDepth::Order2, None) {
                    if dec_res.hard_errors <= 36 && !dec_res.codeword.iter().all(|&b| b == 0) {
                        decode_result = Some(dec_res);
                        break;
                    }
                }
            }
        }

        if let Some(dec_res) = decode_result {
            // 解码成功！打包回 10 字节并解包 FT8 消息
            let mut payload10 = [0u8; 10];
            for i in 0..77 {
                if dec_res.message77[i] != 0 {
                    payload10[i / 8] |= 0x80 >> (i % 8);
                }
            }

            let i3 = crate::pack::get_i3(&payload10);
            let n3 = crate::pack::get_n3(&payload10);
            if i3 > 5 || (i3 == 0 && n3 > 6) || (i3 == 0 && n3 == 2) {
                return None;
            }

            if let Ok(ft8_msg) = unpack77(&payload10) {
                // 重新构造 79 音调
                let tones = crate::modulate::ft8_payload_to_tones(&payload10);

                    // 计算物理 SNR (优先采用全局物理背景谱底噪 xbase，对标 WSJT-X 算法规范与 2500 Hz 参考带宽)
                    let mut xsig = 0.0f32;
                    let mut noise_powers = Vec::with_capacity(NUM_SYMBOLS * 7);
                    for i in 0..NUM_SYMBOLS {
                        let t = tones[i] as usize;
                        xsig += s8[i][t].powi(2);
                        for tone_idx in 0..8 {
                            if tone_idx != t {
                                noise_powers.push(s8[i][tone_idx].powi(2));
                            }
                        }
                    }

                    noise_powers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let p40_idx = (noise_powers.len() as f32 * 0.40) as usize;
                    let noise_floor_per_bin = noise_powers[p40_idx] / 0.5108256;
                    let xnoi = (NUM_SYMBOLS as f32) * noise_floor_per_bin;

                    let arg = if xnoi > 1e-12 { (xsig / xnoi) - 1.0 } else { 0.001 };
                    let mut snr_f = if arg > 0.002 {
                        10.0 * arg.log10() - 27.0
                    } else {
                        -24.0
                    };
                    if snr_f < -24.0 {
                        snr_f = -24.0;
                    }

                    let dt = ((ibest as f32) / 200.0) - 0.5;

                    return Some(DecodedSignal {
                        message: ft8_msg.text,
                        freq: f1,
                        dt,
                        snr: snr_f.round() as i32,
                        tones,
                        hard_errors: dec_res.hard_errors,
                        decode_type: dec_res.decode_type,
                    });
                }
            }

            None
        }

}

fn argmax_8(arr: &[f32; 8]) -> u8 {
    let mut best_idx = 0u8;
    let mut best_val = arr[0];
    for i in 1..8 {
        if arr[i] > best_val {
            best_val = arr[i];
            best_idx = i as u8;
        }
    }
    best_idx
}

fn normalize_bmet(bmet: &mut [f32; LDPC_N]) {
    let sum: f32 = bmet.iter().sum();
    let mean = sum / (LDPC_N as f32);
    let sum_sq: f32 = bmet.iter().map(|&x| (x - mean) * (x - mean)).sum();
    let var = sum_sq / (LDPC_N as f32);
    let std = if var > 1e-9 { var.sqrt() } else { 1.0 };
    for x in bmet.iter_mut() {
        *x = (*x - mean) / std;
    }
}

fn scale_array(arr: &[f32; LDPC_N], scale: f32) -> [f32; LDPC_N] {
    let mut out = [0.0f32; LDPC_N];
    for i in 0..LDPC_N {
        out[i] = arr[i] * scale;
    }
    out
}

impl Default for SymbolExtractor {
    fn default() -> Self {
        Self::new()
    }
}
