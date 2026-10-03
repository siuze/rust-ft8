//! FT8 信道 LDPC(174, 91) 置信传播 (Belief Propagation, BP) 译码器
//!
//! 基于对数似然比 (Log-Likelihood Ratio, LLR) 的和积算法 (Sum-Product Algorithm)。
//! 算法结构与 WSJT-X `bpdecode174_91.f90` 及 `decode174_91.f90` 完全一致。

use super::constants::*;
use super::encoder::ldpc_check;
use crate::crc::check_crc14_bits;

/// BP 译码结果
#[derive(Debug, Clone)]
pub struct BpResult {
    /// 77 位信息有效载荷 (当 CRC 校验通过时有效)
    pub message77: [u8; 77],
    /// 91 位信息比特 (77 载荷 + 14 CRC)
    pub message91: [u8; LDPC_K],
    /// 174 位纠错后完整码字
    pub codeword: [u8; LDPC_N],
    /// 纠正的硬错误比特数 (与接收 LLR 初始硬判决相比较)
    pub hard_errors: usize,
    /// 实际收敛消耗的迭代轮数
    pub iterations: usize,
}

/// 快速分段线性 tanh 近似（完全对齐 WSJT-X pltanh.f90）
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let isign = if x < 0.0 { -1.0 } else { 1.0 };
    let z = x.abs();
    if z <= 0.8 {
        0.83 * x
    } else if z <= 1.6 {
        isign * (0.322 * z + 0.4064)
    } else if z <= 3.0 {
        isign * (0.0524 * z + 0.8378)
    } else if z < 7.0 {
        isign * (0.0012 * z + 0.9914)
    } else {
        isign * 0.9998
    }
}

/// 快速分段线性 atanh 近似（完全对齐 WSJT-X platanh.f90）
#[inline]
pub fn fast_atanh(x: f32) -> f32 {
    let isign = if x < 0.0 { -1.0 } else { 1.0 };
    let z = x.abs();
    if z <= 0.664 {
        x / 0.83
    } else if z <= 0.9217 {
        isign * (z - 0.4064) / 0.322
    } else if z <= 0.9951 {
        isign * (z - 0.8378) / 0.0524
    } else if z <= 0.9998 {
        isign * (z - 0.9914) / 0.0012
    } else {
        isign * 7.0
    }
}

// 编译期预计算的拓扑映射表：消除变量与校验节点消息传递时的线性查找
const LDPC_NM_TO_MN_SLOT: [[usize; 7]; LDPC_M] = {
    let mut table = [[0usize; 7]; LDPC_M];
    let mut j = 0;
    while j < LDPC_M {
        let nr = LDPC_NUM_ROWS[j] as usize;
        let mut i = 0;
        while i < nr {
            let ibj = (LDPC_NM[j][i] - 1) as usize;
            let mut kk = 0;
            while kk < 3 {
                if (LDPC_MN[ibj][kk] - 1) as usize == j {
                    table[j][i] = kk;
                    break;
                }
                kk += 1;
            }
            i += 1;
        }
        j += 1;
    }
    table
};

const LDPC_MN_TO_NM_SLOT: [[usize; 3]; LDPC_N] = {
    let mut table = [[0usize; 3]; LDPC_N];
    let mut j = 0;
    while j < LDPC_N {
        let mut i = 0;
        while i < 3 {
            let ichk = (LDPC_MN[j][i] - 1) as usize;
            let nr = LDPC_NUM_ROWS[ichk] as usize;
            let mut k = 0;
            while k < nr {
                if (LDPC_NM[ichk][k] - 1) as usize == j {
                    table[j][i] = k;
                    break;
                }
                k += 1;
            }
            i += 1;
        }
        j += 1;
    }
    table
};

