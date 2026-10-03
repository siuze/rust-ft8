//! 14-bit CRC 算法实现 (完全对齐 WSJT-X / FT8 规范)
//!
//! 多项式: 0x2757 (x^14 + x^13 + x^10 + x^9 + x^8 + x^6 + x^4 + x^2 + x + 1)
//! 规则: 对 77 位载荷 zero-extended 补 5 个 0 扩展至 82 位后进行模 2 除法计算。

pub const CRC_POLYNOMIAL: u16 = 0x2757;
pub const CRC_WIDTH: usize = 14;
pub const CRC_MASK: u16 = (1 << CRC_WIDTH) - 1; // 0x3FFF

/// 编译期生成的 14-bit CRC 查找表 (256 项)
pub const CRC14_TABLE: [u16; 256] = {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut rem = (i as u16) << 6; // 14 - 8 = 6
        let mut bit = 0;
        while bit < 8 {
            if (rem & (1 << 13)) != 0 {
                rem = (rem << 1) ^ CRC_POLYNOMIAL;
            } else {
                rem <<= 1;
            }
            bit += 1;
        }
        table[i] = rem & CRC_MASK;
        i += 1;
    }
    table
};

/// 快速查表计算 14-bit CRC
pub fn compute_crc14(message: &[u8], num_bits: usize) -> u16 {
    let full_bytes = num_bits / 8;
    let rem_bits = num_bits % 8;

    let mut remainder: u16 = 0;
    let mut i = 0;
    while i < full_bytes && i < message.len() {
        let top = ((remainder >> 6) as u8) ^ message[i];
        remainder = ((remainder & 0x003F) << 8) ^ CRC14_TABLE[top as usize];
        i += 1;
    }

    if rem_bits > 0 && i < message.len() {
        let last_byte = message[i];
        for b in 0..rem_bits {
            let bit = (last_byte >> (7 - b)) & 1;
            remainder ^= (bit as u16) << (CRC_WIDTH - 1);
            if (remainder & (1 << (CRC_WIDTH - 1))) != 0 {
                remainder = (remainder << 1) ^ CRC_POLYNOMIAL;
            } else {
                remainder <<= 1;
            }
        }
    }

    remainder & CRC_MASK
}

/// 专门对 77-bit 载荷（末尾补 5 个 0 扩展至 82-bit）进行极速查表计算
#[inline]
pub fn compute_crc14_payload77(payload10: &[u8; 10]) -> u16 {
    let mut remainder: u16 = 0;
    let mut i = 0;
    // 前 9 字节全部有效
    while i < 9 {
        let top = ((remainder >> 6) as u8) ^ payload10[i];
        remainder = ((remainder & 0x003F) << 8) ^ CRC14_TABLE[top as usize];
        i += 1;
    }
    // 第 10 字节的高 5 位有效 (bits 72..76)，低 3 位必须为 0
    let b9 = payload10[9] & 0xF8;
    let top = ((remainder >> 6) as u8) ^ b9;
    remainder = ((remainder & 0x003F) << 8) ^ CRC14_TABLE[top as usize];

    // 末尾补 5 个 0 达到 82 位，相当于对 80 位后的 2 个 0 进行多项式除法步进
    for _ in 0..2 {
        if (remainder & (1 << 13)) != 0 {
            remainder = (remainder << 1) ^ CRC_POLYNOMIAL;
        } else {
            remainder <<= 1;
        }
    }

    remainder & CRC_MASK
}

/// 对 77 位比特切片计算 14-bit CRC
/// 输入参数 `bits77` 为每个元素 0 或 1 的长度为 77 的切片
pub fn compute_crc14_from_bits(bits77: &[u8]) -> u16 {
    assert!(bits77.len() >= 77, "bits77 长度必须至少为 77");
    let mut bytes = [0u8; 10];
    for i in 0..77 {
        if bits77[i] != 0 {
            bytes[i / 8] |= 1 << (7 - (i % 8));
        }
    }
    compute_crc14_payload77(&bytes)
}

/// 对 77 比特有效载荷（以 10 字节存储，最后 3 位为 0）追加 14 位 CRC，生成 91 位（以 12 字节存储）
pub fn add_crc14_bytes(payload10: &[u8; 10], out12: &mut [u8; 12]) {
    out12[..10].copy_from_slice(payload10);
    // 清空第 9 字节的低 3 位和第 10 字节
    out12[9] &= 0xF8;
    out12[10] = 0;
    out12[11] = 0;

    // 对 82 位（77 载荷 + 5 零）计算 CRC
    let checksum = compute_crc14(out12, 82);

    // 将 14-bit CRC 填入 bits 77..90
    out12[9] |= (checksum >> 11) as u8;
    out12[10] = (checksum >> 3) as u8;
    out12[11] = (checksum << 5) as u8;
}

/// 从 91 位比特流（或以 12 字节存储）中提取接收到的 14-bit CRC
pub fn extract_crc14_from_bytes(a91: &[u8; 12]) -> u16 {
    (((a91[9] & 0x07) as u16) << 11) | ((a91[10] as u16) << 3) | ((a91[11] >> 5) as u16)
}

/// 校验接收到的 91 位比特数组（每个元素为 0 或 1）的 CRC14 是否一致
pub fn check_crc14_bits(cw91: &[u8]) -> bool {
    if cw91.len() < 91 {
        return false;
    }
    let calculated = compute_crc14_from_bits(&cw91[..77]);
    
    // 提取收到的 14-bit CRC
    let mut received: u16 = 0;
    for i in 0..14 {
        received = (received << 1) | (cw91[77 + i] as u16 & 1);
    }
    
    calculated == received
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crc14_basic() {
        // 简单测试向量验证
        let data = [0xAA, 0x55, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
        let mut full = [0u8; 12];
        add_crc14_bytes(&data, &mut full);
        
        let extracted = extract_crc14_from_bytes(&full);
        
        // 转换为比特数组测试
        let mut bits91 = vec![0u8; 91];
        for i in 0..91 {
            bits91[i] = (full[i / 8] >> (7 - (i % 8))) & 1;
        }
        
        assert!(check_crc14_bits(&bits91));
        assert_eq!(compute_crc14_from_bits(&bits91[..77]), extracted);
        
        // 破坏 1 比特后必须校验失败
        bits91[10] ^= 1;
        assert!(!check_crc14_bits(&bits91));
    }
}
