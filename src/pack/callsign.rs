//! FT8 呼号 28-bit 与 58-bit 编解码模块 (完全对齐 WSJT-X 规范)

use crate::hash::{cache_callsign, hash_callsign, lookup_callsign_22};
use super::text::*;

pub const NTOKENS: u32 = 2063592;
pub const MAX22: u32 = 4194304; // 2^22

/// 将标准呼号字符转换为 c6 格式 (最多 6 字符)
fn format_basecall_c6(call: &str) -> Option<[u8; 6]> {
    let call = call.to_ascii_uppercase();
    let bytes = call.as_bytes();
    let len = bytes.len();
    if len < 3 || len > 7 {
        return None;
    }

    let mut c6 = [b' '; 6];

    // 特殊前缀置换
    if call.starts_with("3DA0") && len <= 7 {
        c6[0] = b'3';
        c6[1] = b'D';
        c6[2] = b'0';
        let rem = &bytes[4..];
        c6[3..3 + rem.len()].copy_from_slice(rem);
        return Some(c6);
    } else if call.starts_with("3X") && bytes[2].is_ascii_alphabetic() && len <= 7 {
        c6[0] = b'Q';
        let rem = &bytes[2..];
        c6[1..1 + rem.len()].copy_from_slice(rem);
        return Some(c6);
    }

    // 检查数字位置并右对齐
    if bytes[2].is_ascii_digit() && len <= 6 {
        // 例如 "AB0XYZ"
        c6[..len].copy_from_slice(bytes);
        Some(c6)
    } else if bytes[1].is_ascii_digit() && len <= 5 {
        // 例如 "A0XYZ" -> " A0XYZ"
        c6[1..1 + len].copy_from_slice(bytes);
        Some(c6)
    } else {
        None
    }
}

/// 尝试将标准呼号打包为 28 位整数
pub fn pack_basecall(call: &str) -> Option<u32> {
    let c6 = format_basecall_c6(call)?;

    let i0 = CHARS_37.iter().position(|&x| x == c6[0])? as u32;
    let i1 = CHARS_36.iter().position(|&x| x == c6[1])? as u32;
    let i2 = CHARS_10.iter().position(|&x| x == c6[2])? as u32;
    let i3 = CHARS_27.iter().position(|&x| x == c6[3])? as u32;
    let i4 = CHARS_27.iter().position(|&x| x == c6[4])? as u32;
    let i5 = CHARS_27.iter().position(|&x| x == c6[5])? as u32;

    let mut n = i0;
    n = n * 36 + i1;
    n = n * 10 + i2;
    n = n * 27 + i3;
    n = n * 27 + i4;
    n = n * 27 + i5;

    Some(n)
}

/// 解析 CQ 附加修饰符 (CQ nnn 或 CQ a[bcd])
pub fn parse_cq_modifier(call: &str) -> Option<u32> {
    if !call.starts_with("CQ ") {
        return None;
    }
    let rest = &call[3..].trim();
    if rest.len() == 3 && rest.chars().all(|c| c.is_ascii_digit()) {
        let n: u32 = rest.parse().ok()?;
        return Some(n); // 000..999
    }
    if rest.len() >= 1 && rest.len() <= 4 && rest.chars().all(|c| c.is_ascii_alphabetic()) {
        let mut m: u32 = 0;
        for c in rest.chars() {
            let cu = c.to_ascii_uppercase() as u32;
            m = 27 * m + (cu - ('A' as u32) + 1);
        }
        return Some(1000 + m);
    }
    None
}