/// 执行 LDPC(174, 91) 置信传播 (BP) 译码
///
/// # 参数
/// - `llr`: 174 位接收符号的对数似然比（LLR > 0 倾向于 1，LLR < 0 倾向于 0）
/// - `max_iterations`: 最大迭代轮数（通常取 30 ~ 50）
/// - `ap_mask`: 先验信息掩码（若某些位已知，置为 1，不更新其消息；通常传入 None）
///
/// # 返回
/// - `Ok(BpResult)`: 译码成功（满足全部校验方程且 CRC14 校验正确）
/// - `Err(z_save)`: 译码未收敛，返回保存的前几轮 LLR 累加和供 OSD 回退
pub fn bp_decode(
    llr: &[f32; LDPC_N],
    max_iterations: usize,
    ap_mask: Option<&[u8; LDPC_N]>,
) -> Result<BpResult, Vec<[f32; LDPC_N]>> {
    let mut tov = [[0.0f32; 3]; LDPC_N]; // 校验节点传递到变量节点的消息
    let mut toc = [[0.0f32; 7]; LDPC_M]; // 变量节点传递到校验节点的消息
    let mut tanhtoc = [[0.0f32; 7]; LDPC_M];
    let mut zn = [0.0f32; LDPC_N];
    let mut zsum = [0.0f32; LDPC_N];
    let mut zsave = Vec::with_capacity(3);

    // 初始化：toc 设为通道 LLR
    for j in 0..LDPC_M {
        let nr = LDPC_NUM_ROWS[j] as usize;
        for i in 0..nr {
            let bit_idx = (LDPC_NM[j][i] - 1) as usize;
            toc[j][i] = llr[bit_idx];
        }
    }

    let mut ncnt = 0;
    let mut nclast = LDPC_M;

    for iter in 0..=max_iterations {
        // 1. 更新后验比特对数似然比 zn (第 0 轮迭代 tov 全 0)
        for i in 0..LDPC_N {
            let mask = ap_mask.map_or(0, |m| m[i]);
            if mask != 1 {
                zn[i] = llr[i] + tov[i][0] + tov[i][1] + tov[i][2];
            } else {
                zn[i] = llr[i];
            }
            zsum[i] += zn[i];
        }

        // 保存前几轮累计 LLR 用于后续可能的 OSD 回退
        if iter > 0 && iter <= 3 {
            zsave.push(zsum);
        }

        // 2. 判决当前码字候选 cw
        let mut cw = [0u8; LDPC_N];
        let mut sum_cw = 0;
        for i in 0..LDPC_N {
            cw[i] = if zn[i] > 0.0 { 1 } else { 0 };
            sum_cw += cw[i] as usize;
        }

        // 全零码字非合法 FT8 传输码字
        if sum_cw > 0 {
            // 3. 校验方程检验
            let ncheck = ldpc_check(&cw);

            if ncheck == 0 {
                // 满足全部 83 个 LDPC 校验方程，检查 CRC14
                if check_crc14_bits(&cw[..LDPC_K]) {
                    // CRC 校验成功！
                    let mut hard_errors = 0;
                    for i in 0..LDPC_N {
                        let orig_hard = if llr[i] >= 0.0 { 1 } else { 0 };
                        if cw[i] != orig_hard {
                            hard_errors += 1;
                        }
                    }

                    let mut message91 = [0u8; LDPC_K];
                    message91.copy_from_slice(&cw[..LDPC_K]);

                    let mut message77 = [0u8; 77];
                    message77.copy_from_slice(&cw[..77]);

                    return Ok(BpResult {
                        message77,
                        message91,
                        codeword: cw,
                        hard_errors,
                        iterations: iter,
                    });
                }
            }

            // 4. 早停准则：若未满足校验数持续不降且已迭代若干轮，提前退出
            if iter > 0 {
                let nd = (ncheck as isize) - (nclast as isize);
                if nd < 0 {
                    ncnt = 0;
                } else {
                    ncnt += 1;
                }

                if ncnt >= 5 && iter >= 10 && ncheck > 15 {
                    return Err(zsave);
                }
            }
            nclast = ncheck;
        }

        // 5. 变量节点向校验节点传递消息 (toc)，通过静态映射表直接索引
        for j in 0..LDPC_M {
            let nr = LDPC_NUM_ROWS[j] as usize;
            for i in 0..nr {
                let ibj = (LDPC_NM[j][i] - 1) as usize;
                let slot = LDPC_NM_TO_MN_SLOT[j][i];
                toc[j][i] = zn[ibj] - tov[ibj][slot];
            }
        }

        // 6. 校验节点向变量节点传递消息 (tov)，通过前缀/后缀积实现 O(1) 连乘
        let mut prod_except_k = [[1.0f32; 7]; LDPC_M];
        for j in 0..LDPC_M {
            let nr = LDPC_NUM_ROWS[j] as usize;
            for i in 0..nr {
                tanhtoc[j][i] = fast_tanh(-toc[j][i] / 2.0);
            }

            let mut prefix = [1.0f32; 8];
            let mut suffix = [1.0f32; 8];
            for k in 0..nr {
                prefix[k + 1] = prefix[k] * tanhtoc[j][k];
            }
            let mut k = nr;
            while k > 0 {
                suffix[k - 1] = suffix[k] * tanhtoc[j][k - 1];
                k -= 1;
            }
            for k in 0..nr {
                prod_except_k[j][k] = prefix[k] * suffix[k + 1];
            }
        }

        for j in 0..LDPC_N {
            for i in 0..3 {
                let ichk = (LDPC_MN[j][i] - 1) as usize;
                let k_slot = LDPC_MN_TO_NM_SLOT[j][i];
                let tmn = prod_except_k[ichk][k_slot];
                tov[j][i] = 2.0 * fast_atanh(-tmn);
            }
        }
    }

    Err(zsave)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::add_crc14_bytes;
    use crate::ldpc::encoder::{encode174_packed, unpack_codeword};

    #[test]
    fn test_bp_decode_noiseless() {
        // 构造合法的 77 位载荷 + CRC14
        let payload = [0x5D, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0, 0x00];
        let mut msg12 = [0u8; 12];
        add_crc14_bytes(&payload, &mut msg12);

        // LDPC 编码为 174 位码字
        let packed = encode174_packed(&msg12);
        let cw_bits = unpack_codeword(&packed);

        // 转换为无噪 LLR：1 对应 +5.0，0 对应 -5.0
        let mut llr = [0.0f32; LDPC_N];
        for i in 0..LDPC_N {
            llr[i] = if cw_bits[i] != 0 { 5.0 } else { -5.0 };
        }

        let res = bp_decode(&llr, 30, None).expect("无噪 LLR 必须秒级收敛成功");
        assert_eq!(res.codeword, cw_bits);
        assert_eq!(res.hard_errors, 0);
        assert_eq!(res.iterations, 0, "无噪情况下在第 0 轮即直接校验成功");
    }

    #[test]
    fn test_bp_decode_with_errors() {
        // 构造合法的 77 位载荷 + CRC14
        let payload = [0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0x00];
        let mut msg12 = [0u8; 12];
        add_crc14_bytes(&payload, &mut msg12);

        let packed = encode174_packed(&msg12);
        let cw_bits = unpack_codeword(&packed);

        let mut llr = [0.0f32; LDPC_N];
        for i in 0..LDPC_N {
            llr[i] = if cw_bits[i] != 0 { 3.0 } else { -3.0 };
        }

        // 人为翻转 8 个随机比特的符号（模拟强干扰）
        let flipped_indices = [5, 12, 27, 43, 68, 95, 118, 150];
        for &idx in &flipped_indices {
            llr[idx] = -llr[idx];
        }

        let res = bp_decode(&llr, 30, None).expect("BP 算法应能纠正 8 个硬判决错误");
        assert_eq!(res.codeword, cw_bits);
        assert_eq!(res.hard_errors, 8);
    }
}
