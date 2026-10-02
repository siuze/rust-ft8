//! 77-bit FT8 消息打包与解包协议模块 (完全对齐 WSJT-X 规范)

pub mod text;
pub mod callsign;
pub mod grid;

use text::*;
use callsign::*;
use grid::*;
use crate::hash::lookup_callsign_12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ft8MessageType {
    FreeText,        // i3 = 0, n3 = 0
    Dxpedition,      // i3 = 0, n3 = 1
    EuVhf,           // i3 = 0, n3 = 2
    ArrlFd,          // i3 = 0, n3 = 3 or 4
    Telemetry,       // i3 = 0, n3 = 5
    Standard,        // i3 = 1 or 2
    ArrlRtty,        // i3 = 3
    NonstandardCall, // i3 = 4
    Wwrof,           // i3 = 5
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ft8Message {
    pub text: String,
    pub msg_type: Ft8MessageType,
    pub call_to: Option<String>,
    pub call_de: Option<String>,
    pub extra: Option<String>,
    pub i3: u8,
    pub payload: [u8; 10],
}

/// 解析载荷的 i3 (bits 74..76)
#[inline]
pub fn get_i3(payload: &[u8; 10]) -> u8 {
    (payload[9] >> 3) & 0x07
}

/// 解析载荷的 n3 (bits 71..73)
#[inline]
pub fn get_n3(payload: &[u8; 10]) -> u8 {
    ((payload[8] << 2) & 0x04) | ((payload[9] >> 6) & 0x03)
}

/// 将 77-bit 载荷（以 10 字节存储，高位优先，第 9 字节低 3 位补 0）解包为 `Ft8Message`
pub fn unpack77(payload: &[u8; 10]) -> Result<Ft8Message, String> {
    let i3 = get_i3(payload);

    match i3 {
        0 => {
            let n3 = get_n3(payload);
            if n3 == 0 {
                // Free text (Type 0.0)
                let mut b71 = [0u8; 9];
                let mut carry = 0u8;
                for i in 0..9 {
                    b71[i] = (carry << 7) | (payload[i] >> 1);
                    carry = payload[i] & 0x01;
                }
                let free_str = unpack_free_text(&b71);
                Ok(Ft8Message {
                    text: free_str,
                    msg_type: Ft8MessageType::FreeText,
                    call_to: None,
                    call_de: None,
                    extra: None,
                    i3: 0,
                    payload: *payload,
                })
            } else if n3 == 5 {
                // Telemetry (Type 0.5)
                let mut b71 = [0u8; 9];
                let mut carry = 0u8;
                for i in 0..9 {
                    b71[i] = (carry << 7) | (payload[i] >> 1);
                    carry = payload[i] & 0x01;
                }
                let hex_str = b71.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join("");
                Ok(Ft8Message {
                    text: hex_str,
                    msg_type: Ft8MessageType::Telemetry,
                    call_to: None,
                    call_de: None,
                    extra: None,
                    i3: 0,
                    payload: *payload,
                })
            } else {
                Err(format!("暂不支持的 i3=0, n3={} 模式", n3))
            }
        }
        1 | 2 => {
            // Standard message (Type 1 / Type 2)
            let n29a: u32 = ((payload[0] as u32) << 21)
                | ((payload[1] as u32) << 13)
                | ((payload[2] as u32) << 5)
                | ((payload[3] as u32) >> 3);

            let n29b: u32 = (((payload[3] & 0x07) as u32) << 26)
                | ((payload[4] as u32) << 18)
                | ((payload[5] as u32) << 10)
                | ((payload[6] as u32) << 2)
                | ((payload[7] as u32) >> 6);

            let ir = ((payload[7] >> 5) & 0x01) as u8;
            let igrid4: u16 = (((payload[7] & 0x1F) as u16) << 10)
                | ((payload[8] as u16) << 2)
                | ((payload[9] >> 6) as u16);

            let call_to = unpack28(n29a >> 1, (n29a & 1) as u8, i3)?;
            let call_de = unpack28(n29b >> 1, (n29b & 1) as u8, i3)?;
            let extra = unpack_grid_report(igrid4, ir);

            let mut text = format!("{} {}", call_to, call_de);
            if !extra.is_empty() {
                text.push(' ');
                text.push_str(&extra);
            }

            Ok(Ft8Message {
                text,
                msg_type: Ft8MessageType::Standard,
                call_to: Some(call_to),
                call_de: Some(call_de),
                extra: if extra.is_empty() { None } else { Some(extra) },
                i3,
                payload: *payload,
            })
        }
        4 => {
            // Nonstandard call message (Type 4)
            let n12: u16 = ((payload[0] as u16) << 4) | ((payload[1] as u16) >> 4);
            let mut n58: u64 = ((payload[1] & 0x0F) as u64) << 54;
            n58 |= (payload[2] as u64) << 46;
            n58 |= (payload[3] as u64) << 38;
            n58 |= (payload[4] as u64) << 30;
            n58 |= (payload[5] as u64) << 22;
            n58 |= (payload[6] as u64) << 14;
            n58 |= (payload[7] as u64) << 6;
            n58 |= (payload[8] as u64) >> 2;

            let iflip = (payload[8] >> 1) & 0x01;
            let nrpt = ((payload[8] & 0x01) << 1) | (payload[9] >> 7);
            let icq = (payload[9] >> 6) & 0x01;

            let call_decoded = unpack58(n58)?;
            let call_hash = lookup_callsign_12(n12).map(|c| format!("<{}>", c)).unwrap_or_else(|| "<...>".to_string());

            let (call_1, call_2) = if iflip == 0 {
                (call_hash, call_decoded)
            } else {
                (call_decoded, call_hash)
            };

            let (call_to, call_de, extra) = if icq == 0 {
                let rpt = match nrpt {
                    1 => "RRR".to_string(),
                    2 => "RR73".to_string(),
                    3 => "73".to_string(),
                    _ => String::new(),
                };
                (call_1, call_2, rpt)
            } else {
                ("CQ".to_string(), call_2, String::new())
            };

            let mut text = format!("{} {}", call_to, call_de);
            if !extra.is_empty() {
                text.push(' ');
                text.push_str(&extra);
            }

            Ok(Ft8Message {
                text,
                msg_type: Ft8MessageType::NonstandardCall,
                call_to: Some(call_to),
                call_de: Some(call_de),
                extra: if extra.is_empty() { None } else { Some(extra) },
                i3: 4,
                payload: *payload,
            })
        }
        _ => Err(format!("不支持的 i3 类型: {}", i3)),
    }
}

/// 将文本消息打包为 77-bit 载荷（存入 `[u8; 10]`）
pub fn pack77(message: &str) -> Result<[u8; 10], String> {
    let msg = message.trim();
    if msg.is_empty() {
        return Err("消息不能为空".to_string());
    }

    let parts: Vec<&str> = msg.split_whitespace().collect();

    // 1. 如果包含非标准复合呼号 (带有 '/' 且两端字符长度不属于标准 /P 或 /R)
    let has_nonstd = parts.iter().any(|p| {
        p.contains('/') && !p.ends_with("/P") && !p.ends_with("/R")
    });

    if has_nonstd {
        return pack_nonstd(&parts);
    }

    // 2. 尝试标准通联消息打包 (2 或 3 个字段)
    if parts.len() >= 2 && parts.len() <= 4 {
        if let Ok(payload) = pack_std(&parts) {
            return Ok(payload);
        }
    }

    // 3. 回退尝试自由文本打包 (最多 13 字符)
    if msg.len() <= 13 {
        if let Some(b71) = pack_free_text(msg) {
            let mut payload = [0u8; 10];
            let mut carry = 0u8;
            for i in (0..9).rev() {
                payload[i] = (b71[i] << 1) | (carry >> 7);
                carry = b71[i] & 0x80;
            }
            payload[9] = 0; // i3 = 0, n3 = 0
            return Ok(payload);
        }
    }

    Err(format!("无法打包该消息: '{}'", message))
}

fn is_callsign_or_token(s: &str) -> bool {
    let s = s.trim().to_ascii_uppercase();
    if s == "CQ" || s == "DE" || s == "QRZ" || s.starts_with("CQ ") {
        return true;
    }
    if s.starts_with('<') && s.ends_with('>') {
        return true;
    }
    s.chars().any(|c| c.is_ascii_digit())
}

fn pack_std(parts: &[&str]) -> Result<[u8; 10], String> {
    let (call_to, call_de, extra) = if parts.len() == 2 {
        (parts[0], parts[1], "")
    } else if parts.len() == 3 {
        if parts[0] == "CQ" && parse_cq_modifier(&format!("CQ {}", parts[1])).is_some() {
            let cq_tok = format!("CQ {}", parts[1]);
            (cq_tok.leak() as &str, parts[2], "")
        } else {
            (parts[0], parts[1], parts[2])
        }
    } else if parts.len() == 4 && parts[0] == "CQ" {
        if parse_cq_modifier(&format!("CQ {}", parts[1])).is_some() {
            let cq_tok = format!("CQ {}", parts[1]);
            (cq_tok.leak() as &str, parts[2], parts[3])
        } else {
            return Err("CQ 4字段格式不合法".to_string());
        }
    } else {
        return Err("字段数量不匹配标准通联格式".to_string());
    };

    if !is_callsign_or_token(call_to) || !is_callsign_or_token(call_de) {
        return Err("非合法呼号格式，应作为自由文本处理".to_string());
    }

    let (n28a, ipa) = pack28(call_to)?;
    let (n28b, ipb) = pack28(call_de)?;
    let (igrid4, ir) = pack_grid_report(extra);

    let i3: u8 = if ipa != 0 || ipb != 0 {
        if call_to.ends_with("/P") || call_de.ends_with("/P") { 2 } else { 1 }
    } else {
        1
    };

    let n29a: u32 = (n28a << 1) | (ipa as u32);
    let n29b: u32 = (n28b << 1) | (ipb as u32);

    let mut payload = [0u8; 10];
    payload[0] = (n29a >> 21) as u8;
    payload[1] = (n29a >> 13) as u8;
    payload[2] = (n29a >> 5) as u8;
    payload[3] = ((n29a << 3) as u8) | ((n29b >> 26) as u8);
    payload[4] = (n29b >> 18) as u8;
    payload[5] = (n29b >> 10) as u8;
    payload[6] = (n29b >> 2) as u8;
    payload[7] = ((n29b << 6) as u8) | ((ir & 1) << 5) | ((igrid4 >> 10) as u8 & 0x1F);
    payload[8] = (igrid4 >> 2) as u8;
    payload[9] = ((igrid4 << 6) as u8) | ((i3 & 0x07) << 3);

    Ok(payload)
}

fn pack_nonstd(parts: &[&str]) -> Result<[u8; 10], String> {
    if parts.len() < 2 {
        return Err("非标准呼号通联至少需要两个字段".to_string());
    }

    let (call_to, call_de, extra) = if parts.len() == 2 {
        (parts[0], parts[1], "")
    } else {
        (parts[0], parts[1], parts[2])
    };

    let is_cq = call_to.starts_with("CQ");
    let (iflip, call12, call58) = if is_cq {
        (0u8, "", call_de)
    } else if call_to.contains('/') {
        // call_to 含有斜杠 -> 作为 58-bit，call_de 作为 12-bit
        (1u8, call_de, call_to)
    } else {
        // call_de 含有斜杠 -> 作为 58-bit，call_to 作为 12-bit
        (0u8, call_to, call_de)
    };

    let n12 = if is_cq {
        0u16
    } else {
        crate::hash::hash_callsign(call12, 12) as u16
    };

    let n58 = pack58(call58).ok_or_else(|| format!("无法打包 58-bit 呼号: {}", call58))?;

    let nrpt = match extra {
        "RRR" => 1u8,
        "RR73" => 2u8,
        "73" => 3u8,
        _ => 0u8,
    };

    let icq: u8 = if is_cq { 1 } else { 0 };
    let i3: u8 = 4;

    let mut payload = [0u8; 10];
    payload[0] = (n12 >> 4) as u8;
    payload[1] = ((n12 << 4) as u8) | ((n58 >> 54) as u8 & 0x0F);
    payload[2] = (n58 >> 46) as u8;
    payload[3] = (n58 >> 38) as u8;
    payload[4] = (n58 >> 30) as u8;
    payload[5] = (n58 >> 22) as u8;
    payload[6] = (n58 >> 14) as u8;
    payload[7] = (n58 >> 6) as u8;
    payload[8] = ((n58 << 2) as u8) | (iflip << 1) | (nrpt >> 1);
    payload[9] = (nrpt << 7) | (icq << 6) | (i3 << 3);

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_messages_roundtrip() {
        let test_msgs = [
            "CQ W1AW FN42",
            "CQ DX OH8JK KP11",
            "VK4BLE OH8JK R-17",
            "RK6AH JH1AJT -05",
            "PA3EPP SP8NFO KN09",
            "RV6K RU3XL -13",
            "SQ8OHR UA9LL MO27",
            "JR5MJS OH8NW 73",
            "CQ JA OH1LWZ KP11",
            "SV1GN RK6AUV LN05",
            "PB5DX EI3CTB IO63",
            "CQ IZ1ANK JN33",
            "NT6Q OH8GDU -17",
            "CQ DL1UDO JO31",
            "CQ DG0OFT JO50",
            "G1XJM HA7JIV JN97",
            "SP7XIF JA2GQT -15",
            "W1AW/R K1ABC FN42",
            "W1AW K1ABC/P RRR",
            "W1AW K1ABC RR73",
        ];

        for &msg in &test_msgs {
            let packed = pack77(msg).unwrap_or_else(|e| panic!("打包失败 '{}': {}", msg, e));
            let unpacked = unpack77(&packed).unwrap_or_else(|e| panic!("解包失败 '{}': {}", msg, e));
            assert_eq!(unpacked.text, msg, "消息回环不匹配");
        }
    }

    #[test]
    fn test_nonstandard_roundtrip() {
        let test_msgs = [
            "CQ PJ4/KA1ABC",
            "<W1AW> PJ4/KA1ABC RR73",
            "PJ4/KA1ABC <W1AW> 73",
        ];

        for &msg in &test_msgs {
            let packed = pack77(msg).unwrap_or_else(|e| panic!("打包失败 '{}': {}", msg, e));
            let unpacked = unpack77(&packed).unwrap_or_else(|e| panic!("解包失败 '{}': {}", msg, e));
            assert_eq!(unpacked.text, msg);
        }
    }

    #[test]
    fn test_free_text_roundtrip() {
        let free_msgs = ["HELLO WORLD", "TNX 73 GL"];
        for &msg in &free_msgs {
            let packed = pack77(msg).unwrap_or_else(|e| panic!("打包失败 '{}': {}", msg, e));
            let unpacked = unpack77(&packed).unwrap_or_else(|e| panic!("解包失败 '{}': {}", msg, e));
            assert_eq!(unpacked.text, msg);
        }
    }
}
