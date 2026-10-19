//! `mx:` 短链系统（0.2.0）。
//!
//! 短链 = rootfs 地址的简写，只映射地址，不携带启动配方（配方走 `recipes/`，
//! 见 `docs/roadmap.md` §4）。内置集只四大发行版——与 `distro install` 子命令面
//! 一一对应（项目主人明确纠正过：不造「工具链镜像」概念）：
//!
//! ```text
//! mx:alpine / mx:ubuntu / mx:debian / mx:kali
//! ```
//!
//! 解析两层语义：
//! - 内置名 → [`ResolvedLink::Distro`]：走既有 [`crate::proot::distro::DistroManager`]
//!   安装流程（架构适配 URL、sha256 动态获取、解压初始化全复用）；
//! - 用户自定义名 → [`ResolvedLink::Url`]：走 `install_custom` 流程。
//!
//! 自定义短链定义（`links.toml`，用户级 `~/.config/moxsh-proot/links.toml`）：
//!
//! ```toml
//! [links]
//! mydev = "https://example.com/rootfs.tar.xz"
//! # 多源回退：同名可跟数组，按序尝试
//! cnmirror = ["https://mirror.example.com/rootfs.tar.xz", "https://example.com/rootfs.tar.xz"]
//! ```
//!
//! TOML 解析为**手写极简子集**（依赖极简约束，见 Cargo.toml）：只认
//! `[links]` 段下的 `name = "url"` / `name = ["url", ...]` 两种形式，
//! 与 [`crate::proot::distro::parse_sha256sums`] 同思路——纯文本、可单测。

use std::collections::BTreeMap;
use std::path::Path;

use crate::proot::distro::DistroManager;
use crate::proot::{ProotError, ProotResult};

/// 短链 scheme 前缀（`mx:alpine` 的 `mx:`）。
pub const MX_PREFIX: &str = "mx:";

/// 内置短链表：`(短链名, 发行版 id)`。
///
/// 四大发行版与 `distro::builtin_specs()` 严格对齐——短链是安装面的简写，
/// 不引入第五个数据源。
pub const BUILTIN_LINKS: &[(&str, &str)] = &[
    ("alpine", "alpine"),
    ("ubuntu", "ubuntu"),
    ("debian", "debian"),
    ("kali", "kali"),
];

/// 短链解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedLink {
    /// 内置四大发行版（元素为发行版 id）。
    Distro(String),
    /// 自定义 rootfs 地址（多源按序回退；单源时只有一个元素）。
    Url(Vec<String>),
}

/// 剥掉 `mx:` 前缀。非短链形式返回 None。
pub fn strip_mx(spec: &str) -> Option<&str> {
    spec.strip_prefix(MX_PREFIX)
}

/// 判断是否为短链形式（`mx:` 开头）。
pub fn is_mx(spec: &str) -> bool {
    spec.starts_with(MX_PREFIX)
}

/// 内置短链名 → 发行版 id。
pub fn builtin_distro_id(name: &str) -> Option<&'static str> {
    BUILTIN_LINKS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| *id)
}

/// 用户自定义短链表（`links.toml` 的 `[links]` 段）。
#[derive(Debug, Clone, Default)]
pub struct LinkTable {
    /// 短链名 → 地址列表（单源一元、多源回退）。
    links: BTreeMap<String, Vec<String>>,
}

impl LinkTable {
    /// 空表。
    pub fn new() -> LinkTable {
        LinkTable::default()
    }

    /// 从文件加载（不存在 = 空表，不是错误）。
    pub fn load(path: &Path) -> ProotResult<LinkTable> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(LinkTable::new()),
            Err(e) => return Err(ProotError::Io(e)),
        };
        Ok(parse_links_toml(&text))
    }
}

