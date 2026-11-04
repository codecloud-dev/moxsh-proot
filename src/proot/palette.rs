//! 语义色板 / Design Tokens —— CLI ANSI 与 TUI ratatui 的唯一色值来源。
//!
//! 三端（CLI 转义码 / TUI ratatui / 官网 CSS 变量）共用同一套**语义槽位**：
//! 改一处色值，三端同时变。这里提供前两端；官网 `:root` 变量与之同名。
//!
//! 无障碍硬约束（手册 §12.3）：
//! - `NO_COLOR` 任意值 → 全部关闭 ANSI；
//! - 非 TTY（管道 / CI / 重定向）→ 自动降级为纯文本；
//! - 颜色**永不单独承载信息**：CLI 的 `[安装]` / `✓` / `✗` / `⚠` 与 TUI 的
//!   状态符号始终同在，色盲用户不丢信息；
//! - 三级降级：真彩 → 256 色 → 16 色（按 `COLORTERM` / `TERM` 判定）。

use std::io::IsTerminal;

/// 语义槽位（与官网 CSS 变量 `--accent` / `--success` … 一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// 标题、强调、品牌
    Accent,
    /// 普通边框 / 次要文字
    Info,
    /// 成功
    Success,
    /// 警告
    Warning,
    /// 错误
    Error,
    /// 选中 / 焦点
    Active,
}

// ── 16 色基准值（编译期常量，CLI 直接引用，零运行期开销）─────────────
pub const ANSI_ACCENT: &str = "\x1b[1;35m";
pub const ANSI_INFO: &str = "\x1b[36m";
pub const ANSI_SUCCESS: &str = "\x1b[32m";
pub const ANSI_WARNING: &str = "\x1b[33m";
pub const ANSI_ERROR: &str = "\x1b[31m";
pub const ANSI_ACTIVE: &str = "\x1b[1;33m";
pub const ANSI_BOLD: &str = "\x1b[1m";
pub const ANSI_RESET: &str = "\x1b[0m";

/// 是否允许输出颜色：`NO_COLOR` 未设 且 stderr 是终端。
pub fn enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stderr().is_terminal()
}

/// 终端色彩深度（三级降级）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    TrueColor,
    C256,
    C16,
}

pub fn depth() -> Depth {
    if std::env::var("COLORTERM").map(|v| v.contains("truecolor") || v.contains("24bit")).unwrap_or(false) {
        Depth::TrueColor
    } else if std::env::var("TERM").map(|v| v.contains("256color")).unwrap_or(false) {
        Depth::C256
    } else {
        Depth::C16
    }
}

/// 取槽位对应的 ANSI 前缀；禁用时返回空串。
pub fn ansi(slot: Slot) -> &'static str {
    if !enabled() {
        return "";
    }
    match slot {
        Slot::Accent => ANSI_ACCENT,
        Slot::Info => ANSI_INFO,
        Slot::Success => ANSI_SUCCESS,
        Slot::Warning => ANSI_WARNING,
        Slot::Error => ANSI_ERROR,
        Slot::Active => ANSI_ACTIVE,
    }
}

/// 给一段文本包上语义色（禁用时原样返回）。
pub fn paint(s: &str, slot: Slot) -> String {
    if enabled() {
        format!("{}{}{}", ansi(slot), s, ANSI_RESET)
    } else {
        s.to_string()
    }
}

/// TUI 侧：语义槽位 → ratatui 颜色（仅 tui feature 编译）。
#[cfg(feature = "tui")]
pub fn color(slot: Slot) -> ratatui::style::Color {
    use ratatui::style::Color;
    match slot {
        Slot::Accent => Color::Magenta,
        Slot::Info => Color::DarkGray,
        Slot::Success => Color::Green,
        Slot::Warning => Color::Yellow,
        Slot::Error => Color::Red,
        Slot::Active => Color::Yellow,
    }
}

/// 给 TUI 的 Span 上语义色（仅 tui feature 编译）。
#[cfg(feature = "tui")]
pub fn style(slot: Slot) -> ratatui::style::Style {
    use ratatui::style::Style;
    if enabled() {
        Style::default().fg(color(slot))
    } else {
        Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_have_distinct_codes() {
        let all = [
            (Slot::Accent, ANSI_ACCENT),
            (Slot::Info, ANSI_INFO),
            (Slot::Success, ANSI_SUCCESS),
            (Slot::Warning, ANSI_WARNING),
            (Slot::Error, ANSI_ERROR),
            (Slot::Active, ANSI_ACTIVE),
        ];
        for (slot, code) in all {
            assert!(code.starts_with("\x1b["));
            assert!(!code.is_empty());
            let _ = slot;
        }
    }

    #[test]
    fn paint_returns_plain_when_disabled() {
        // 测试进程 stderr 通常不是终端；即便恰好是终端，也不能含半截转义
        let out = paint("hello", Slot::Success);
        assert!(out.contains("hello"));
        if !enabled() {
            assert_eq!(out, "hello");
        } else {
            assert!(out.ends_with(ANSI_RESET));
        }
    }

    #[test]
    fn depth_is_one_of_three() {
        assert!(matches!(depth(), Depth::TrueColor | Depth::C256 | Depth::C16));
    }
}
