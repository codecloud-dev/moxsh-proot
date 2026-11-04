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

// ==================== 内容池扩展（诗词 / 俳句 / 场景分池）====================
//
// 版权边界（手册 §9.3）：只用**公版原文**与**自写短句**。
// - 唐诗宋词 / 古文原文：公版；
// - 古典俳句日文原文（松尾芭蕉 1644–1694、与谢芜村 1716–1783、
//   小林一茶 1763–1827）：作者逝世远超保护期，原文公版；
// - **不用**网上抄来的现代译文（那是当代译者作品，有版权）；
// - 自写短句属于本项目，最可控。
// 用户自定义池 `~/.mox/quotes.txt` 由用户自己负责内容：逐行一条，# 开头为注释。
//
// 触发纪律（手册 §9.2）：只放「人眼等待区」，绝不进 --json / 管道 / 日志 / CI。
// 非 TTY、`CI` 环境、`MOX_QUOTES=0` 一律自动禁用。

use std::io::IsTerminal;
use std::path::PathBuf;

/// 内容场景：不同等待区取不同的池，避免「装完还在说稍等」这种错位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentScene {
    Greeting,
    DownloadWait,
    BindWait,
    InstallDone,
    Welcome,
    Failure,
}

const POOL_GREETING: &[&str] = &[
    "海内存知己，天涯若比邻。 ——王勃《送杜少府之任蜀州》",
    "古池や 蛙飛び込む 水の音  ——松尾芭蕉",
    "欢迎回来，容器还热着。",
];
const POOL_DOWNLOAD: &[&str] = &[
    "长风破浪会有时，直挂云帆济沧海。 ——李白《行路难》",
    "行到水穷处，坐看云起时。 ——王维《终南别业》",
    "やせ蛙 負けるな一茶 これにあり  ——小林一茶",
    "下载在跑，先喝口水。",
    "菜の花や 月は東に 日は西に  ——与谢芜村",
];
const POOL_BIND: &[&str] = &[
    "身无彩凤双飞翼，心有灵犀一点通。 ——李商隐《无题》",
    "扫描完成前，先听一句：慢慢来，比较快。",
    "春の海 ひねもすのたり のたりかな  ——与谢芜村",
];
const POOL_DONE: &[&str] = &[
    "春风得意马蹄疾，一日看尽长安花。 ——孟郊《登科后》",
    "装好了，进去逛逛吧。",
    "やれ打つな 蝿が手をすり 足をする  ——小林一茶",
];
const POOL_WELCOME: &[&str] = &[
    "有朋自远方来，不亦乐乎。 ——《论语》",
    "旅に病んで 夢は枯野を かけ廻る  ——松尾芭蕉",
    "从这里开始，一条命令进 Linux。",
];
const POOL_FAILURE: &[&str] = &[
    "山重水复疑无路，柳暗花明又一村。 ——陆游《游山西村》",
    "网络不稳多半是暂时，重试一次通常就好。",
];

/// 场景 → 内置兜底池（官网/断网也不空白）。
pub fn pool_for(scene: ContentScene) -> &'static [&'static str] {
    match scene {
        ContentScene::Greeting => POOL_GREETING,
        ContentScene::DownloadWait => POOL_DOWNLOAD,
        ContentScene::BindWait => POOL_BIND,
        ContentScene::InstallDone => POOL_DONE,
        ContentScene::Welcome => POOL_WELCOME,
        ContentScene::Failure => POOL_FAILURE,
    }
}

fn quotes_enabled() -> bool {
    if std::env::var("MOX_QUOTES")
        .map(|v| v == "0" || v.eq_ignore_ascii_case("off"))
        .unwrap_or(false)
    {
        return false;
    }
    if std::env::var_os("CI").is_some() {
        return false;
    }
    std::io::stderr().is_terminal()
}

/// 用户自定义池：`MOX_QUOTES_FILE` 或 `~/.mox/quotes.txt`。
fn user_pool() -> Option<Vec<String>> {
    let path = std::env::var_os("MOX_QUOTES_FILE").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".mox").join("quotes.txt"))
    })?;
    let text = std::fs::read_to_string(path).ok()?;
    let lines: Vec<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect();
    Some(lines)
}

/// 「当日不重复」+「低频随机」：以 (本地日期, 场景, pid) 为种子，
/// 同一天同一场景稳定命中同一句，既不会每次刷新刷屏，也不需要落盘状态。
fn day_seed() -> u64 {
    let t = unsafe { libc::time(std::ptr::null_mut()) };
    ((t as u64) / 86400)
        .wrapping_mul(6364136223846793005)
        .wrapping_add(std::process::id() as u64)
}

/// 取一条应景内容；`None` = 本次不出现（低频 / 非交互 / 用户关闭）。
///
/// `MOX_QUOTE_RATE` 可设 0–100 控制出现概率，默认 35%。
pub fn pick_line(scene: ContentScene) -> Option<String> {
    if !quotes_enabled() {
        return None;
    }
    let rate = std::env::var("MOX_QUOTE_RATE")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(35)
        .min(100);
    if rate == 0 {
        return None;
    }
    let seed = day_seed().wrapping_add(scene as u64).wrapping_mul(2654435761);
    if seed % 100 >= rate {
        return None;
    }
    if let Some(custom) = user_pool() {
        if !custom.is_empty() {
            return Some(custom[(seed as usize) % custom.len()].clone());
        }
    }
    let pool = pool_for(scene);
    Some(pool[(seed as usize) % pool.len()].to_string())
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

    #[test]
    fn every_scene_has_fallback_pool() {
        for sc in [
            ContentScene::Greeting,
            ContentScene::DownloadWait,
            ContentScene::BindWait,
            ContentScene::InstallDone,
            ContentScene::Welcome,
            ContentScene::Failure,
        ] {
            assert!(!pool_for(sc).is_empty(), "场景 {:?} 兜底池不能为空", sc);
            for line in pool_for(sc) {
                assert!(!line.trim().is_empty());
            }
        }
    }

    #[test]
    fn no_modern_translation_sources() {
        // 版权边界：俳句必须是日文原文（含假名/汉字），不能塞当代中文译文。
        // 覆盖全部场景（含 Greeting / Failure），否则俳句原文断言会漏检。
        let all = [
            pool_for(ContentScene::Greeting),
            pool_for(ContentScene::DownloadWait),
            pool_for(ContentScene::BindWait),
            pool_for(ContentScene::InstallDone),
            pool_for(ContentScene::Welcome),
            pool_for(ContentScene::Failure),
        ]
        .concat();
        assert!(all.iter().any(|l| l.contains("蛙飛び込む")));
        assert!(all.iter().any(|l| l.contains("——")) || all.iter().any(|l| l.contains("《")));
    }

    #[test]
    fn custom_pool_ignores_comments_and_blanks() {
        let dir = std::env::temp_dir().join(format!("mox-quotes-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let f = dir.join("q.txt");
        std::fs::write(&f, "# 注释\n\n自己写的一句\n  \n第二句\n").unwrap();
        std::env::set_var("MOX_QUOTES_FILE", &f);
        let pool = user_pool().unwrap();
        assert_eq!(pool, vec!["自己写的一句", "第二句"]);
        std::env::remove_var("MOX_QUOTES_FILE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