/// 将呼号或特殊 token 打包为 28 位整数并提取 suffix flag (ip)
/// - 返回: `(n28, ip)`
pub fn pack28(call: &str) -> Result<(u32, u8), String> {
    let call_trim = call.trim().to_ascii_uppercase();

    // 0. 带括号的哈希呼号处理: <HEX>[CALL] 或 <HEX> 或 <CALL>
    if call_trim.starts_with('<') {
        if let Some(r_bracket) = call_trim.find('>') {
            let inside = call_trim[1..r_bracket].trim();

            // 若存在 [CALL] 后缀，如 <ED3C6B>[BG5VDH]
            let real_call = if let Some(l_sq) = call_trim.find('[') {
                if let Some(r_sq) = call_trim.find(']') {
                    if r_sq > l_sq {
                        Some(call_trim[l_sq + 1..r_sq].trim())
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(c) = real_call {
                if !c.is_empty() {
                    cache_callsign(c);
                }
            }

            // 1. 如果尖括号内是纯十六进制数值 (如 <ED3C6B> 或 <1A2B3C>)
            if inside.len() <= 6 && !inside.is_empty() && inside.chars().all(|c| c.is_ascii_hexdigit()) {
                if let Ok(n_hex) = u32::from_str_radix(inside, 16) {
                    if n_hex < MAX22 {
                        let n28 = NTOKENS + n_hex;
                        return Ok((n28, 0));
                    }
                }
            }

            // 2. 如果尖括号内是具体呼号 (如 <BG5VDH>)
            if inside.len() >= 3 && inside.len() <= 11 {
                let n22 = hash_callsign(inside, 22);
                cache_callsign(inside);
                let n28 = NTOKENS + n22;
                return Ok((n28, 0));
            }
        }
    }

    // 1. 特殊 Token
    if call_trim == "DE" {
        return Ok((0, 0));
    }
    if call_trim == "QRZ" {
        return Ok((1, 0));
    }
    if call_trim == "CQ" {
        return Ok((2, 0));
    }

    // 2. CQ 修饰符 (CQ 000..999 或 CQ ABCD)
    if call_trim.starts_with("CQ ") && call_trim.len() < 8 {
        if let Some(v) = parse_cq_modifier(&call_trim) {
            return Ok((3 + v, 0));
        }
    }

    // 3. /P 或 /R 后缀检查
    let mut ip: u8 = 0;
    let mut base_call = call_trim.as_str();
    if call_trim.ends_with("/P") || call_trim.ends_with("/R") {
        ip = 1;
        base_call = &call_trim[..call_trim.len() - 2];
    }

    // 4. 尝试标准呼号打包
    if let Some(n_base) = pack_basecall(base_call) {
        cache_callsign(&call_trim);
        let n28 = NTOKENS + MAX22 + n_base;
        return Ok((n28, ip));
    }

    // 5. 无法按标准呼号打包，采用 22-bit 哈希打包
    if call_trim.len() >= 3 && call_trim.len() <= 11 {
        let n22 = hash_callsign(&call_trim, 22);
        cache_callsign(&call_trim);
        let n28 = NTOKENS + n22;
        return Ok((n28, 0));
    }

    Err(format!("无法打包呼号: {}", call))
}

/// 从 28 位整数解包出呼号或 Token
pub fn unpack28(n28: u32, ip: u8, i3: u8) -> Result<String, String> {
    // 1. 特殊 Token
    if n28 < NTOKENS {
        if n28 == 0 {
            return Ok("DE".to_string());
        }
        if n28 == 1 {
            return Ok("QRZ".to_string());
        }
        if n28 == 2 {
            return Ok("CQ".to_string());
        }
        if n28 <= 1002 {
            let nnn = n28 - 3;
            return Ok(format!("CQ {:03}", nnn));
        }
        if n28 <= 532443 {
            let mut n = n28 - 1003;
            let mut aaaa = [b' '; 4];
            for i in (0..4).rev() {
                let rem = (n % 27) as usize;
                aaaa[i] = CHARS_27[rem];
                n /= 27;
            }
            let s = String::from_utf8_lossy(&aaaa).trim().to_string();
            return Ok(format!("CQ {}", s));
        }
        return Err(format!("未知的 n28 Token: {}", n28));
    }

    let n28_rem = n28 - NTOKENS;

    // 2. 22-bit 哈希呼号 (严格格式化为 <HEX>[CALL] 或 <HEX>)
    if n28_rem < MAX22 {
        let hex_str = format!("{:06X}", n28_rem);
        if let Some(call) = lookup_callsign_22(n28_rem) {
            return Ok(format!("<{}>[{}]", hex_str, call));
        }
        return Ok(format!("<{}>", hex_str));
    }

    // 3. 标准呼号解包
    let mut n = n28_rem - MAX22;
    let mut c6 = [b' '; 6];

    c6[5] = CHARS_27[(n % 27) as usize];
    n /= 27;
    c6[4] = CHARS_27[(n % 27) as usize];
    n /= 27;
    c6[3] = CHARS_27[(n % 27) as usize];
    n /= 27;
    c6[2] = CHARS_10[(n % 10) as usize];
    n /= 10;
    c6[1] = CHARS_36[(n % 36) as usize];
    n /= 36;
    c6[0] = CHARS_37[(n % 37) as usize];

    let mut result = String::new();

    // 恢复特殊前缀
    if c6[0] == b'3' && c6[1] == b'D' && c6[2] == b'0' && c6[3] != b' ' {
        result.push_str("3DA0");
        let rest = String::from_utf8_lossy(&c6[3..]);
        result.push_str(rest.trim());
    } else if c6[0] == b'Q' && c6[1].is_ascii_alphabetic() {
        result.push_str("3X");
        let rest = String::from_utf8_lossy(&c6[1..]);
        result.push_str(rest.trim());
    } else {
        let s = String::from_utf8_lossy(&c6);
        result.push_str(s.trim());
    }

    if result.len() < 3 {
        return Err(format!("解包出的呼号太短: {}", result));
    }

    if ip != 0 {
        if i3 == 1 {
            result.push_str("/R");
        } else if i3 == 2 {
            result.push_str("/P");
        }
    }

    cache_callsign(&result);
    Ok(result)
}

/// 非标准呼号 58-bit 打包 (最多 11 个字符)
pub fn pack58(call: &str) -> Option<u64> {
    let clean = call.trim_matches(|c| c == '<' || c == '>').trim().to_ascii_uppercase();
    if clean.len() < 3 || clean.len() > 11 {
        return None;
    }

    let mut res: u64 = 0;
    for &b in clean.as_bytes() {
        let j = char_to_38(b)? as u64;
        res = res * 38 + j;
    }

    cache_callsign(&clean);
    Some(res)
}

/// 非标准呼号 58-bit 解包
pub fn unpack58(mut n58: u64) -> Result<String, String> {
    let mut c11 = [b' '; 11];
    for i in (0..11).rev() {
        let rem = (n58 % 38) as usize;
        c11[i] = CHARS_38[rem];
        n58 /= 38;
    }

    let s = String::from_utf8_lossy(&c11).trim().to_string();
    if s.len() < 3 {
        return Err(format!("unpack58 呼号太短: {}", s));
    }

    cache_callsign(&s);
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pack28_standard_calls() {
        let test_calls = ["W1AW", "K1ABC", "BG5XYZ", "OH8JK", "VK4BLE", "JA1ABC", "3DA0XYZ", "3XA0BC"];
        for call in test_calls {
            let (n28, ip) = pack28(call).expect("打包失败");
            assert_eq!(ip, 0);
            let unpacked = unpack28(n28, ip, 1).expect("解包失败");
            assert_eq!(unpacked, call);
        }
    }

    #[test]
    fn test_pack28_with_suffix() {
        let (n28, ip) = pack28("W1AW/R").expect("打包失败");
        assert_eq!(ip, 1);
        let unpacked = unpack28(n28, ip, 1).expect("解包失败");
        assert_eq!(unpacked, "W1AW/R");

        let (n28_p, ip_p) = pack28("W1AW/P").expect("打包失败");
        assert_eq!(ip_p, 1);
        let unpacked_p = unpack28(n28_p, ip_p, 2).expect("解包失败");
        assert_eq!(unpacked_p, "W1AW/P");
    }

    #[test]
    fn test_pack28_tokens() {
        let tokens = ["CQ", "DE", "QRZ", "CQ 123", "CQ POTA", "CQ DX"];
        for tok in tokens {
            let (n28, ip) = pack28(tok).expect("Token 打包失败");
            assert_eq!(ip, 0);
            let unpacked = unpack28(n28, ip, 1).expect("Token 解包失败");
            assert_eq!(unpacked, tok);
        }
    }

    #[test]
    fn test_pack58_roundtrip() {
        let nonstd = "PJ4/KA1ABC";
        let n58 = pack58(nonstd).expect("pack58 失败");
        let unpacked = unpack58(n58).expect("unpack58 失败");
        assert_eq!(unpacked, nonstd);
    }
}
