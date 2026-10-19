//! moxsh 用户配置（0.2.0）：`config.toml` 极简子集解析。
//!
//! 配置面（`docs/roadmap.md` §1/§5，均经项目主人确认）：
//!
//! - **数据根三层**：`MOXSH_ROOT` 环境变量 > `config.toml` 的
//!   `[general] root_dir` > 默认 `~/.moxsh`；
//! - **每容器进/退命令**：全局默认 + 每容器覆盖（`[containers.<id>]` 段）。
//!   「以防误触」语义：`enter` = 进容器时在容器内执行的**首个命令**
//!   （替代默认登录 shell，可自带确认逻辑，如
//!   `sh -c 'read -p "确认进入？"' && /bin/bash -l`）；`exit` = 会话结束后
//!   **宿主侧**收尾命令（容器内 exit 由 shell 自身处理，引擎不拦 exit
//!   syscall，故 exit 语义降级为收尾提示/清理，文档明示）。
//!
//! config.toml 与 links.toml 同目录：`~/.config/moxsh-proot/config.toml`
//! （`MOXSH_CONFIG_DIR` 可覆盖目录）。
//!
//! TOML 解析为手写极简子集（依赖极简约束，见 Cargo.toml 与
//! [`crate::proot::links::parse_links_toml`] 注释），只认下面三种段。

use std::path::{Path, PathBuf};

use crate::proot::{ProotError, ProotResult};

/// 默认数据根目录名（`$HOME/.moxsh`）。
pub const DEFAULT_ROOT_DIR_NAME: &str = ".moxsh";

/// 用户配置（config.toml 解析结果）。
#[derive(Debug, Clone, Default)]
pub struct Settings {
    /// `[general] root_dir`（未展开 `~`）。
    root_dir: Option<String>,
    /// `[enter] default`：全局默认进入命令。
    default_enter: Option<String>,
    /// `[exit] default`：全局默认收尾命令。
    default_exit: Option<String>,
    /// `[containers.<id>] enter/exit`：每容器覆盖。
    container_enter: Vec<(String, String)>,
    container_exit: Vec<(String, String)>,
}

impl Settings {
    /// 从文件加载（不存在 = 空配置，不是错误）。
    pub fn load(path: &Path) -> ProotResult<Settings> {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Settings::default())
            }
            Err(e) => return Err(ProotError::Io(e)),
        };
        Ok(parse_config_toml(&text))
    }

    /// 用户级 `config.toml` 路径。
    pub fn user_path() -> PathBuf {
        config_dir().join("config.toml")
    }

    /// 从默认路径加载。
    pub fn load_user() -> ProotResult<Settings> {
        Settings::load(&Self::user_path())
    }

    /// `[general] root_dir`（`~` 已展开；未配置返回 None）。
    pub fn root_dir(&self) -> Option<PathBuf> {
        self.root_dir.as_deref().map(expand_tilde)
    }

    /// 解析数据根目录（三层优先级，roadmap §5）：
    /// `MOXSH_ROOT` 环境变量 > `config.toml` > `~/.moxsh`。
    pub fn resolve_root_dir(&self) -> PathBuf {
        if let Some(r) = std::env::var_os("MOXSH_ROOT") {
            if !r.is_empty() {
                return PathBuf::from(r);
            }
        }
        if let Some(d) = self.root_dir() {
            return d;
        }
        default_root_dir()
    }

    /// 进入某容器时容器内执行的首个命令（每容器覆盖 > 全局默认）。
    pub fn enter_cmd(&self, id: &str) -> Option<&str> {
        self.container_enter
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
            .or(self.default_enter.as_deref())
    }

    /// 会话结束后的宿主侧收尾命令（每容器覆盖 > 全局默认）。
    pub fn exit_cmd(&self, id: &str) -> Option<&str> {
        self.container_exit
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, v)| v.as_str())
            .or(self.default_exit.as_deref())
    }
}

/// 默认数据根：`$HOME/.moxsh`。
pub fn default_root_dir() -> PathBuf {
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(DEFAULT_ROOT_DIR_NAME)
}

/// 配置目录：`MOXSH_CONFIG_DIR` > `~/.config/moxsh-proot`。
pub fn config_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MOXSH_CONFIG_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(".config/moxsh-proot")
}

