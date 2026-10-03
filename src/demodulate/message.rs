//! FT8 解调消息结构化解析与通联状态机 (QSO State Machine)
//!
//! 提供满足工程与上位机控制要求的结构化解码输出，涵盖：
//! - 真实时间延迟 DT (已校正时间窗口偏差)
//! - 消息发送方/接收方呼号解析与提取
//! - 归属国家/地区及其中文名查询
//! - 通联生命周期阶段判定 (广播CQ、定向呼叫、上报SNR、回复SNR、73结束等)
//! - 网格与距离计算

use crate::dxcc::{grid_distance, grid_to_latlon, lookup_callsign_country};

/// 消息类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// 标准通联消息 (Type 1 / 2)
    Standard,
    /// 非标准复合呼号消息 (Type 4)
    NonstandardCall,
    /// 自由文本消息 (Type 0.0)
    FreeText,
    /// 遥测与特殊格式消息 (Type 0.5)
    Telemetry,
}

/// 通联生命周期阶段
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QsoStage {
    /// 1. 广播 CQ (如 "CQ BD4SUR OM99" 或 "CQ DX BG5VDH OL02")
    BroadcastCq,
    /// 2. 定向呼叫应答 (如 "BD4SUR VR2XXX OL02"，呼叫对方并附带己方网格)
    DirectedCall,
    /// 3. 初次上报信号报告 (如 "VR2XXX BD4SUR -10" 或 "+05")
    ReportSNR,
    /// 4. 回复确认并上报信号报告 (如 "BD4SUR VR2XXX R-08")
    ReportSNRWithR,
    /// 5. 收到报告并确认 (如 "RRR" 或 "RR73")
    Confirmation,
    /// 6. 通联结束 73 (如 "73")
    Finished73,
    /// 自由文本聊天
    FreeText,
    /// 未知或非标准阶段
    Unknown,
}

/// 完整结构化的 FT8 解调消息
#[derive(Debug, Clone, PartialEq)]
pub struct Ft8DecodedMessage {
    /// 1. 时间延迟 DT (单位: 秒，已相对于真实 15 秒时隙窗口校正)
    pub dt: f32,
    /// 2. 信噪比 SNR (单位: dB，如 -14, +6)
    pub snr: i32,
    /// 载波音频中心频率 (单位: Hz，如 1109.0)
    pub freq: f32,
    /// 3. 完整消息文本
    /// 文本内若存在哈希，统一形如 "<ED3C6B>[BG5VDH]" 或 "<1A2B3C>" (十六进制大写)
    pub message: String,
    /// 4. 消息发送方呼号的归属国家/地区英文名 (如 "China", "United States", 未知时为空字符串)
    pub country: String,
    /// 4.1 消息发送方呼号的归属国家/地区中文名 (如 "中国", "美国")
    pub country_cn: String,
    /// 5. 消息类型 (标准消息, 非标准呼号, 自由文本, 遥测)
    pub message_type: MessageType,
    /// 6. 消息发送方呼号 (Callsign DE，若为复合呼号或哈希呼号已提纯，如 "BG5VDH")
    pub sender_callsign: String,
    /// 7. 消息接收方呼号 (Callsign TO，若为 CQ 或广播则留空 "")
    pub receiver_callsign: String,
    /// 8. 消息属于通联生命周期的哪一个阶段
    pub qso_stage: QsoStage,
    /// 9. 消息中的网格定位 (Maidenhead Grid，如 "OL02"，若无则留空 "")
    pub grid: String,
    /// 10. 信号频漂 (单位: Hz，在 12.64s 发射期内的频率漂移量)
    pub drift: f32,
    /// 11. 请求调用序列号 (调用者传入的 uint64 标识，用于多路、多帧或异步流水线关联上下文)
    pub sequence_id: u64,
}

impl Ft8DecodedMessage {
    /// 从原始解调信号构造结构化消息，并结合 `window_start_offset` 进行 DT 时间基准校正 (默认 sequence_id = 0)
    pub fn parse(
        raw_dt: f32,
        snr: i32,
        freq: f32,
        message_text: &str,
        window_start_offset: f32,
    ) -> Self {
        Self::parse_with_sequence(raw_dt, snr, freq, 0.0, message_text, window_start_offset, 0)
    }

