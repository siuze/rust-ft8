//! 业余无线电 DXCC 国家/地区识别、中英文转换与大圆距离计算模块
//!
//! 提供：
//! - 呼号前缀匹配 DXCC 国家/地区 (内置全球常见前缀库，并支持加载外部 cty.dat)
//! - 国家/地区英文名转中文名
//! - 梅登黑德网格 (Maidenhead Grid) 与经纬度互相转换
//! - 基于地球椭球/大圆球面的两点间距离 (km) 与航向角 (度) 计算

use std::collections::HashMap;
use std::sync::OnceLock;

/// 地球平均半径 (公里)
pub const EARTH_RADIUS_KM: f64 = 6371.0;

/// DXCC 实体信息
#[derive(Debug, Clone, PartialEq)]
pub struct DxccEntity {
    /// 英文名称 (如 "China", "United States", "Japan")
    pub name_en: String,
    /// 中文名称 (如 "中国", "美国", "日本")
    pub name_cn: String,
    /// 洲别 (AS, NA, EU, AF, OC, SA, AN)
    pub continent: String,
    /// CQ 分区
    pub cq_zone: u8,
    /// ITU 分区
    pub itu_zone: u8,
}

/// 呼号与国家地区查询数据库
pub struct DxccDatabase {
    // 前缀树或按长度降序排列的前缀列表，实现最长前缀匹配 (Longest Prefix Match)
    prefix_table: Vec<(String, DxccEntity)>,
}

static GLOBAL_DXCC_DB: OnceLock<DxccDatabase> = OnceLock::new();