/// 展开 `~`/`~/` 前缀为 `$HOME`（其余原样）。
pub fn expand_tilde(p: &str) -> PathBuf {
    if p == "~" {
        return default_root_dir().parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("/"));
    }
    if let Some(rest) = p.strip_prefix("~/") {
        let home = std::env::var_os("HOME").unwrap_or_default();
        return Path::new(&home).join(rest);
    }
    PathBuf::from(p)
}

/// 解析 `config.toml` 极简子集。
///
/// 认得三种段：`[general]`（root_dir）、`[enter]`/`[exit]`（default）、
/// `[containers.<id>]`（enter/exit）。其余段忽略；段外行忽略；
/// `#` 注释（引号内不切，复用 links.rs 同款思路的手写实现）。
pub fn parse_config_toml(text: &str) -> Settings {
    let mut s = Settings::default();
    // 当前段上下文：None = 段外；General；EnterDefault；ExitDefault；
    // Container(id)。
    #[derive(PartialEq)]
    enum Section {
        None,
        General,
        Enter,
        Exit,
        Container(String),
    }
    let mut section = Section::None;
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            let inner = line.trim_start_matches('[').trim_end_matches(']');
            section = match inner {
                "general" => Section::General,
                "enter" => Section::Enter,
                "exit" => Section::Exit,
                other => {
                    if let Some(id) = other.strip_prefix("containers.") {
                        Section::Container(id.trim().to_string())
                    } else {
                        Section::None
                    }
                }
            };
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        let val = unquote(v.trim()).to_string();
        if val.is_empty() {
            continue;
        }
        match &section {
            Section::General if key == "root_dir" => s.root_dir = Some(val),
            Section::Enter if key == "default" => s.default_enter = Some(val),
            Section::Exit if key == "default" => s.default_exit = Some(val),
            Section::Container(id) if key == "enter" => {
                s.container_enter.push((id.clone(), val))
            }
            Section::Container(id) if key == "exit" => {
                s.container_exit.push((id.clone(), val))
            }
            _ => {}
        }
    }
    s
}

/// 在 config.toml 文本里 upsert `[general] root_dir`（保留其余一切内容）。
///
/// 规则：
/// - `[general]` 段内已有 `root_dir` 行 → 替换（重复行去重留一）；
/// - `[general]` 段内没有 → 段尾补一行；
/// - 全文没有 `[general]` 段 → 文末追加一段；
/// - 其余行原样保留（含注释与未知段——手写 TOML 子集的写侧对称）。
pub fn upsert_general_root_dir(text: &str, root: &str) -> String {
    let new_line = format!("root_dir = \"{}\"", root);
    let mut out: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut done = false;
    for line in text.lines() {
        let t = strip_comment(line).trim();
        if let Some(inner) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // 离开 general 段时还没写 → 在段尾补一行
            if section == "general" && !done {
                out.push(new_line.clone());
                done = true;
            }
            section = inner.trim().to_string();
            out.push(line.to_string());
            continue;
        }
        if section == "general" {
            if let Some((k, _)) = t.split_once('=') {
                if k.trim() == "root_dir" {
                    if !done {
                        out.push(new_line.clone());
                        done = true;
                    }
                    continue; // 替换旧行（含重复项去重）
                }
            }
        }
        out.push(line.to_string());
    }
    if !done {
        if section == "general" {
            out.push(new_line); // general 恰是最后一段
        } else {
            if !out.is_empty() {
                out.push(String::new());
            }
            out.push("[general]".to_string());
            out.push(new_line);
        }
    }
    let mut result = out.join("\n");
    result.push('\n');
    result
}