/// 解析 `links.toml` 极简子集。
///
/// 规则：
/// - `[links]` 段之前的其他段一律忽略（如 `[settings]`）；
/// - `name = "url"`：单源；
/// - `name = ["url", "url2"]`：多源回退（**单行数组**，不跨行）；
/// - `#` 起头为注释；空行跳过；
/// - 段外（无任何段头）的行忽略——防止把文件头的杂项吃进来。
pub fn parse_links_toml(text: &str) -> LinkTable {
    let mut table = LinkTable::new();
    let mut in_links = false;
    for raw in text.lines() {
        let line = strip_toml_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            in_links = line == "[links]";
            continue;
        }
        if !in_links {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let name = k.trim();
        if !DistroManager::valid_id(name) {
            continue; // 复用 id 白名单：防路径注入（短链名会进路径/命令）
        }
        let value = v.trim();
        let urls = if let Some(inner) = value
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
        {
            // 单行数组：按逗号切，逐元素去引号。
            inner
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(unquote)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        } else {
            let u = unquote(value);
            if u.is_empty() {
                Vec::new()
            } else {
                vec![u.to_string()]
            }
        };
        if !urls.is_empty() {
            table.links.insert(name.to_string(), urls);
        }
    }
    table
}

/// 去掉行内注释（`#` 起头到行尾；引号内的 `#` 不切——URL 里可能有 fragment）。
fn strip_toml_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

/// 去掉首尾配对双引号（无引号则原样）。
fn unquote(s: &str) -> &str {
    s.trim()
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or_else(|| s.trim())
}

impl LinkTable {
    /// 查自定义短链（不存在返回 None）。
    pub fn get(&self, name: &str) -> Option<&Vec<String>> {
        self.links.get(name)
    }
}

/// 解析短链 spec：内置名优先，其次用户表，都没有则报错列出可用项。
///
/// `mx:` 后为空（即裸 `mx:`）视为参数错误。
pub fn resolve(spec: &str, user: &LinkTable) -> ProotResult<ResolvedLink> {
    let name = strip_mx(spec).ok_or_else(|| {
        ProotError::InvalidArg(format!("不是短链形式（期望 mx:<名字>）: {}", spec))
    })?;
    if name.is_empty() {
        return Err(ProotError::InvalidArg("短链名为空（mx: 后要跟名字）".into()));
    }
    // 用户自定义优先还是内置优先？——内置优先：用户表里同名会 Shadow
    // 内置发行版，造成「mx:alpine 突然变成用户的私有 rootfs」的意外。
    if let Some(id) = builtin_distro_id(name) {
        return Ok(ResolvedLink::Distro(id.to_string()));
    }
    if let Some(urls) = user.get(name) {
        return Ok(ResolvedLink::Url(urls.clone()));
    }
    let avail = builtin_names().join(" / ");
    Err(ProotError::InvalidArg(format!(
        "未知短链 mx:{}（可用：mx:{}；自定义见 ~/.config/moxsh-proot/links.toml）",
        name, avail
    )))
}

/// 内置短链名列表（错误信息/帮助文本用）。
pub fn builtin_names() -> Vec<&'static str> {
    BUILTIN_LINKS.iter().map(|(n, _)| *n).collect()
}

/// 用户级 `links.toml` 路径：`$MOXSH_CONFIG_DIR/links.toml`，缺省
/// `~/.config/moxsh-proot/links.toml`（XDG 风格，不占 $HOME 根目录）。
pub fn user_links_path() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("MOXSH_CONFIG_DIR") {
        return std::path::PathBuf::from(dir).join("links.toml");
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    std::path::Path::new(&home)
        .join(".config/moxsh-proot")
        .join("links.toml")
}