    /// 包含频漂 (drift) 的结构化消息解析函数 (默认 sequence_id = 0)
    pub fn parse_with_drift(
        raw_dt: f32,
        snr: i32,
        freq: f32,
        drift: f32,
        message_text: &str,
        window_start_offset: f32,
    ) -> Self {
        Self::parse_with_sequence(raw_dt, snr, freq, drift, message_text, window_start_offset, 0)
    }

    /// 包含频漂与调用序列号 (sequence_id) 的全参结构化解析函数
    pub fn parse_with_sequence(
        raw_dt: f32,
        snr: i32,
        freq: f32,
        drift: f32,
        message_text: &str,
        window_start_offset: f32,
        sequence_id: u64,
    ) -> Self {
        let corrected_dt = raw_dt + window_start_offset;
        let text = message_text.trim();
        let tokens: Vec<&str> = text.split_whitespace().collect();

        let mut sender_callsign = String::new();
        let mut receiver_callsign = String::new();
        let mut grid = String::new();
        let mut qso_stage = QsoStage::Unknown;
        let mut message_type = MessageType::Standard;

        if tokens.is_empty() {
            return Self {
                dt: corrected_dt,
                snr,
                freq,
                message: text.to_string(),
                country: String::new(),
                country_cn: String::new(),
                message_type,
                sender_callsign,
                receiver_callsign,
                qso_stage,
                grid,
                drift,
                sequence_id,
            };
        }

        // 识别自由文本标志 (如以引号开头或不满足呼号特征)
        if tokens.len() == 1 {
            // 极短自由文本或遥测
            message_type = MessageType::FreeText;
            qso_stage = QsoStage::FreeText;
        } else if tokens[0] == "CQ" || tokens[0].starts_with("CQ") {
            // 广播 CQ:
            // "CQ BD4SUR OM99"
            // "CQ DX BD4SUR OM99"
            // "CQ 050 BD4SUR OM99"
            qso_stage = QsoStage::BroadcastCq;
            receiver_callsign = String::new(); // CQ 广播接收方留空

            if tokens.len() >= 3 && (tokens[1].len() <= 4 || tokens[1].chars().all(|c| c.is_ascii_digit())) {
                // 带 CQ 修饰符 (如 CQ DX, CQ NA, CQ 050)
                sender_callsign = extract_clean_call(tokens[2]);
                if tokens.len() >= 4 {
                    if is_valid_grid(tokens[3]) {
                        grid = tokens[3].to_string();
                    }
                }
            } else if tokens.len() >= 2 {
                sender_callsign = extract_clean_call(tokens[1]);
                if tokens.len() >= 3 && is_valid_grid(tokens[2]) {
                    grid = tokens[2].to_string();
                }
            }
        } else if tokens.len() >= 2 {
            let to_call = extract_clean_call(tokens[0]);
            let de_call = extract_clean_call(tokens[1]);
            receiver_callsign = to_call;
            sender_callsign = de_call;

            if tokens.len() >= 3 {
                let tail = tokens[2].trim().to_ascii_uppercase();
                if tail == "73" {
                    qso_stage = QsoStage::Finished73;
                } else if tail == "RRR" || tail == "RR73" {
                    qso_stage = QsoStage::Confirmation;
                } else if tail.starts_with("R+") || tail.starts_with("R-") || (tail.starts_with('R') && tail[1..].chars().all(|c| c.is_ascii_digit() || c == '-' || c == '+')) {
                    qso_stage = QsoStage::ReportSNRWithR;
                } else if is_snr_report(&tail) {
                    qso_stage = QsoStage::ReportSNR;
                } else if is_valid_grid(&tail) {
                    grid = tail;
                    qso_stage = QsoStage::DirectedCall;
                } else {
                    qso_stage = QsoStage::DirectedCall;
                }
            } else {
                // 两段式消息 (如 "BD4SUR VR2XXX")
                qso_stage = QsoStage::DirectedCall;
            }
        }

        // 判断消息类型
        if sender_callsign.contains('/') || receiver_callsign.contains('/') {
            message_type = MessageType::NonstandardCall;
        }

        // 查询发送方国家/地区
        let (country, country_cn) = if let Some(entity) = lookup_callsign_country(&sender_callsign) {
            (entity.name_en.clone(), entity.name_cn.clone())
        } else {
            (String::new(), String::new())
        };

        Self {
            dt: corrected_dt,
            snr,
            freq,
            message: text.to_string(),
            country,
            country_cn,
            message_type,
            sender_callsign,
            receiver_callsign,
            qso_stage,
            grid,
            drift,
            sequence_id,
        }
    }

