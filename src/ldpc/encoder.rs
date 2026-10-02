//! LDPC(174, 91) 编码器及校验函数

use super::constants::*;

/// 计算一个字节的奇偶校验位（如果 1 的个数为奇数返回 1，否则返回 0）
#[inline]
pub fn parity8(mut x: u8) -> u8 {
    x ^= x >> 4;
    x ^= x >> 2;
    x ^= x >> 1;
    x & 1
}

/// 对 91 位信息比特（打包在 12 字节中，MSB 在前）进行 LDPC(174, 91) 编码
/// 返回 22 字节（174 比特）的打包码字
pub fn encode174_packed(message91: &[u8; LDPC_K_BYTES]) -> [u8; LDPC_N_BYTES] {
    let mut codeword = [0u8; LDPC_N_BYTES];
    // 前 12 字节拷贝信息比特
    codeword[..LDPC_K_BYTES].copy_from_slice(message91);
    // 清除第 12 字节（索引 11）超出 91 位的低 5 位，确保校验位起始位干净
    codeword[LDPC_K_BYTES - 1] &= 0xE0;

    // 第一个校验位存放在第 11 字节的 bit 4 (0x10)
    let mut col_mask: u8 = 0x80 >> (LDPC_K % 8); // 91 % 8 = 3 -> 0x10
    let mut col_idx: usize = LDPC_K_BYTES - 1;

    for i in 0..LDPC_M {
        let mut nsum = 0u8;
        for j in 0..LDPC_K_BYTES {
            let bits = message91[j] & LDPC_GENERATOR[i][j];
            nsum ^= parity8(bits);
        }

        if (nsum & 1) != 0 {
            codeword[col_idx] |= col_mask;
        }

        col_mask >>= 1;
        if col_mask == 0 {
            col_mask = 0x80;
            col_idx += 1;
        }
    }

    codeword
}

/// 对 91 位比特数组（每个元素为 0 或 1）进行 LDPC 编码
/// 输出 174 位比特数组（前 91 位为信息位，后 83 位为校验位）
pub fn encode174_bits(message91: &[u8; LDPC_K]) -> [u8; LDPC_N] {
    let mut codeword = [0u8; LDPC_N];
    codeword[..LDPC_K].copy_from_slice(message91);

    // 先打包为 12 字节
    let mut msg_bytes = [0u8; LDPC_K_BYTES];
    for (i, &b) in message91.iter().enumerate() {
        if b != 0 {
            msg_bytes[i / 8] |= 0x80 >> (i % 8);
        }
    }

    // 计算 83 个校验位
    for i in 0..LDPC_M {
        let mut nsum = 0u8;
        for j in 0..LDPC_K_BYTES {
            let bits = msg_bytes[j] & LDPC_GENERATOR[i][j];
            nsum ^= parity8(bits);
        }
        codeword[LDPC_K + i] = nsum & 1;
    }

    codeword
}

/// 检查 174 位码字是否满足全部 83 个 LDPC 奇偶校验方程
/// 返回未满足的校验方程数量（0 表示完全满足，为有效码字）
pub fn ldpc_check(codeword: &[u8; LDPC_N]) -> usize {
    let mut errors = 0;

    for m in 0..LDPC_M {
        let num = LDPC_NUM_ROWS[m] as usize;
        let mut sum = 0u8;
        for i in 0..num {
            let bit_idx = (LDPC_NM[m][i] - 1) as usize;
            sum ^= codeword[bit_idx];
        }
        if sum != 0 {
            errors += 1;
        }
    }

    errors
}

/// 将 22 字节打包的 174 位码字解包为 174 个独立的 u8 比特 (0 或 1)
pub fn unpack_codeword(packed: &[u8; LDPC_N_BYTES]) -> [u8; LDPC_N] {
    let mut bits = [0u8; LDPC_N];
    for i in 0..LDPC_N {
        let byte_val = packed[i / 8];
        let bit_val = (byte_val >> (7 - (i % 8))) & 1;
        bits[i] = bit_val;
    }
    bits
}

/// 将 174 个独立的 u8 比特打包为 22 字节
pub fn pack_codeword(bits: &[u8; LDPC_N]) -> [u8; LDPC_N_BYTES] {
    let mut packed = [0u8; LDPC_N_BYTES];
    for (i, &b) in bits.iter().enumerate() {
        if b != 0 {
            packed[i / 8] |= 0x80 >> (i % 8);
        }
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode174_and_check() {
        // 创建一个测试消息（91 位）
        let mut msg_bytes = [0u8; 12];
        msg_bytes[0] = 0xAA;
        msg_bytes[1] = 0x55;
        msg_bytes[2] = 0x12;
        msg_bytes[3] = 0x34;
        msg_bytes[4] = 0x56;
        msg_bytes[5] = 0x78;
        msg_bytes[6] = 0x9A;
        msg_bytes[7] = 0xBC;
        msg_bytes[8] = 0xDE;
        msg_bytes[9] = 0xF0;
        msg_bytes[10] = 0x42;
        msg_bytes[11] = 0xE0; // top 3 bits valid

        let packed_cw = encode174_packed(&msg_bytes);
        let bits_cw = unpack_codeword(&packed_cw);

        // 校验生成的码字必须满足所有 83 个 LDPC 方程
        let errors = ldpc_check(&bits_cw);
        assert_eq!(errors, 0, "LDPC 编码后的码字必须通过所有校验方程");

        // encode174_bits 也应当产生完全一致的结果
        let mut msg_bits = [0u8; 91];
        for i in 0..91 {
            msg_bits[i] = (msg_bytes[i / 8] >> (7 - (i % 8))) & 1;
        }
        let bits_cw2 = encode174_bits(&msg_bits);
        assert_eq!(bits_cw, bits_cw2);
    }

    #[test]
    fn test_ldpc_check_detects_errors() {
        let msg_bytes = [0x55; 12];
        let packed_cw = encode174_packed(&msg_bytes);
        let mut bits_cw = unpack_codeword(&packed_cw);

        assert_eq!(ldpc_check(&bits_cw), 0);

        // 翻转第 10 个比特
        bits_cw[10] ^= 1;
        let errors = ldpc_check(&bits_cw);
        assert!(errors > 0, "翻转比特后应能检测出错误校验方程");
    }
}