impl DxccDatabase {
    /// 获取全局默认的 DXCC 数据库单例
    pub fn global() -> &'static Self {
        GLOBAL_DXCC_DB.get_or_init(Self::new_builtin)
    }

    /// 创建内置核心前缀数据库
    pub fn new_builtin() -> Self {
        let mut db = Self {
            prefix_table: Vec::with_capacity(512),
        };
        db.load_builtin_rules();
        // 按前缀长度降序排列，确保优先匹配更长、更精确的前缀 (如 VR2 优先于 V)
        db.prefix_table.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        db
    }

    /// 根据呼号查询对应的 DXCC 国家/地区实体
    pub fn lookup(&self, callsign: &str) -> Option<&DxccEntity> {
        let clean = clean_callsign(callsign);
        if clean.len() < 2 {
            return None;
        }

        // 优先匹配完整基准呼号的前缀
        for (prefix, entity) in &self.prefix_table {
            if clean.starts_with(prefix) {
                return Some(entity);
            }
        }
        None
    }

    /// 从标准 cty.dat 格式文本中加载或扩充规则
    pub fn load_from_cty_dat(&mut self, content: &str) {
        // cty.dat 每条记录格式:
        // Country Name: CQ: ITU: Continent: Lat: Lon: TZ: Primary_Prefix:
        //   prefix1, =exact_call1, prefix2... ;
        for block in content.split(';') {
            let block = block.trim();
            if block.is_empty() {
                continue;
            }
            let lines: Vec<&str> = block.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
            if lines.is_empty() {
                continue;
            }

            let header = lines[0];
            let header_parts: Vec<&str> = header.split(':').map(|s| s.trim()).collect();
            if header_parts.len() < 5 {
                continue;
            }

            let name_en = header_parts[0].trim();
            let cq_zone: u8 = header_parts[1].parse().unwrap_or(0);
            let itu_zone: u8 = header_parts[2].parse().unwrap_or(0);
            let continent = header_parts[3].trim().to_uppercase();
            let name_cn = country_en_to_cn(name_en).to_string();

            let entity = DxccEntity {
                name_en: name_en.to_string(),
                name_cn,
                continent,
                cq_zone,
                itu_zone,
            };

            // 解析后续行中的所有前缀列表
            for &line in &lines[1..] {
                for token in line.split(',') {
                    let mut p = token.trim();
                    if p.is_empty() {
                        continue;
                    }
                    // 移除可能存在的覆盖标注 (如 (24)[44])
                    if let Some(pos) = p.find('(') {
                        p = &p[..pos];
                    }
                    if let Some(pos) = p.find('[') {
                        p = &p[..pos];
                    }
                    let p = p.trim_start_matches('=').trim().to_uppercase();
                    if !p.is_empty() {
                        self.prefix_table.push((p, entity.clone()));
                    }
                }
            }
        }

        // 重新按前缀长度降序排列
        self.prefix_table.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    }

    fn load_builtin_rules(&mut self) {
        let entities = [
            ("China", "中国", "AS", 24, 44, vec![
                "BA", "BD", "BG", "BH", "BI", "BJ", "BL", "BT", "BY", "BZ", "B0", "B1", "B2", "B3", "B4", "B5", "B6", "B7", "B8", "B9"
            ]),
            ("Hong Kong", "中国香港", "AS", 24, 44, vec!["VR2", "VR"]),
            ("Macao", "中国澳门", "AS", 24, 44, vec!["XX9"]),
            ("Taiwan", "中国台湾", "AS", 24, 44, vec!["BV", "BX", "BM", "BN", "BO", "BP", "BQ", "BU"]),
            ("Japan", "日本", "AS", 25, 45, vec![
                "JA", "JH", "JR", "JE", "JF", "JG", "JI", "JJ", "JK", "JL", "JM", "JN", "JO", "JP", "JQ", "JS", "7J", "7K", "7L", "7M", "7N", "8J", "8N"
            ]),
            ("United States", "美国", "NA", 5, 8, vec![
                "AA", "AB", "AC", "AD", "AE", "AF", "AG", "AH", "AI", "AJ", "AK",
                "NA", "NB", "NC", "ND", "NE", "NF", "NG", "NH", "NI", "NJ", "NK", "NL", "NM", "NN", "NO", "NP", "NQ", "NR", "NS", "NT", "NU", "NV", "NW", "NX", "NY", "NZ",
                "WA", "WB", "WC", "WD", "WE", "WF", "WG", "WH", "WI", "WJ", "WK", "WL", "WM", "WN", "WO", "WP", "WQ", "WR", "WS", "WT", "WU", "WV", "WW", "WX", "WY", "WZ",
                "K0", "K1", "K2", "K3", "K4", "K5", "K6", "K7", "K8", "K9",
                "W0", "W1", "W2", "W3", "W4", "W5", "W6", "W7", "W8", "W9",
                "N0", "N1", "N2", "N3", "N4", "N5", "N6", "N7", "N8", "N9"
            ]),
            ("South Korea", "韩国", "AS", 25, 44, vec!["HL", "DS", "DT", "D7", "D8", "D9", "6K", "6L", "6M", "6N"]),
            ("Russia (European)", "俄罗斯 (欧洲)", "EU", 16, 29, vec![
                "RA", "RC", "RD", "RE", "RF", "RG", "RN", "RU", "RV", "RW", "RX", "RY", "RZ", "UA", "UB", "UC", "UD", "UF", "UG", "UN", "UR", "UV", "UW", "UX", "UY", "UZ", "R0", "R1", "R2", "R3", "R4", "R5", "R6", "R7"
            ]),
            ("Russia (Asiatic)", "俄罗斯 (亚洲)", "AS", 18, 30, vec![
                "RA8", "RA9", "RA0", "RC8", "RC9", "RC0", "RD8", "RD9", "RD0", "UA8", "UA9", "UA0", "R8", "R9", "R0"
            ]),
            ("Germany", "德国", "EU", 14, 28, vec!["DA", "DB", "DC", "DD", "DF", "DG", "DH", "DI", "DJ", "DK", "DL", "DM", "DN", "DO", "DP", "DQ", "DR"]),
            ("England", "英国", "EU", 14, 27, vec!["G", "M", "2E", "GX", "MX", "2X"]),
            ("Scotland", "苏格兰", "EU", 14, 27, vec!["GM", "MM", "2M", "GS", "MS"]),
            ("Australia", "澳大利亚", "OC", 30, 59, vec!["VK", "AX", "VI"]),
            ("Canada", "加拿大", "NA", 4, 9, vec!["VE", "VA", "VO", "VY", "CF", "CG", "CH", "CI", "CJ", "CK", "CY", "CZ"]),
            ("France", "法国", "EU", 14, 27, vec!["F", "TM", "TK"]),
            ("Italy", "意大利", "EU", 15, 28, vec!["I", "IK", "IZ", "IU", "IQ", "IA", "IB", "IV", "IW"]),
            ("Spain", "西班牙", "EU", 14, 37, vec!["EA", "EB", "EC", "ED", "EE", "EF", "EG", "EH", "AM", "AN", "AO"]),
            ("Netherlands", "荷兰", "EU", 14, 27, vec!["PA", "PB", "PC", "PD", "PE", "PF", "PG", "PH", "PI"]),
            ("Poland", "波兰", "EU", 15, 28, vec!["SP", "SQ", "SN", "SO", "3Z", "HF"]),
            ("Brazil", "巴西", "SA", 11, 15, vec!["PP", "PQ", "PR", "PS", "PT", "PU", "PV", "PW", "PX", "PY", "ZV", "ZW", "ZX", "ZY", "ZZ"]),
            ("Argentina", "阿根廷", "SA", 13, 14, vec!["LU", "LW", "AY", "AZ", "L1", "L2", "L3", "L4", "L5", "L6", "L7", "L8", "L9"]),
            ("New Zealand", "新西兰", "OC", 32, 60, vec!["ZL", "ZM"]),
            ("Thailand", "泰国", "AS", 26, 49, vec!["HS", "E2"]),
            ("Indonesia", "印度尼西亚", "OC", 28, 54, vec!["YB", "YC", "YD", "YE", "YF", "YG", "YH", "7A", "7B", "7C", "7D", "7E", "7F", "7G", "7H", "7I", "8A", "8B", "8C", "8D", "8E"]),
            ("Malaysia", "马来西亚", "OC", 28, 54, vec!["9M", "9W"]),
            ("Singapore", "新加坡", "AS", 28, 54, vec!["9V", "S6"]),
            ("Philippines", "菲律宾", "OC", 27, 50, vec!["DU", "DV", "DW", "DX", "DY", "DZ", "4D", "4E", "4F", "4G", "4H", "4I"]),
            ("India", "印度", "AS", 22, 41, vec!["VU", "AT", "AU", "AV", "AW"]),
            ("South Africa", "南非", "AF", 38, 57, vec!["ZS", "ZR", "ZT", "ZU"]),
            ("Sweden", "瑞典", "EU", 14, 18, vec!["SM", "SA", "SB", "SC", "SD", "SE", "SF", "SG", "SH", "SI", "SJ", "SK", "SL", "7S", "8S"]),
            ("Norway", "挪威", "EU", 14, 18, vec!["LA", "LB", "LC", "LD", "LE", "LF", "LG", "LH", "LI", "LJ", "LN", "3Y", "JW", "JX"]),
            ("Finland", "芬兰", "EU", 15, 18, vec!["OH", "OF", "OG", "OI"]),
            ("Ukraine", "乌克兰", "EU", 16, 29, vec!["UR", "US", "UT", "UU", "UV", "UW", "UX", "UY", "UZ", "EM", "EN", "EO"]),
            ("Turkey", "土耳其", "EU", 20, 39, vec!["TA", "TB", "TC", "YM"]),
            ("Greece", "希腊", "EU", 20, 28, vec!["SV", "SW", "SX", "SY", "SZ", "J4"]),
        ];

        for (en, cn, cont, cq, itu, prefixes) in entities {
            let entity = DxccEntity {
                name_en: en.to_string(),
                name_cn: cn.to_string(),
                continent: cont.to_string(),
                cq_zone: cq,
                itu_zone: itu,
            };
            for p in prefixes {
                self.prefix_table.push((p.to_string(), entity.clone()));
            }
        }
    }
}