/// 从用户路径加载短链表（封装默认路径逻辑）。
pub fn load_user_links() -> ProotResult<LinkTable> {
    LinkTable::load(&user_links_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_mx_forms() {
        assert_eq!(strip_mx("mx:alpine"), Some("alpine"));
        assert_eq!(strip_mx("mx:"), Some(""));
        assert_eq!(strip_mx("alpine"), None);
        assert_eq!(strip_mx("/data/rootfs"), None);
        assert!(is_mx("mx:ubuntu"));
        assert!(!is_mx("ubuntu"));
    }

    #[test]
    fn builtin_links_are_four_distros() {
        assert_eq!(BUILTIN_LINKS.len(), 4);
        assert_eq!(builtin_distro_id("alpine"), Some("alpine"));
        assert_eq!(builtin_distro_id("ubuntu"), Some("ubuntu"));
        assert_eq!(builtin_distro_id("debian"), Some("debian"));
        assert_eq!(builtin_distro_id("kali"), Some("kali"));
        assert_eq!(builtin_distro_id("node"), None); // 不造工具链镜像概念
    }

    #[test]
    fn parse_single_and_multi_urls() {
        let t = parse_links_toml(
            r#"
            # 我的私有 rootfs
            [links]
            mydev = "https://example.com/rootfs.tar.xz"
            cnmirror = ["https://mirror.example.com/rootfs.tar.xz", "https://example.com/rootfs.tar.xz"]
            "#,
        );
        assert_eq!(
            t.get("mydev"),
            Some(&vec!["https://example.com/rootfs.tar.xz".to_string()])
        );
        assert_eq!(t.get("cnmirror").map(Vec::len), Some(2));
        assert_eq!(t.get("nope"), None);
    }

    #[test]
    fn parse_ignores_other_sections_and_comments() {
        let t = parse_links_toml(
            r#"
            [settings]
            root_dir = "~/moxsh"   # 不是 [links] 段，忽略
            [links]
            # 注释行
            dev = "https://a/x.tar.xz" # 行尾注释
            [other]
            junk = "https://b/y.tar.xz"
            "#,
        );
        assert_eq!(t.get("dev"), Some(&vec!["https://a/x.tar.xz".to_string()]));
        assert_eq!(t.get("root_dir"), None);
        assert_eq!(t.get("junk"), None);
    }

    #[test]
    fn parse_rejects_bad_names() {
        let t = parse_links_toml(
            r#"
            [links]
            ../evil = "https://a/x.tar.xz"
            ".hidden" = "https://a/y.tar.xz"
            ok = "https://a/z.tar.xz"
            "#,
        );
        assert_eq!(t.get("../evil"), None);
        assert_eq!(t.get(".hidden"), None);
        assert_eq!(t.get("ok"), Some(&vec!["https://a/z.tar.xz".to_string()]));
    }

    #[test]
    fn parse_url_fragment_not_treated_as_comment() {
        let t = parse_links_toml(
            r#"
            [links]
            frag = "https://example.com/rootfs.tar.xz#anchor"
            "#,
        );
        assert_eq!(
            t.get("frag"),
            Some(&vec!["https://example.com/rootfs.tar.xz#anchor".to_string()])
        );
    }

    #[test]
    fn resolve_builtin_before_user_shadow() {
        let user = parse_links_toml(
            r#"
            [links]
            alpine = "https://evil.com/rootfs.tar.xz"
            mydev = "https://example.com/x.tar.xz"
            "#,
        );
        // 内置优先：用户表里的 alpine 是意外 Shadow，不生效。
        assert_eq!(
            resolve("mx:alpine", &user).unwrap(),
            ResolvedLink::Distro("alpine".into())
        );
        assert_eq!(
            resolve("mx:mydev", &user).unwrap(),
            ResolvedLink::Url(vec!["https://example.com/x.tar.xz".to_string()])
        );
    }

    #[test]
    fn resolve_errors_are_actionable() {
        let user = LinkTable::new();
        let e = resolve("mx:node", &user).unwrap_err();
        let msg = e.to_string();
        assert!(msg.contains("mx:node"), "错误信息要带输入: {}", msg);
        assert!(msg.contains("mx:alpine"), "错误信息要列可用项: {}", msg);
        assert!(msg.contains("links.toml"), "错误信息要指自定义路径: {}", msg);

        assert!(resolve("mx:", &user).is_err());
        assert!(resolve("alpine", &user).is_err());
    }

    #[test]
    fn load_missing_file_is_empty_table() {
        let t = LinkTable::load(Path::new("/nonexistent/moxsh/links.toml")).unwrap();
        assert_eq!(t.links.len(), 0);
    }
}
