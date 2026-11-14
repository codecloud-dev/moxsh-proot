//! 终端二维码（半块字符渲染），仅 `tui` feature 启用。
//!
//! 复用 `qrcode` crate 做编码（纯 Rust、byte 模式、多 ECC 等级），本模块负责把二维码
//! 矩阵渲染成终端可用的「半块字符」图：一个终端字符纵向塞两个二维码模块
//! （上模块=字符上半、下模块=字符下半），标准映射如下（**物理正确**画法，
//! 保证扫码器能识别；文档 §1.5 那张表把明暗反了，此处以实际为准）：
//!
//! ```text
//!   上模块  下模块    字符
//!   暗      暗        █  (U+2588 全块)
//!   暗      亮        ▀  (U+2580 上半块)
//!   亮      暗        ▄  (U+2584 下半块)
//!   亮      亮        空格
//! ```
//!
//! 效果：二维码纵向压缩一半高度，终端/SSH 下扫得又快又稳。
//!
//! 本模块由 `src/proot/mod.rs` 的 `#[cfg(feature = "tui")] pub mod qrcode_term;`
//! 整体门控，无需再写模块级 `cfg` 属性。

use qrcode::Color;
use qrcode::EcLevel;
use qrcode::QrCode;

/// 把文本编码成二维码模块矩阵（true = 暗模块）。`ecc` 越高越抗损、占用越大。
pub fn build_matrix(text: &str, ecc: EcLevel) -> Result<Vec<Vec<bool>>, String> {
    let code = QrCode::with_error_correction_level(text.as_bytes(), ecc)
        .map_err(|e| format!("二维码编码失败: {e}"))?;
    let w = code.width();
    let colors = code.to_colors(); // row-major：索引 i = y*w + x，Dark = 暗模块
    let mut matrix = vec![vec![false; w]; w];
    for (i, c) in colors.into_iter().enumerate() {
        matrix[i / w][i % w] = c == Color::Dark;
    }
    Ok(matrix)
}

/// 半块字符渲染：在矩阵外圈加 `quiet` 行/列安静区，再把每两行压成一个终端行。
pub fn render_halfblocks(matrix: &[Vec<bool>], quiet: usize) -> String {
    let size = matrix.len();
    if size == 0 {
        return String::new();
    }
    let total = size + quiet * 2;
    // 总行数取偶，避免最底一行（安静区）被 step_by(2) 丢弃导致缺底边
    let total = if total % 2 == 1 { total + 1 } else { total };
    // 矩阵在「总网格」中的有效区间：[quiet, quiet+size)；区间外均为安静区（亮）。
    let lo = quiet as i32;
    let hi = (quiet + size) as i32;
    let get = |r: i32, c: i32| -> bool {
        if r < lo || c < lo || r >= hi || c >= hi {
            return false; // 安静区视为亮模块
        }
        matrix[(r - lo) as usize][(c - lo) as usize]
    };
    let mut out = String::new();
    for r in (0..total).step_by(2) {
        for c in 0..total {
            let top = get(r as i32, c as i32);
            let bot = get(r as i32 + 1, c as i32);
            let ch = match (top, bot) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            out.push(ch);
        }
        out.push('\n');
    }
    out
}

/// 一步到位：编码 + 半块渲染（默认 ECC=M，quiet zone=2）。
pub fn render_uri(text: &str) -> Result<String, String> {
    let m = build_matrix(text, EcLevel::M)?;
    Ok(render_halfblocks(&m, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_square_and_odd() {
        // 一个短 URL 应当编出合法的奇数边长矩阵（含安静区前的模块数）
        let m = build_matrix("https://x.dev/activate?code=ABCD-1234", EcLevel::M).unwrap();
        assert!(!m.is_empty());
        assert_eq!(m.len(), m[0].len(), "二维码矩阵必须是方阵");
        assert_eq!(m.len() % 2, 1, "边长应为奇数（版本 N 的模块数 = 17+4N）");
    }

    #[test]
    fn halfblock_packs_two_rows() {
        // 4x4 全暗矩阵（quiet=1 → 总网格 6x6，含上下/左右安静区），半块渲染应为 3 行终端字符。
        let m = vec![vec![true; 4]; 4];
        let s = render_halfblocks(&m, 1);
        let lines: Vec<&str> = s.trim_end_matches('\n').split('\n').collect();
        assert_eq!(lines.len(), 3, "6 行模块应压成 3 行终端字符");
        // 内侧 4 列（去掉左右安静区）逐行应是：顶 ▄ / 中 █ / 底 ▀。
        let inner = |s: &str| s.chars().skip(1).take(4).collect::<String>();
        assert_eq!(inner(lines[0]), "▄▄▄▄", "顶行内侧应为全 ▄（上亮下暗）: {:?}", lines[0]);
        assert_eq!(inner(lines[1]), "████", "中行内侧应为全 █: {:?}", lines[1]);
        assert_eq!(inner(lines[2]), "▀▀▀▀", "底行内侧应为全 ▀（上暗下亮）: {:?}", lines[2]);
        // 安静区列（首尾）应为亮（空格）。
        assert_eq!(lines[0].chars().next(), Some(' '));
        assert_eq!(lines[0].chars().last(), Some(' '));
    }

    #[test]
    fn encodes_known_uri_stable() {
        // 同样输入必须编码出完全一致的矩阵（确定性，可复现）
        let a = build_matrix("mox-proot-login", EcLevel::M).unwrap();
        let b = build_matrix("mox-proot-login", EcLevel::M).unwrap();
        assert_eq!(a, b);
    }
}
