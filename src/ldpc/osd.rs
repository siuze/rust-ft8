//! FT8 信道 LDPC(174, 91) 顺序统计量译码器 (Ordered Statistics Decoder, OSD)
//!
//! 当置信传播 (BP) 算法由于强多径衰落或深信噪比 (-21 dB 以下) 未能收敛时，
//! OSD 算法通过按信道可靠度排序，并在最可靠独立比特基上生成候选码字，
//! 结合 14-bit CRC 校验判定极高置信度的正确解码。
//! 算法设计对齐 WSJT-X `osd174_91.f90`。

use super::constants::*;
use super::encoder::encode174_bits;

/// OSD 译码深度配置
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsdDepth {
    /// 仅尝试 0 阶（最可靠基硬判决）
    Order0 = 0,
    /// 尝试 0 阶与 1 阶（翻转 1 个比特，共 91 个测试向量）
    Order1 = 1,
    /// 尝试 0 阶、1 阶与 2 阶（翻转 2 个比特，共 ~4100 个测试向量）
    Order2 = 2,
}

/// OSD 译码结果
#[derive(Debug, Clone)]
pub struct OsdResult {
    /// 77 位信息载荷比特
    pub message77: [u8; 77],
    /// 91 位信息比特 (77 载荷 + 14 CRC)
    pub message91: [u8; LDPC_K],
    /// 174 位完整码字
    pub codeword: [u8; LDPC_N],
    /// 纠正的硬判决错误数
    pub hard_errors: usize,
    /// 软距离度量 sum(|LLR| * err)
    pub soft_distance: f32,
    /// 成功的 OSD 阶数 (0, 1, 2)
    pub order: usize,
}

/// 获取 LDPC(174, 91) 的 91 x 174 系统生成矩阵
pub fn get_generator_matrix() -> &'static [[u8; LDPC_N]; LDPC_K] {
    use std::sync::OnceLock;
    static G_CACHE: OnceLock<[[u8; LDPC_N]; LDPC_K]> = OnceLock::new();

    G_CACHE.get_or_init(|| {
        let mut g = [[0u8; LDPC_N]; LDPC_K];
        for i in 0..LDPC_K {
            let mut basis = [0u8; LDPC_K];
            basis[i] = 1;
            g[i] = encode174_bits(&basis);
        }
        g
    })
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct BitWord174 {
    pub w: [u64; 3],
}

impl BitWord174 {
    #[inline(always)]
    pub const fn new() -> Self {
        Self { w: [0, 0, 0] }
    }

    #[inline(always)]
    pub fn get_bit(&self, idx: usize) -> u8 {
        let word = idx / 64;
        let shift = idx % 64;
        ((self.w[word] >> shift) & 1) as u8
    }

    #[inline(always)]
    pub fn set_bit(&mut self, idx: usize) {
        let word = idx / 64;
        let shift = idx % 64;
        self.w[word] |= 1u64 << shift;
    }

    #[inline(always)]
    pub fn toggle_bit(&mut self, idx: usize) {
        let word = idx / 64;
        let shift = idx % 64;
        self.w[word] ^= 1u64 << shift;
    }

    #[inline(always)]
    pub fn xor(&self, other: &Self) -> Self {
        Self {
            w: [
                self.w[0] ^ other.w[0],
                self.w[1] ^ other.w[1],
                self.w[2] ^ other.w[2],
            ],
        }
    }
}