/// 清洗呼号，去除前后尖括号、前缀或后缀斜杠中的修饰部分，提取最主要的前缀
fn clean_callsign(call: &str) -> String {
    let mut s = call.trim().to_ascii_uppercase();
    // 去除 <...>
    if s.starts_with('<') && s.ends_with('>') {
        s = s[1..s.len() - 1].to_string();
    }
    // 处理带方括号的哈希呼号形如 <ED3C6B>[BG5VDH]
    if let Some(pos) = s.find('[') {
        if let Some(end_pos) = s.find(']') {
            if end_pos > pos {
                s = s[pos + 1..end_pos].to_string();
            }
        }
    }

    // 处理斜杠呼号 (如 BA4TB/P, W1AW/3, VP2E/K1ABC)
    if s.contains('/') {
        let parts: Vec<&str> = s.split('/').collect();
        // 找到最长的一部分作为基准呼号
        let mut best = parts[0];
        for &p in &parts[1..] {
            if p.len() > best.len() && !p.eq_ignore_ascii_case("P") && !p.eq_ignore_ascii_case("R") && !p.eq_ignore_ascii_case("M") && !p.eq_ignore_ascii_case("MM") && !p.eq_ignore_ascii_case("AM") {
                best = p;
            }
        }
        return best.to_string();
    }

    s
}

/// 根据呼号直接查询国家/地区实体
pub fn lookup_callsign_country(call: &str) -> Option<&'static DxccEntity> {
    DxccDatabase::global().lookup(call)
}

