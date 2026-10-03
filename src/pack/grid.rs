//! FT8 网格与信号报告 15-bit + 1-bit 编解码模块

pub const MAXGRID4: u16 = 32400; // 18 * 18 * 10 * 10

/// 将 4 字符网格定位符、信号报告或特殊 Token 打包为 `(igrid4, ir)`
/// - 返回: `(igrid15, ro_bit)`
pub fn pack_grid_report(extra: &str) -> (u16, u8) {
    let s = extra.trim().to_ascii_uppercase();
    if s.is_empty() {
        return (MAXGRID4 + 1, 0);
    }

    if s == "RRR" {
        return (MAXGRID4 + 2, 0);
    }
    if s == "RR73" {
        return (MAXGRID4 + 3, 0);
    }
    if s == "73" {
        return (MAXGRID4 + 4, 0);
    }

    // 检查 "R [4位网格]"
    if s.starts_with("R ") && s.len() == 6 {
        let grid = &s[2..];
        if is_valid_grid4(grid) {
            let igrid = encode_grid4(grid);
            return (igrid, 1);
        }
    }

    // 检查标准 4 字符网格 (如 FN42, PM95)
    if s.len() == 4 && is_valid_grid4(&s) {
        let igrid = encode_grid4(&s);
        return (igrid, 0);
    }

    // 检查信号报告 (如 +05, -12, R+03, R-18)
    let (has_r, rpt_str) = if s.starts_with('R') {
        (1, &s[1..])
    } else {
        (0, s.as_str())
    };

    if let Ok(val) = rpt_str.parse::<i32>() {
        if val >= -30 && val <= 30 {
            let irpt = (35 + val) as u16;
            return (MAXGRID4 + irpt, has_r);
        }
    }

    // 默认空
    (MAXGRID4 + 1, 0)
}

/// 从 `(igrid4, ir)` 解包出网格、报告或 Token
pub fn unpack_grid_report(igrid4: u16, ir: u8) -> String {
    if igrid4 <= MAXGRID4 {
        // 4 字符网格定位符
        let mut n = igrid4;
        let d3 = (n % 10) as u8 + b'0';
        n /= 10;
        let d2 = (n % 10) as u8 + b'0';
        n /= 10;
        let l1 = (n % 18) as u8 + b'A';
        n /= 18;
        let l0 = (n % 18) as u8 + b'A';

        let grid = format!("{}{}{}{}", l0 as char, l1 as char, d2 as char, d3 as char);
        if ir > 0 {
            format!("R {}", grid)
        } else {
            grid
        }
    } else {
        let irpt = igrid4 - MAXGRID4;
        match irpt {
            1 => String::new(),
            2 => "RRR".to_string(),
            3 => "RR73".to_string(),
            4 => "73".to_string(),
            _ => {
                let val = (irpt as i32) - 35;
                if val >= -30 && val <= 30 {
                    if ir > 0 {
                        format!("R{:+03}", val)
                    } else {
                        format!("{:+03}", val)
                    }
                } else {
                    String::new()
                }
            }
        }
    }
}

fn is_valid_grid4(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 4
        && (b[0] >= b'A' && b[0] <= b'R')
        && (b[1] >= b'A' && b[1] <= b'R')
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
}

fn encode_grid4(s: &str) -> u16 {
    let b = s.as_bytes();
    let mut n: u16 = (b[0] - b'A') as u16;
    n = n * 18 + (b[1] - b'A') as u16;
    n = n * 10 + (b[2] - b'0') as u16;
    n = n * 10 + (b[3] - b'0') as u16;
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_roundtrip() {
        let grids = ["FN42", "PM95", "KO85", "IO91", "JN25", "KP11"];
        for g in grids {
            let (igrid, ir) = pack_grid_report(g);
            assert_eq!(ir, 0);
            let unpacked = unpack_grid_report(igrid, ir);
            assert_eq!(unpacked, g);
        }

        let r_grid = "R FN42";
        let (igrid, ir) = pack_grid_report(r_grid);
        assert_eq!(ir, 1);
        assert_eq!(unpack_grid_report(igrid, ir), r_grid);
    }

    #[test]
    fn test_reports_and_tokens() {
        let tests = ["-15", "+03", "R-20", "R+12", "RRR", "RR73", "73", ""];
        for item in tests {
            let (igrid, ir) = pack_grid_report(item);
            let unpacked = unpack_grid_report(igrid, ir);
            assert_eq!(unpacked, item);
        }
    }
}