/// 执行 LDPC(174, 91) 顺序统计量译码 (OSD)
///
/// # 参数
/// - `llr`: 174 位接收符号的后验或通道 LLR (LLR > 0 倾向于 1)
/// - `depth`: OSD 阶数 (Order0, Order1, Order2)
///
/// # 返回
/// - `Some(OsdResult)`: 成功译码（找到通过 CRC14 校验的候选码字）
/// - `None`: 未找到符合校验的码字
pub fn osd_decode(llr: &[f32; LDPC_N], depth: OsdDepth) -> Option<OsdResult> {
    let gen = get_generator_matrix();

    // 1. 硬判决
    let mut hdec = [0u8; LDPC_N];
    for i in 0..LDPC_N {
        if llr[i] >= 0.0 {
            hdec[i] = 1;
        }
    }

    // 2. 按可靠度 (|LLR|) 降序排列索引
    let mut indices: [usize; LDPC_N] = std::array::from_fn(|i| i);
    indices.sort_by(|&a, &b| {
        llr[b]
            .abs()
            .partial_cmp(&llr[a].abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // 3. 构建重排生成矩阵 gen_mrb (K x N = 91 x 174)
    let mut gen_mrb = [BitWord174::new(); LDPC_K];
    for r in 0..LDPC_K {
        for c in 0..LDPC_N {
            if gen[r][indices[c]] != 0 {
                gen_mrb[r].set_bit(c);
            }
        }
    }

    // 4. 二元高斯消元法将前 91 列化为单位矩阵 I_91
    for id in 0..LDPC_K {
        let mut pivot_col = None;
        for col in id..LDPC_N {
            if gen_mrb[id].get_bit(col) != 0 {
                pivot_col = Some(col);
                break;
            }
        }

        if let Some(col) = pivot_col {
            if col != id {
                for r in 0..LDPC_K {
                    let b1 = gen_mrb[r].get_bit(id);
                    let b2 = gen_mrb[r].get_bit(col);
                    if b1 != b2 {
                        gen_mrb[r].toggle_bit(id);
                        gen_mrb[r].toggle_bit(col);
                    }
                }
                indices.swap(id, col);
            }

            // 对其他所有行消元
            let pivot_row = gen_mrb[id];
            for r in 0..LDPC_K {
                if r != id && gen_mrb[r].get_bit(id) != 0 {
                    gen_mrb[r] = gen_mrb[r].xor(&pivot_row);
                }
            }
        }
    }

    // 5. 0 阶最可靠比特基向量 m0 与 0 阶码字 c0
    let mut c0 = BitWord174::new();
    for r in 0..LDPC_K {
        if llr[indices[r]] >= 0.0 {
            c0 = c0.xor(&gen_mrb[r]);
        }
    }

    // 预计算前 91 个原信息位在重排空间中的位置映射 (用于 CRC 快速早筛)
    // 转换为连续平铺字节数组，发挥 CPU 顺序读写与自动向量化能力
    let mut gen_bytes = [[0u8; LDPC_N]; LDPC_K];
    for r in 0..LDPC_K {
        for c in 0..LDPC_N {
            gen_bytes[r][c] = gen_mrb[r].get_bit(c);
        }
    }
    let mut c0_bytes = [0u8; LDPC_N];
    for c in 0..LDPC_N {
        c0_bytes[c] = c0.get_bit(c);
    }
    // CRC needs only the 91 systematic information bits. Map them into the
    // reliability-ordered codeword once, rather than unshuffling 174 bits for
    // every one of the Order 2 candidates.
    let mut info_positions = [0usize; LDPC_K];
    for (mrb_idx, &orig_idx) in indices.iter().enumerate() {
        if orig_idx < LDPC_K {
            info_positions[orig_idx] = mrb_idx;
        }
    }

    let mut best_result: Option<OsdResult> = None;
    let mut min_soft_dist = f32::MAX;

    // --- Order 0 评估 ---
    check_candidate(&c0_bytes, &indices, &info_positions, &hdec, llr, 0, &mut best_result, &mut min_soft_dist);
    if depth == OsdDepth::Order0 && best_result.is_some() {
        return best_result;
    }

    // --- Order 1 评估 (单比特翻转) ---
    if depth as usize >= 1 {
        for i1 in 0..LDPC_K {
            let mut c1 = c0_bytes;
            let g1 = &gen_bytes[i1];
            for c in 0..LDPC_N {
                c1[c] ^= g1[c];
            }
            check_candidate(&c1, &indices, &info_positions, &hdec, llr, 1, &mut best_result, &mut min_soft_dist);
        }
    }

    // --- Order 2 评估 (双比特翻转) ---
    if depth as usize >= 2 {
        for i1 in 0..LDPC_K {
            let mut c0_i1 = c0_bytes;
            let g1 = &gen_bytes[i1];
            for c in 0..LDPC_N {
                c0_i1[c] ^= g1[c];
            }
            for i2 in (i1 + 1)..LDPC_K {
                let mut c2 = c0_i1;
                let g2 = &gen_bytes[i2];
                for c in 0..LDPC_N {
                    c2[c] ^= g2[c];
                }
                check_candidate(&c2, &indices, &info_positions, &hdec, llr, 2, &mut best_result, &mut min_soft_dist);
            }
        }
    }

    best_result
}

#[inline(always)]
fn check_candidate(
    cw_mrb: &[u8; LDPC_N],
    indices: &[usize; LDPC_N],
    info_positions: &[usize; LDPC_K],
    hdec: &[u8; LDPC_N],
    llr: &[f32; LDPC_N],
    order: usize,
    best_result: &mut Option<OsdResult>,
    min_soft_dist: &mut f32,
) {
    let mut message91 = [0u8; LDPC_K];
    for (bit, &mrb_idx) in info_positions.iter().enumerate() {
        message91[bit] = cw_mrb[mrb_idx];
    }

    // 提取接收到的 14-bit CRC
    let mut rx_crc = 0u16;
    for i in 0..14 {
        rx_crc = (rx_crc << 1) | (message91[77 + i] as u16);
    }

    // 采用 256 表项查表法极速计算 77 比特载荷 CRC
    let calc_crc = crate::crc::compute_crc14_from_bits(&message91[..77]);
    if calc_crc == rx_crc {
        let mut orig_cw = [0u8; LDPC_N];
        for (mrb_idx, &orig_idx) in indices.iter().enumerate() {
            orig_cw[orig_idx] = cw_mrb[mrb_idx];
        }
        // CRC 校验成功！计算软距离
        let mut soft_dist = 0.0f32;
        let mut hard_errors = 0;
        for i in 0..LDPC_N {
            if orig_cw[i] != hdec[i] {
                soft_dist += llr[i].abs();
                hard_errors += 1;
            }
        }

        if soft_dist < *min_soft_dist {
            *min_soft_dist = soft_dist;
            let mut message77 = [0u8; 77];
            message77.copy_from_slice(&message91[..77]);

            *best_result = Some(OsdResult {
                message77,
                message91,
                codeword: orig_cw,
                hard_errors,
                soft_distance: soft_dist,
                order,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::add_crc14_bytes;
    use crate::ldpc::encoder::{encode174_packed, unpack_codeword};

    #[test]
    fn test_osd_decode_order0() {
        let payload = [0x42, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x00];
        let mut msg12 = [0u8; 12];
        add_crc14_bytes(&payload, &mut msg12);

        let packed = encode174_packed(&msg12);
        let cw_bits = unpack_codeword(&packed);

        let mut llr = [0.0f32; LDPC_N];
        for i in 0..LDPC_N {
            llr[i] = if cw_bits[i] != 0 { 4.0 } else { -4.0 };
        }

        let res = osd_decode(&llr, OsdDepth::Order0).expect("Order 0 必须解码成功");
        assert_eq!(res.codeword, cw_bits);
        assert_eq!(res.order, 0);
    }

    #[test]
    fn test_osd_decode_order1() {
        let payload = [0x99, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00];
        let mut msg12 = [0u8; 12];
        add_crc14_bytes(&payload, &mut msg12);

        let packed = encode174_packed(&msg12);
        let cw_bits = unpack_codeword(&packed);

        let mut llr = [0.0f32; LDPC_N];
        for i in 0..LDPC_N {
            llr[i] = if cw_bits[i] != 0 { 2.5 } else { -2.5 };
        }

        // 翻转 1 个高可靠度比特（使得 Order 0 失败，但 Order 1 成功纠正）
        llr[0] = -llr[0];

        let res = osd_decode(&llr, OsdDepth::Order1).expect("Order 1 应能纠正单比特高可靠度错误");
        assert_eq!(res.codeword, cw_bits);
        assert!(res.order <= 1);
    }
}