    /// 计算从本机网格到此消息发送方网格的大圆距离 (单位: 公里 km)
    pub fn distance_to_my_grid(&self, my_grid: &str) -> Option<f64> {
        if self.grid.is_empty() {
            return None;
        }
        grid_distance(my_grid, &self.grid)
    }

    /// 获取发送方网格中心点的经纬度坐标 (纬度 Lat, 经度 Lon)
    pub fn sender_latlon(&self) -> Option<(f64, f64)> {
        if self.grid.is_empty() {
            return None;
        }
        grid_to_latlon(&self.grid)
    }
}

/// 提取干净的呼号 (剥离外层的方括号、尖括号等修饰)
fn extract_clean_call(token: &str) -> String {
    let t = token.trim();
    // 优先提取 [CALL] 内的呼号 (如 <ED3C6B>[BG5VDH] -> BG5VDH)
    if let Some(pos) = t.find('[') {
        if let Some(end_pos) = t.find(']') {
            if end_pos > pos {
                return t[pos + 1..end_pos].trim().to_ascii_uppercase();
            }
        }
    }
    // 去除尖括号 (如 <VR2XYZ> -> VR2XYZ)
    if t.starts_with('<') && t.ends_with('>') && t.len() > 2 {
        return t[1..t.len() - 1].trim().to_ascii_uppercase();
    }
    t.to_ascii_uppercase()
}

/// 校验是否为合法的梅登黑德网格 (4 位或 6 位)
fn is_valid_grid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 4 && b.len() != 6 {
        return false;
    }
    if !b[0].is_ascii_alphabetic() || !b[1].is_ascii_alphabetic() {
        return false;
    }
    if !b[2].is_ascii_digit() || !b[3].is_ascii_digit() {
        return false;
    }
    if b.len() == 6 && (!b[4].is_ascii_alphabetic() || !b[5].is_ascii_alphabetic()) {
        return false;
    }
    true
}

/// 校验是否为 +/- 数值的信号报告 (如 +05, -12, -09)
fn is_snr_report(s: &str) -> bool {
    if !s.starts_with('+') && !s.starts_with('-') {
        return false;
    }
    s[1..].chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_cq_broadcast() {
        let msg = Ft8DecodedMessage::parse(0.45, -12, 1109.0, "CQ BD4SUR OM99", -0.9);
        assert!((msg.dt - (-0.45)).abs() < 1e-4); // 0.45 - 0.9 = -0.45
        assert_eq!(msg.snr, -12);
        assert_eq!(msg.freq, 1109.0);
        assert_eq!(msg.sender_callsign, "BD4SUR");
        assert_eq!(msg.receiver_callsign, ""); // CQ 时接收方留空
        assert_eq!(msg.grid, "OM99");
        assert_eq!(msg.qso_stage, QsoStage::BroadcastCq);
        assert_eq!(msg.country, "China");
        assert_eq!(msg.country_cn, "中国");
    }

    #[test]
    fn test_parse_hash_call_and_stages() {
        // 哈希呼号带方括号解析与回复 SNR 测试
        let msg = Ft8DecodedMessage::parse(1.40, 6, 7074.0, "BD4SUR <ED3C6B>[BG5VDH] R-08", -0.9);
        assert!((msg.dt - 0.50).abs() < 1e-4); // 1.40 - 0.9 = +0.50
        assert_eq!(msg.receiver_callsign, "BD4SUR");
        assert_eq!(msg.sender_callsign, "BG5VDH");
        assert_eq!(msg.grid, "");
        assert_eq!(msg.qso_stage, QsoStage::ReportSNRWithR);
        assert_eq!(msg.country_cn, "中国");

        // 73 结束测试
        let msg73 = Ft8DecodedMessage::parse(1.0, 0, 1000.0, "BG5VDH VR2XYZ 73", 0.0);
        assert_eq!(msg73.qso_stage, QsoStage::Finished73);
        assert_eq!(msg73.receiver_callsign, "BG5VDH");
        assert_eq!(msg73.sender_callsign, "VR2XYZ");
        assert_eq!(msg73.country_cn, "中国香港");
    }
}
