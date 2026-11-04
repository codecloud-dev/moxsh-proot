//! 终端「惊喜体验」三件套：时间上下文感知 / 古诗 / 彩蛋。
//!
//! 设计目标（引流与黏性，无依赖、纯本地、零网络、零额外 crate）：
//! - **时间上下文感知**：按本地时区把一天分 `晨 / 午 / 晚 / 夜` 四相，给出应景问候；
//! - **古诗**：每一相配一句应景古诗（出自公开典籍，零版权争议），附作者出处；
//! - **彩蛋**：固定「暗号」触发隐藏文案（`mox egg` / `MOX_EGG=1`），平时不显。
//!
//! 单一来源：本模块是 Rust 侧的唯一真源，供
//! - TUI 欢迎屏（`tui.rs` 启动时调用 [`time_banner`]）；
//! - `mox-proot-distro time` 子命令（CLI 复用，可单测）；
//! - 安装器/向导收尾文案。
//!
//! 容器内控制台 `mox` 的无参欢迎与彩蛋暗号走 shell 自包含实现
//! （`console.rs` 的 `CONSOLE_SCRIPT`），不依赖本模块——保证即使
//! distro 二进制异常，控制台也能自洽地送出问候。

/// 一天的四相（按本地时间划分）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePhase {
    /// 05:00–10:59 晨
    Morning,
    /// 11:00–16:59 午
    Noon,
    /// 17:00–22:59 晚
    Evening,
    /// 23:00–04:59 夜
    Night,
}

impl TimePhase {
    /// 由本地小时（0–23）推导时相。
    pub fn from_hour(hour: u32) -> TimePhase {
        match hour {
            5..=10 => TimePhase::Morning,
            11..=16 => TimePhase::Noon,
            17..=22 => TimePhase::Evening,
            _ => TimePhase::Night,
        }
    }

    /// 时相的中文名（单字）。
    pub fn name(self) -> &'static str {
        match self {
            TimePhase::Morning => "晨",
            TimePhase::Noon => "午",
            TimePhase::Evening => "晚",
            TimePhase::Night => "夜",
        }
    }

    /// 应景问候（时间上下文感知的核心输出）。
    pub fn greeting(self) -> &'static str {
        match self {
            TimePhase::Morning => "早安，新的一天从容器里开始 ☀",
            TimePhase::Noon => "午安，喝口水再战 ☕",
            TimePhase::Evening => "晚安，夜色正好适合折腾 🌙",
            TimePhase::Night => "夜深了，注意身体，别熬太晚 🌃",
        }
    }

    /// 应景古诗（出自公开典籍，与 [`TimePhase::poem_from`] 配套）。
    pub fn poem(self) -> &'static str {
        match self {
            TimePhase::Morning => "晨起开门雪满山，雪晴云淡日光寒。",
            TimePhase::Noon => "锄禾日当午，汗滴禾下土。",
            TimePhase::Evening => "夕阳无限好，只是近黄昏。",
            TimePhase::Night => "春眠不觉晓，处处闻啼鸟。",
        }
    }

    /// 古诗作者 / 出处标注。
    pub fn poem_from(self) -> &'static str {
        match self {
            TimePhase::Morning => "——郑燮《山中雪后》",
            TimePhase::Noon => "——李绅《悯农》",
            TimePhase::Evening => "——李商隐《登乐游原》",
            TimePhase::Night => "——孟浩然《春晓》",
        }
    }
}

/// 取得本地小时（0–23）。零依赖（仅用 libc::time + localtime）。
fn local_hour() -> u32 {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    // SAFETY：localtime 接收合法 time_t 常量指针，返回指向静态结构的指针；
    // 我们只在本次调用内读取，不跨调用持有。
    let tm_ptr = unsafe { libc::localtime(&t) };
    if tm_ptr.is_null() {
        return 0;
    }
    let tm = unsafe { *tm_ptr };
    tm.tm_hour as u32
}

/// 现在的时相（取本地时间）。
pub fn current_phase() -> TimePhase {
    TimePhase::from_hour(local_hour())
}

/// 时间上下文欢迎块（多行，供控制台 / TUI 打印）。
///
/// 形如：
///
/// ```text
/// 晚安，夜色正好适合折腾 🌙
///  春眠不觉晓，处处闻啼鸟。 ——孟浩然《春晓》
/// ```
pub fn time_banner() -> String {
    let p = current_phase();
    format!(
        "{}\n  {} {}",
        p.greeting(),
        p.poem(),
        p.poem_from()
    )
}

/// 隐藏彩蛋：给定「暗号」返回彩蛋文案；不匹配返回 `None`。
///
/// 平时不显示；特定暗号（如 `mox egg`、环境变量 `MOX_EGG=1`）触发。
/// 设计原则：彩蛋不承载任何功能，只给愿意探索的用户一个小惊喜，
/// 同时暗含「本引擎是手写对齐上游、未抄代码」的自述。
pub fn easter_egg(code: &str) -> Option<&'static str> {
    match code {
        "egg" | "彩蛋" | "konami" | "蛋" => Some(
            "🥚 彩蛋：你找到了隐藏菜单！\n\
             \x20 mox-proot 的每一行代码都是手写对齐上游 PRoot 行为——没有抄。\n\
             \x20 试试无参 `mox` 进入，看时间送你的那句诗。",
        ),
        "secret" | "秘密" | "mox" => Some(
            "🤫 嘘——这是只有我们俩知道的事：\n\
             \x20 容器里的时间，比宿主慢一拍也是一种浪漫。",
        ),
        _ => None,
    }
}

/// 环境变量彩蛋开关：`MOX_EGG=1` 时认为彩蛋被显式触发。
pub fn egg_enabled_via_env() -> bool {
    std::env::var_os("MOX_EGG").map(|v| v == "1" || v == "true").unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_from_hour_covers_all() {
        assert_eq!(TimePhase::from_hour(0), TimePhase::Night);
        assert_eq!(TimePhase::from_hour(5), TimePhase::Morning);
        assert_eq!(TimePhase::from_hour(10), TimePhase::Morning);
        assert_eq!(TimePhase::from_hour(11), TimePhase::Noon);
        assert_eq!(TimePhase::from_hour(16), TimePhase::Noon);
        assert_eq!(TimePhase::from_hour(17), TimePhase::Evening);
        assert_eq!(TimePhase::from_hour(22), TimePhase::Evening);
        assert_eq!(TimePhase::from_hour(23), TimePhase::Night);
    }

    #[test]
    fn poem_and_from_are_paired() {
        for p in [
            TimePhase::Morning,
            TimePhase::Noon,
            TimePhase::Evening,
            TimePhase::Night,
        ] {
            assert!(!p.poem().is_empty());
            assert!(p.poem_from().starts_with("——"));
        }
    }

    #[test]
    fn time_banner_has_three_lines() {
        let b = time_banner();
        assert!(b.contains('☀') || b.contains('☕') || b.contains('🌙') || b.contains('🌃'));
        assert!(b.contains("——"), "应含古诗出处：{}", b);
    }

    #[test]
    fn easter_egg_only_on_code() {
        assert!(easter_egg("egg").is_some());
        assert!(easter_egg("彩蛋").is_some());
        assert!(easter_egg("随意").is_none());
        assert!(easter_egg("").is_none());
    }
}
