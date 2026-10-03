//! FT8 LDPC(174, 91) 编解码器模块
//!
//! 包括：
//! - 常量矩阵与表结构 (`constants`)
//! - 奇偶校验生成器与编码器 (`encoder`)
//! - 置信传播译码器 (`bp`)
//! - 顺序统计量译码器 (`osd`)
//! - 混合 BP / OSD 译码总入口 (`decode174_91`)

pub mod bp;
pub mod constants;
pub mod encoder;
pub mod osd;

pub use bp::{bp_decode, BpResult};
pub use constants::*;
pub use encoder::{
    encode174_bits, encode174_packed, ldpc_check, pack_codeword, parity8, unpack_codeword,
};
pub use osd::{osd_decode, OsdDepth, OsdResult};

/// 译码成功类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeType {
    /// 通过置信传播 (BP) 算法收敛
    Bp,
    /// 通过顺序统计量 (OSD) 算法恢复
    Osd,
}

/// 统一译码结果
#[derive(Debug, Clone)]
pub struct DecodeResult {
    /// 77 位信息有效载荷
    pub message77: [u8; 77],
    /// 91 位完整信息比特 (含 14-bit CRC)
    pub message91: [u8; LDPC_K],
    /// 174 位纠错后完整码字
    pub codeword: [u8; LDPC_N],
    /// 纠正的硬判决错误数
    pub hard_errors: usize,
    /// 译码成功类型 (BP 或 OSD)
    pub decode_type: DecodeType,
}

/// 混合 BP + OSD 联合译码器（对标 WSJT-X decode174_91.f90）
///
/// # 参数
/// - `llr`: 174 位接收符号 LLR（LLR > 0 倾向于 1）
/// - `max_osd`:
///   - `< 0`: 仅执行 BP 译码
///   - `= 0`: 执行 BP，若失败则用通道 LLR 执行 1 次 OSD
///   - `> 0`: 执行 BP，若失败则用 BP 保存的累计后验 LLR 执行最多 `max_osd` 次 OSD
/// - `depth`: OSD 搜索阶数 (Order0, Order1, Order2)
pub fn decode174_91(
    llr: &[f32; LDPC_N],
    max_osd: isize,
    depth: OsdDepth,
    ap_mask: Option<&[u8; LDPC_N]>,
) -> Option<DecodeResult> {
    // 1. 尝试置信传播 (BP) 译码
    match bp_decode(llr, 30, ap_mask) {
        Ok(bp_res) => Some(DecodeResult {
            message77: bp_res.message77,
            message91: bp_res.message91,
            codeword: bp_res.codeword,
            hard_errors: bp_res.hard_errors,
            decode_type: DecodeType::Bp,
        }),
        Err(z_save) => {
            if max_osd < 0 {
                return None;
            }

            // 2. BP 未收敛，回退至 OSD 译码
            if max_osd == 0 {
                // 使用原始通道 LLR 进行 OSD
                if let Some(osd_res) = osd_decode(llr, depth) {
                    return Some(DecodeResult {
                        message77: osd_res.message77,
                        message91: osd_res.message91,
                        codeword: osd_res.codeword,
                        hard_errors: osd_res.hard_errors,
                        decode_type: DecodeType::Osd,
                    });
                }
            } else {
                // 使用 BP 迭代过程保存的累计 LLR 进行 OSD
                let trials = (max_osd as usize).min(z_save.len());
                for i in 0..trials {
                    if let Some(osd_res) = osd_decode(&z_save[i], depth) {
                        return Some(DecodeResult {
                            message77: osd_res.message77,
                            message91: osd_res.message91,
                            codeword: osd_res.codeword,
                            hard_errors: osd_res.hard_errors,
                            decode_type: DecodeType::Osd,
                        });
                    }
                }
                // 若累计 LLR 为空，回退尝试原始 LLR
                if trials == 0 {
                    if let Some(osd_res) = osd_decode(llr, depth) {
                        return Some(DecodeResult {
                            message77: osd_res.message77,
                            message91: osd_res.message91,
                            codeword: osd_res.codeword,
                            hard_errors: osd_res.hard_errors,
                            decode_type: DecodeType::Osd,
                        });
                    }
                }
            }

            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crc::add_crc14_bytes;

    #[test]
    fn test_hybrid_decoder_bp_and_osd() {
        let payload = [0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0x00];
        let mut msg12 = [0u8; 12];
        add_crc14_bytes(&payload, &mut msg12);

        let packed = encode174_packed(&msg12);
        let cw_bits = unpack_codeword(&packed);

        // 1. 无噪测试：应由 BP 直接成功
        let mut llr = [0.0f32; LDPC_N];
        for i in 0..LDPC_N {
            llr[i] = if cw_bits[i] != 0 { 5.0 } else { -5.0 };
        }
        let res_bp = decode174_91(&llr, 2, OsdDepth::Order1, None).expect("BP 路径应直接成功");
        assert_eq!(res_bp.decode_type, DecodeType::Bp);
        assert_eq!(res_bp.codeword, cw_bits);

        // 2. 模拟破坏：翻转 1 个高可靠度比特，并使 BP 迭代困难
        llr[0] = -llr[0];
        let res_osd = decode174_91(&llr, 2, OsdDepth::Order1, None).expect("OSD 路径应能成功回退拯救");
        assert_eq!(res_osd.codeword, cw_bits);
    }
}
