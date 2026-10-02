//! FT8 79 符号音调序列调制器
//!
//! 消息结构:
//! S7 (Costas 同步) + D29 (数据音调) + S7 (Costas 同步) + D29 (数据音调) + S7 (Costas 同步)
//! 总共 79 个符号，每个符号对应 0..7 音调之一。

use crate::constants::{COSTAS_PATTERN, GRAY_MAP, NUM_SYMBOLS};
use crate::crc::add_crc14_bytes;
use crate::ldpc::encoder::encode174_packed;
use crate::pack::pack77;

/// 根据 77 位有效载荷（10 字节，低 3 位为 0）生成 79 个 FT8 符号的音调序列 (0..7)
pub fn ft8_payload_to_tones(payload10: &[u8; 10]) -> [u8; NUM_SYMBOLS] {
    let mut a91 = [0u8; 12];
    add_crc14_bytes(payload10, &mut a91);

    let codeword = encode174_packed(&a91);

    let mut tones = [0u8; NUM_SYMBOLS];
    let mut mask = 0x80u8;
    let mut i_byte = 0usize;

    for i_tone in 0..NUM_SYMBOLS {
        if i_tone < 7 {
            tones[i_tone] = COSTAS_PATTERN[i_tone];
        } else if (36..43).contains(&i_tone) {
            tones[i_tone] = COSTAS_PATTERN[i_tone - 36];
        } else if (72..79).contains(&i_tone) {
            tones[i_tone] = COSTAS_PATTERN[i_tone - 72];
        } else {
            // 提取 3 比特并进行 Gray 映射
            let mut bits3 = 0u8;

            if (codeword[i_byte] & mask) != 0 {
                bits3 |= 4;
            }
            mask >>= 1;
            if mask == 0 {
                mask = 0x80;
                i_byte += 1;
            }

            if (codeword[i_byte] & mask) != 0 {
                bits3 |= 2;
            }
            mask >>= 1;
            if mask == 0 {
                mask = 0x80;
                i_byte += 1;
            }

            if (codeword[i_byte] & mask) != 0 {
                bits3 |= 1;
            }
            mask >>= 1;
            if mask == 0 {
                mask = 0x80;
                i_byte += 1;
            }

            tones[i_tone] = GRAY_MAP[bits3 as usize];
        }
    }

    tones
}

/// 根据 77 位比特数组直接生成 79 音调
pub fn ft8_bits_to_tones(bits77: &[u8; 77]) -> [u8; NUM_SYMBOLS] {
    let mut payload10 = [0u8; 10];
    for (i, &b) in bits77.iter().enumerate() {
        if b != 0 {
            payload10[i / 8] |= 0x80 >> (i % 8);
        }
    }
    ft8_payload_to_tones(&payload10)
}

/// 根据文本消息（如 "CQ BD4SUR OM99"）直接编码生成 79 音调
pub fn encode_message_to_tones(message: &str) -> Result<[u8; NUM_SYMBOLS], String> {
    let payload = pack77(message)?;
    Ok(ft8_payload_to_tones(&payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tones_structure() {
        let text = "CQ BD4SUR OM99";
        let tones = encode_message_to_tones(text).expect("打包应成功");

        // 验证 3 组 Costas 序列
        assert_eq!(&tones[0..7], &COSTAS_PATTERN);
        assert_eq!(&tones[36..43], &COSTAS_PATTERN);
        assert_eq!(&tones[72..79], &COSTAS_PATTERN);

        // 验证所有音调都在 0..=7 范围内
        for &t in &tones {
            assert!(t <= 7);
        }
    }
}
