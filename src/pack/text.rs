//! FT8 文本字符表与基础文本工具

/// Free Text (Type 0.0) 的 42 字符全集
pub const CHARS_42: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ+-./?";

/// Callsign (58-bit / Multiplicative Hash) 的 38 字符集
pub const CHARS_38: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ/";

/// Standard Callsign c6 字段字符集:
/// i0: ALPHANUM_SPACE (37: ' ' + 0-9 + A-Z)
pub const CHARS_37: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// i1: ALPHANUM (36: 0-9 + A-Z)
pub const CHARS_36: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// i2: NUMERIC (10: 0-9)
pub const CHARS_10: &[u8] = b"0123456789";

/// i3, i4, i5: LETTERS_SPACE (27: ' ' + A-Z)
pub const CHARS_27: &[u8] = b" ABCDEFGHIJKLMNOPQRSTUVWXYZ";

/// 查找字符在 CHARS_42 中的索引
pub fn char_to_42(c: u8) -> Option<usize> {
    let cu = c.to_ascii_uppercase();
    CHARS_42.iter().position(|&x| x == cu)
}

/// 查找字符在 CHARS_38 中的索引
pub fn char_to_38(c: u8) -> Option<usize> {
    let cu = c.to_ascii_uppercase();
    CHARS_38.iter().position(|&x| x == cu)
}

/// 将 71-bit 自由文本无符号大整数（以 9 字节存储）解码为最多 13 字符字符串
pub fn unpack_free_text(b71: &[u8; 9]) -> String {
    let mut bytes = *b71;
    let mut c14 = [b' '; 13];

    for idx in (0..13).rev() {
        let mut rem: u32 = 0;
        for i in 0..9 {
            rem = (rem << 8) | (bytes[i] as u32);
            bytes[i] = (rem / 42) as u8;
            rem %= 42;
        }
        c14[idx] = CHARS_42[rem as usize];
    }

    String::from_utf8_lossy(&c14).trim().to_string()
}

/// 将最多 13 字符自由文本编码为 71-bit（存入 9 字节，大端）
pub fn pack_free_text(text: &str) -> Option<[u8; 9]> {
    let trimmed = text.trim();
    if trimmed.len() > 13 {
        return None;
    }

    let mut b71 = [0u8; 9];
    let padded = format!("{:<13}", trimmed.to_ascii_uppercase());

    for &c in padded.as_bytes() {
        let val = char_to_42(c)? as u32;
        // b71 = b71 * 42 + val
        let mut carry: u64 = val as u64;
        for i in (0..9).rev() {
            let prod = (b71[i] as u64) * 42 + carry;
            b71[i] = (prod & 0xFF) as u8;
            carry = prod >> 8;
        }
        if carry != 0 {
            return None; // 溢出
        }
    }

    Some(b71)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_free_text_roundtrip() {
        let msg = "HELLO WORLD";
        let packed = pack_free_text(msg).expect("打包失败");
        let unpacked = unpack_free_text(&packed);
        assert_eq!(unpacked, msg);

        let msg2 = "TNX 73 GL!";
        // '!' 不在 42 字符表中，应返回 None
        assert!(pack_free_text(msg2).is_none());

        let msg3 = "TNX 73 GL";
        let packed3 = pack_free_text(msg3).expect("打包失败");
        let unpacked3 = unpack_free_text(&packed3);
        assert_eq!(unpacked3, msg3);
    }
}
