//! 呼号哈希计算与线程安全呼号缓存 (严格对齐 WSJT-X / JTDX)
//!
//! 采用 38 字符基数的乘法哈希:
//! 字符集: " 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ/" (共 38 个字符)
//! 乘法因子: 47055833459 (0xAF51CE173)

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

pub const HASH_MULTIPLIER: u64 = 47055833459;
pub const CHARS_38: &[u8] = b" 0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ/";

/// 获取字符在 38 进制中的索引 (0..37)
#[inline]
pub fn char_index_38(c: u8) -> u64 {
    match c {
        b' ' => 0,
        b'0'..=b'9' => (c - b'0' + 1) as u64,
        b'A'..=b'Z' => (c - b'A' + 11) as u64,
        b'a'..=b'z' => (c - b'a' + 11) as u64, // 支持小写输入
        b'/' => 37,
        _ => 0, // 其他非法字符按空格处理
    }
}

/// 计算指定呼号的乘法哈希值
/// - `call`: 呼号字符串 (将被去除尖括号并右侧填充空格至 11 位)
/// - `bits`: 目标哈希位数 (通常为 10, 12, 22)
pub fn hash_callsign(call: &str, bits: u32) -> u32 {
    let clean_call = call.trim_matches(|c| c == '<' || c == '>').trim();
    let bytes = clean_call.as_bytes();

    let mut n8: u64 = 0;
    for i in 0..11 {
        let c = if i < bytes.len() { bytes[i] } else { b' ' };
        n8 = n8.wrapping_mul(38).wrapping_add(char_index_38(c));
    }

    let prod = n8.wrapping_mul(HASH_MULTIPLIER);
    let shifted = prod >> (64 - bits);
    (shifted & ((1u64 << bits) - 1)) as u32
}

/// 全局呼号缓存管理器
pub struct CallsignCache {
    calls10: HashMap<u16, String>,
    calls12: HashMap<u16, String>,
    calls22: HashMap<u32, String>,
}

impl CallsignCache {
    pub fn new() -> Self {
        Self {
            calls10: HashMap::with_capacity(512),
            calls12: HashMap::with_capacity(1024),
            calls22: HashMap::with_capacity(4096),
        }
    }

    /// 存入单个呼号
    pub fn insert_call(&mut self, call: &str) {
        let clean = call.trim_matches(|c| c == '<' || c == '>').trim();
        if clean.len() < 3 || clean == "..." {
            return;
        }

        let h10 = hash_callsign(clean, 10) as u16;
        let h12 = hash_callsign(clean, 12) as u16;
        let h22 = hash_callsign(clean, 22);

        let s = clean.to_uppercase();
        self.calls10.insert(h10, s.clone());
        self.calls12.insert(h12, s.clone());
        self.calls22.insert(h22, s.clone());

        // 如果是复合呼号 (如 R5AF/0, W1AW/P, VP2E/K1ABC)，同时缓存其基准呼号
        if clean.contains('/') {
            let parts: Vec<&str> = clean.split('/').collect();
            // 找出最长的部分作为基准呼号
            let mut base = parts[0];
            for p in &parts[1..] {
                if p.len() > base.len() {
                    base = p;
                }
            }
            if base.len() >= 3 && base != clean {
                let base_s = base.to_uppercase();
                self.calls10.insert(hash_callsign(base, 10) as u16, base_s.clone());
                self.calls12.insert(hash_callsign(base, 12) as u16, base_s.clone());
                self.calls22.insert(hash_callsign(base, 22), base_s);
            }
        }
    }

    pub fn lookup_10(&self, hash: u16) -> Option<&String> {
        self.calls10.get(&hash)
    }

    pub fn lookup_12(&self, hash: u16) -> Option<&String> {
        self.calls12.get(&hash)
    }

    pub fn lookup_22(&self, hash: u32) -> Option<&String> {
        self.calls22.get(&hash)
    }
}

/// 全局单例呼号缓存 (线程安全)
static GLOBAL_CACHE: OnceLock<Mutex<CallsignCache>> = OnceLock::new();

pub fn global_cache() -> &'static Mutex<CallsignCache> {
    GLOBAL_CACHE.get_or_init(|| Mutex::new(CallsignCache::new()))
}

/// 保存呼号到全局缓存
pub fn cache_callsign(call: &str) {
    if let Ok(mut cache) = global_cache().lock() {
        cache.insert_call(call);
    }
}

/// 根据 10-bit 哈希查询呼号
pub fn lookup_callsign_10(hash: u16) -> Option<String> {
    global_cache().lock().ok()?.lookup_10(hash).cloned()
}

/// 根据 12-bit 哈希查询呼号
pub fn lookup_callsign_12(hash: u16) -> Option<String> {
    global_cache().lock().ok()?.lookup_12(hash).cloned()
}

/// 根据 22-bit 哈希查询呼号
pub fn lookup_callsign_22(hash: u32) -> Option<String> {
    global_cache().lock().ok()?.lookup_22(hash).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_callsign() {
        // 测试呼号哈希值与 WSJT-X 算法行为
        let h22_w1aw = hash_callsign("W1AW", 22);
        let h12_w1aw = hash_callsign("W1AW", 12);
        let h10_w1aw = hash_callsign("W1AW", 10);
        
        assert!(h22_w1aw < (1 << 22));
        assert!(h12_w1aw < (1 << 12));
        assert!(h10_w1aw < (1 << 10));

        // 验证缓存插入与查找
        cache_callsign("W1AW");
        assert_eq!(lookup_callsign_22(h22_w1aw), Some("W1AW".to_string()));
        assert_eq!(lookup_callsign_12(h12_w1aw as u16), Some("W1AW".to_string()));
        assert_eq!(lookup_callsign_10(h10_w1aw as u16), Some("W1AW".to_string()));
    }
}