/// 国家/地区英文名转中文名字典映射
pub fn country_en_to_cn<'a>(en_name: &'a str) -> &'a str {
    static EN_TO_CN_MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    let map = EN_TO_CN_MAP.get_or_init(|| {
        let mut m = HashMap::with_capacity(300);
        m.insert("China", "中国");
        m.insert("Hong Kong", "中国香港");
        m.insert("Macao", "中国澳门");
        m.insert("Taiwan", "中国台湾");
        m.insert("Japan", "日本");
        m.insert("United States", "美国");
        m.insert("South Korea", "韩国");
        m.insert("North Korea", "朝鲜");
        m.insert("Russia (European)", "俄罗斯 (欧洲)");
        m.insert("Russia (Asiatic)", "俄罗斯 (亚洲)");
        m.insert("Germany", "德国");
        m.insert("England", "英国");
        m.insert("Scotland", "苏格兰");
        m.insert("Wales", "威尔士");
        m.insert("Northern Ireland", "北爱尔兰");
        m.insert("Australia", "澳大利亚");
        m.insert("Canada", "加拿大");
        m.insert("France", "法国");
        m.insert("Italy", "意大利");
        m.insert("Spain", "西班牙");
        m.insert("Netherlands", "荷兰");
        m.insert("Poland", "波兰");
        m.insert("Brazil", "巴西");
        m.insert("Argentina", "阿根廷");
        m.insert("New Zealand", "新西兰");
        m.insert("Thailand", "泰国");
        m.insert("Indonesia", "印度尼西亚");
        m.insert("Malaysia", "马来西亚");
        m.insert("Singapore", "新加坡");
        m.insert("Philippines", "菲律宾");
        m.insert("India", "印度");
        m.insert("South Africa", "南非");
        m.insert("Sweden", "瑞典");
        m.insert("Norway", "挪威");
        m.insert("Finland", "芬兰");
        m.insert("Ukraine", "乌克兰");
        m.insert("Turkey", "土耳其");
        m.insert("Greece", "希腊");
        m.insert("Portugal", "葡萄牙");
        m.insert("Belgium", "比利时");
        m.insert("Switzerland", "瑞士");
        m.insert("Austria", "奥地利");
        m.insert("Czech Republic", "捷克");
        m.insert("Hungary", "匈牙利");
        m.insert("Romania", "罗马尼亚");
        m.insert("Bulgaria", "保加利亚");
        m.insert("Ireland", "爱尔兰");
        m.insert("Denmark", "丹麦");
        m.insert("Mexico", "墨西哥");
        m.insert("Chile", "智利");
        m.insert("Colombia", "哥伦比亚");
        m.insert("Peru", "秘鲁");
        m.insert("Mongolia", "蒙古");
        m.insert("Vietnam", "越南");
        m.insert("Israel", "以色列");
        m.insert("Saudi Arabia", "沙特阿拉伯");
        m.insert("United Arab Emirates", "阿联酋");
        m.insert("Egypt", "埃及");
        m
    });

    map.get(en_name).copied().unwrap_or(en_name)
}

/// 将 4 字符或 6 字符的梅登黑德网格 (Maidenhead Grid Locator) 转换为经纬度 (纬度 Lat, 经度 Lon)
/// 范围: Lat [-90.0, +90.0], Lon [-180.0, +180.0]
/// 返回网格中心点坐标
pub fn grid_to_latlon(grid: &str) -> Option<(f64, f64)> {
    let g = grid.trim().to_ascii_uppercase();
    let b = g.as_bytes();
    if b.len() < 4 {
        return None;
    }

    if !(b[0] >= b'A' && b[0] <= b'R' && b[1] >= b'A' && b[1] <= b'R') {
        return None;
    }
    if !(b[2] >= b'0' && b[2] <= b'9' && b[3] >= b'0' && b[3] <= b'9') {
        return None;
    }

    let mut lon = -180.0 + ((b[0] - b'A') as f64) * 20.0 + ((b[2] - b'0') as f64) * 2.0;
    let mut lat = -90.0 + ((b[1] - b'A') as f64) * 10.0 + ((b[3] - b'0') as f64) * 1.0;

    if b.len() >= 6 {
        if b[4] >= b'A' && b[4] <= b'X' && b[5] >= b'A' && b[5] <= b'X' {
            lon += ((b[4] - b'A') as f64) * (2.0 / 24.0) + (1.0 / 24.0);
            lat += ((b[5] - b'A') as f64) * (1.0 / 24.0) + (0.5 / 24.0);
        } else {
            // 4 位精度中心点
            lon += 1.0;
            lat += 0.5;
        }
    } else {
        // 4 位精度中心点
        lon += 1.0;
        lat += 0.5;
    }

    Some((lat, lon))
}