/// 行内注释剥离（引号内 `#` 不切）。
fn strip_comment(line: &str) -> &str {
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

/// 去首尾配对双引号。
fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_general_and_defaults() {
        let s = parse_config_toml(
            r#"
            [general]
            root_dir = "~/moxsh-data"
            [enter]
            default = "read -p 确认？ && /bin/bash -l"
            [exit]
            default = "echo 已退出容器"
            "#,
        );
        assert_eq!(s.root_dir.as_deref(), Some("~/moxsh-data"));
        assert_eq!(s.default_enter.as_deref(), Some("read -p 确认？ && /bin/bash -l"));
        assert_eq!(s.default_exit.as_deref(), Some("echo 已退出容器"));
    }

    #[test]
    fn parse_container_overrides() {
        let s = parse_config_toml(
            r#"
            [enter]
            default = "/bin/bash -l"
            [containers.alpine]
            enter = "read -p 进入 alpine？ && /bin/sh -l"
            exit = "echo bye alpine"
            [containers.kali]
            enter = "/bin/bash -l"
            "#,
        );
        assert_eq!(
            s.enter_cmd("alpine"),
            Some("read -p 进入 alpine？ && /bin/sh -l")
        );
        assert_eq!(s.enter_cmd("kali"), Some("/bin/bash -l"));
        // 未覆盖的回落全局默认
        assert_eq!(s.enter_cmd("ubuntu"), Some("/bin/bash -l"));
        assert_eq!(s.exit_cmd("alpine"), Some("echo bye alpine"));
        assert_eq!(s.exit_cmd("kali"), None);
        assert_eq!(s.exit_cmd("ubuntu"), None);
    }

    #[test]
    fn parse_ignores_unknown_sections() {
        let s = parse_config_toml(
            r#"
            [settings]
            junk = "x"
            root_dir = "应该被忽略"
            [general]
            root_dir = "/data/moxsh"
            [other]
            enter = "y"
            "#,
        );
        assert_eq!(s.root_dir.as_deref(), Some("/data/moxsh"));
        assert_eq!(s.default_enter, None);
    }

    #[test]
    fn tilde_expansion() {
        let home = std::env::var_os("HOME").unwrap_or_default();
        assert_eq!(expand_tilde("~/moxsh"), Path::new(&home).join("moxsh"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
        assert_eq!(expand_tilde("relative"), PathBuf::from("relative"));
    }

    #[test]
    fn load_missing_file_is_default() {
        let s = Settings::load(Path::new("/nonexistent/moxsh/config.toml")).unwrap();
        assert_eq!(s.root_dir, None);
        assert_eq!(s.enter_cmd("alpine"), None);
    }

    #[test]
    fn root_dir_priority_documented() {
        // env 优先于配置文件（不设 env 时走配置→默认）。
        let s = parse_config_toml("[general]\nroot_dir = \"/from/config\"\n");
        // 注意：测试环境若设了 MOXSH_ROOT 会拿到 env 值——这里只验证配置层。
        assert_eq!(s.root_dir(), Some(PathBuf::from("/from/config")));
        let empty = Settings::default();
        assert_eq!(empty.root_dir(), None);
        // 默认值形状（HOME 可能未设，只验证不以 panic 返回）。
        let _ = default_root_dir();
    }

    #[test]
    fn upsert_root_dir_empty_text_appends_section() {
        let got = upsert_general_root_dir("", "/data/moxsh");
        assert_eq!(got, "[general]\nroot_dir = \"/data/moxsh\"\n");
        assert_eq!(
            parse_config_toml(&got).root_dir.as_deref(),
            Some("/data/moxsh")
        );
    }

    #[test]
    fn upsert_root_dir_replaces_and_dedups() {
        let text = "# 我的配置\n[general]\nroot_dir = \"/old\"\nroot_dir = \"/dup\"\n[enter]\ndefault = \"echo hi\"\n";
        let got = upsert_general_root_dir(text, "/new");
        // 旧行被替换、重复行去重、其余内容原样。
        assert_eq!(
            got,
            "# 我的配置\n[general]\nroot_dir = \"/new\"\n[enter]\ndefault = \"echo hi\"\n"
        );
        assert_eq!(parse_config_toml(&got).root_dir.as_deref(), Some("/new"));
        assert_eq!(
            parse_config_toml(&got).default_enter.as_deref(),
            Some("echo hi")
        );
    }

    #[test]
    fn upsert_root_dir_inserts_at_section_end() {
        // general 在中间：root_dir 补在段尾（[enter] 之前），不串段。
        let text = "[general]\n# 没有 root_dir\n[enter]\ndefault = \"x\"\n";
        let got = upsert_general_root_dir(text, "~/moxsh-data");
        assert_eq!(
            got,
            "[general]\n# 没有 root_dir\nroot_dir = \"~/moxsh-data\"\n[enter]\ndefault = \"x\"\n"
        );
        assert_eq!(
            parse_config_toml(&got).root_dir.as_deref(),
            Some("~/moxsh-data")
        );
    }

    #[test]
    fn upsert_root_dir_general_is_last_section() {
        let text = "[enter]\ndefault = \"x\"\n[general]\n";
        let got = upsert_general_root_dir(text, "/data/moxsh");
        assert_eq!(
            got,
            "[enter]\ndefault = \"x\"\n[general]\nroot_dir = \"/data/moxsh\"\n"
        );
    }
}
