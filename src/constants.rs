//! FT8 物理层与帧结构常量定义

/// FT8 采样率 (Hz)
pub const SAMPLE_RATE: usize = 12000;

/// FT8 时隙周期 (秒)
pub const SLOT_TIME: f32 = 15.0;

/// FT8 符号周期 (秒) = 1920 / 12000 = 0.160s
pub const SYMBOL_TIME: f32 = 0.160;

/// 每个符号的采样点数 (12000 * 0.16)
pub const NSPS: usize = 1920;

/// 8-FSK 音调间隔 (Hz) = 1 / 0.16s
pub const TONE_SPACING: f32 = 6.25;

/// 总符号数 (7 + 29 + 7 + 29 + 7)
pub const NUM_SYMBOLS: usize = 79;

/// 数据符号数 (58 个符号，每个符号 3 比特，对应 174 位)
pub const NUM_DATA_SYMBOLS: usize = 58;

/// Costas 同步符号总数 (3 * 7 = 21)
pub const NUM_SYNC_SYMBOLS: usize = 21;

/// 有效载荷比特数
pub const PAYLOAD_BITS: usize = 77;

/// CRC 校验比特数
pub const CRC_BITS: usize = 14;

/// 信息比特总数 (载荷 77 + CRC 14)
pub const INFO_BITS: usize = 91;

/// LDPC 编码后总比特数 (58 * 3)
pub const CODEWORD_BITS: usize = 174;

/// LDPC 奇偶校验比特数 (174 - 91)
pub const PARITY_BITS: usize = 83;

/// 7x7 Costas 序列同步图案
pub const COSTAS_PATTERN: [u8; 7] = [3, 1, 4, 0, 6, 5, 2];

/// Costas 同步符号在 79 个符号中的起始位置
pub const COSTAS_POSITIONS: [usize; 3] = [0, 36, 72];

/// 3-bit 数据映射到 8-FSK 音调编号的 Gray 映射表
/// bits (0..7) -> tone (0..7)
pub const GRAY_MAP: [u8; 8] = [0, 1, 3, 2, 5, 6, 4, 7];

/// 8-FSK 音调编号还原为 3-bit 数据的逆 Gray 映射表
/// tone (0..7) -> bits (0..7)
pub const GRAY_INV: [u8; 8] = [0, 1, 3, 2, 6, 4, 5, 7];

/// 判断指定符号索引是否属于 Costas 同步符号
#[inline]
pub fn is_sync_symbol(idx: usize) -> bool {
    (idx < 7) || (idx >= 36 && idx < 43) || (idx >= 72 && idx < 79)
}

/// 如果该符号是 Costas 同步符号，返回预期的音调编号 (0..7)
#[inline]
pub fn get_sync_tone(idx: usize) -> Option<u8> {
    if idx < 7 {
        Some(COSTAS_PATTERN[idx])
    } else if idx >= 36 && idx < 43 {
        Some(COSTAS_PATTERN[idx - 36])
    } else if idx >= 72 && idx < 79 {
        Some(COSTAS_PATTERN[idx - 72])
    } else {
        None
    }
}

/// 将 79 个符号索引映射为 58 个数据符号索引 (0..57)
#[inline]
pub fn symbol_to_data_idx(idx: usize) -> Option<usize> {
    if idx >= 7 && idx < 36 {
        Some(idx - 7)
    } else if idx >= 43 && idx < 72 {
        Some(idx - 7 - 7)
    } else {
        None
    }
}

/// 将 58 个数据符号索引 (0..57) 映射回 79 个符号在帧中的实际索引
#[inline]
pub fn data_to_symbol_idx(data_idx: usize) -> usize {
    if data_idx < 29 {
        data_idx + 7
    } else {
        data_idx + 7 + 7
    }
}