/// 将经纬度转换为 4 字符或 6 字符梅登黑德网格 (默认 6 字符)
pub fn latlon_to_grid(lat: f64, lon: f64) -> String {
    let lat = lat.clamp(-90.0, 90.0) + 90.0;
    let lon = (lon + 180.0).rem_euclid(360.0);

    let f1 = (lon / 20.0).floor() as u8;
    let f2 = (lat / 10.0).floor() as u8;

    let rem_lon1 = lon - (f1 as f64) * 20.0;
    let rem_lat1 = lat - (f2 as f64) * 10.0;

    let sq1 = (rem_lon1 / 2.0).floor() as u8;
    let sq2 = (rem_lat1 / 1.0).floor() as u8;

    let rem_lon2 = rem_lon1 - (sq1 as f64) * 2.0;
    let rem_lat2 = rem_lat1 - (sq2 as f64) * 1.0;

    let ss1 = (rem_lon2 / (2.0 / 24.0)).floor() as u8;
    let ss2 = (rem_lat2 / (1.0 / 24.0)).floor() as u8;

    let c1 = (b'A' + f1) as char;
    let c2 = (b'A' + f2) as char;
    let c3 = (b'0' + sq1) as char;
    let c4 = (b'0' + sq2) as char;
    let c5 = (b'a' + ss1.min(23)) as char;
    let c6 = (b'a' + ss2.min(23)) as char;

    format!("{}{}{}{}{}{}", c1, c2, c3, c4, c5, c6)
}

/// 计算两个经纬度坐标之间的大圆距离 (Great-Circle Distance)
/// 使用 Haversine 公式，单位：公里 (km)
pub fn great_circle_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();

    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());

    EARTH_RADIUS_KM * c
}

/// 计算从起点 (lat1, lon1) 到终点 (lat2, lon2) 的初始大圆航向角 (Bearing)
/// 返回角度范围 [0.0, 360.0) 度 (正北为 0°, 顺时针递增)
pub fn great_circle_bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let d_lon = (lon2 - lon1).to_radians();

    let y = d_lon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * d_lon.cos();

    let mut bearing = y.atan2(x).to_degrees();
    bearing = (bearing + 360.0).rem_euclid(360.0);
    bearing
}

/// 直接计算两个梅登黑德网格之间的大圆距离 (km)
pub fn grid_distance(grid1: &str, grid2: &str) -> Option<f64> {
    let (lat1, lon1) = grid_to_latlon(grid1)?;
    let (lat2, lon2) = grid_to_latlon(grid2)?;
    Some(great_circle_distance(lat1, lon1, lat2, lon2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callsign_lookup() {
        let e1 = lookup_callsign_country("BD4SUR").expect("BD4SUR 应匹配中国");
        assert_eq!(e1.name_en, "China");
        assert_eq!(e1.name_cn, "中国");

        let e2 = lookup_callsign_country("VR2XYZ").expect("VR2XYZ 应匹配香港");
        assert_eq!(e2.name_en, "Hong Kong");
        assert_eq!(e2.name_cn, "中国香港");

        let e3 = lookup_callsign_country("W1AW/3").expect("W1AW 应匹配美国");
        assert_eq!(e3.name_en, "United States");

        let e4 = lookup_callsign_country("<ED3C6B>[BG5VDH]").expect("带方括号呼号应匹配中国");
        assert_eq!(e4.name_en, "China");
    }

    #[test]
    fn test_grid_and_distance() {
        // 北京附近的网格 OM89 (约 39.5°N, 117.0°E)
        let (lat1, lon1) = grid_to_latlon("OM89").expect("OM89 解析失败");
        assert!((lat1 - 39.5).abs() < 1.0);
        assert!((lon1 - 117.0).abs() < 1.0);

        // 经纬度往返转换互逆测试
        let grid_back = latlon_to_grid(lat1, lon1);
        assert_eq!(&grid_back[..4], "OM89");

        // 上海网格 PM01 (约 31.5°N, 121.0°E)
        let dist = grid_distance("OM89", "PM01").expect("计算网格距离失败");
        // 北京到上海直线距离约 1050 ~ 1100 公里
        assert!(dist > 950.0 && dist < 1200.0, "实际距离: {}", dist);
    }
}
