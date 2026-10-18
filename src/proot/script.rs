//! 引擎内嵌脚本运行时（Python / Lua 双后端，五语言融合的脚本层）。
//!
//! # 分工由来
//!
//! 引擎核心（ptrace / 路径翻译 / 网络栈）用 Rust 写死之后，**供给类**与
//! **配置类**工作交给脚本语言各展所长：
//!
//! - **Python**（`py/runtime/`）：容器内供给/巡检脚本。特长在标准库丰富
//!   （`socket`/`urllib`/`textwrap`），「装完后验证 DNS/hosts/网络边界」
//!   这类数据型任务写起来最快；且脚本**同时在宿主侧可直接运行**——
//!   开发期不用进容器就能自证逻辑（见 `py/verify/`）。
//! - **Lua**（`lua/`）：容器钩子/配置层。体积小、语义稳、无运行时依赖，
//!   适合声明式配置与生命周期钩子。
//!
//! # 引擎职责边界
//!
//! 本模块只做三件事：**探测解释器 → 发现钩子脚本 → 组装 guest 命令**。
//! 真正的执行由引擎 ptrace 层完成（[`crate::proot::cli::run_config`]），
//! 脚本内看到的路径即 guest 路径——脚本作者无需关心路径翻译细节。
//!
//! # 目录约定
//!
//! ```text
//! $MOXSH_ROOT/hooks/          # 钩子目录（安装器从仓库 py/runtime、lua/hooks 种子复制）
//!   post-install.py           # 命名：<kind>.<后缀>，kind 见 HookKind
//!   post-install.lua
//!   pre-login.py
//! ```
//!
//! 钩子在 guest 内可见路径固定为 [`GUEST_HOOK_DIR`]（经 `-b` 绑定进入），
//! 与宿主目录解耦——脚本里写死 `/moxsh/hooks/xxx` 即可跨设备复用。

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use super::{ProotConfig, ProotResult};

/// 钩子目录在 guest 内的固定可见路径（经 `-b` 绑定）。
pub const GUEST_HOOK_DIR: &str = "/moxsh/hooks";

/// 脚本运行时类型（探测顺序即优先级：Python 先于 Lua）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    /// Python 3（供给/巡检脚本）。
    Python,
    /// Lua 5.x（钩子/配置层）。
    Lua,
}

impl Runtime {
    /// guest 内候选解释器路径（按优先级；merged-usr 发行版 `/bin` 是
    /// `usr/bin` 的相对符号链接，规范化后两者等价，仍双查兜底非合并布局）。
    pub fn guest_candidates(&self) -> &'static [&'static str] {
        match self {
            Runtime::Python => &["/usr/bin/python3", "/bin/python3", "/usr/bin/python"],
            Runtime::Lua => &["/usr/bin/lua", "/bin/lua", "/usr/bin/luajit"],
        }
    }

    /// 钩子文件后缀（小写，不含点）。
    pub fn suffix(&self) -> &'static str {
        match self {
            Runtime::Python => "py",
            Runtime::Lua => "lua",
        }
    }

    /// 按后缀反查（未知后缀返回 None）。
    pub fn from_suffix(ext: &str) -> Option<Runtime> {
        match ext.to_ascii_lowercase().as_str() {
            "py" => Some(Runtime::Python),
            "lua" => Some(Runtime::Lua),
            _ => None,
        }
    }

    /// 全部运行时（探测顺序）。
    pub fn all() -> &'static [Runtime] {
        &[Runtime::Python, Runtime::Lua]
    }
}

impl fmt::Display for Runtime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Runtime::Python => write!(f, "python"),
            Runtime::Lua => write!(f, "lua"),
        }
    }
}

/// 钩子生命周期类型（文件名前缀，大小写不敏感）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    /// 安装完成后触发（DNS/hosts 初始化之后）——供给、建用户、预热缓存。
    PostInstall,
    /// 登录 shell 启动前触发——环境体检、motd、网络边界报告。
    PreLogin,
}

impl HookKind {
    /// 文件名 stem 中的标识串（如 `post-install.py`）。
    pub fn as_str(&self) -> &'static str {
        match self {
            HookKind::PostInstall => "post-install",
            HookKind::PreLogin => "pre-login",
        }
    }

    /// 从文件名 stem 解析（允许 `NN-` 数字前缀排序，如 `10-post-install.py`）。
    pub fn from_stem(stem: &str) -> Option<HookKind> {
        let stem = stem.to_ascii_lowercase();
        // 去掉可选数字排序前缀
        let bare = match stem.split_once('-') {
            Some((head, rest)) if !head.is_empty() && head.chars().all(|c| c.is_ascii_digit()) => {
                rest.to_string()
            }
            _ => stem.clone(),
        };
        match bare.as_str() {
            "post-install" => Some(HookKind::PostInstall),
            "pre-login" => Some(HookKind::PreLogin),
            _ => None,
        }
    }

    /// 注入 guest 的环境变量名（MOXSH_HOOK_KIND 的值）。
    pub fn env_value(&self) -> &'static str {
        match self {
            HookKind::PostInstall => "post-install",
            HookKind::PreLogin => "pre-login",
        }
    }
}

/// 钩子宿主目录：`$MOXSH_ROOT/hooks`。
pub fn hooks_dir(root: &Path) -> PathBuf {
    root.join("hooks")
}

/// 探测 rootfs 内可用解释器（按 [`Runtime::all`] 顺序，去重）。
///
/// 纯文件存在性检查：guest 二进制能否真正跑起来由引擎 ptrace 层负责
/// （loader 显式调用等），这里只回答「这个 rootfs 有没有 Python/Lua」。
pub fn probe_interpreters(rootfs: &Path) -> Vec<Runtime> {
    let mut out = Vec::new();
    for rt in Runtime::all() {
        if rt
            .guest_candidates()
            .iter()
            .any(|c| rootfs.join(c.trim_start_matches('/')).is_file())
        {
            out.push(*rt);
        }
    }
    out
}

/// 发现钩子目录下指定生命周期的脚本（按文件名排序，保证执行顺序确定）。
///
/// 返回 `(Runtime, 脚本宿主路径)`；无法识别后缀/kind 的文件静默跳过
/// （钩子目录是用户领地，放 README 之类不应报错）。
pub fn discover_hooks(root: &Path, kind: HookKind) -> Vec<(Runtime, PathBuf)> {
    let dir = hooks_dir(root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut found: Vec<(Runtime, PathBuf)> = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if !path.is_file() {
            continue;
        }
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(rt) = Runtime::from_suffix(ext) else {
            continue;
        };
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if HookKind::from_stem(stem) == Some(kind) {
            found.push((rt, path));
        }
    }
    // 按文件名排序（数字前缀用户约定即执行序）
    found.sort_by(|a, b| a.1.file_name().cmp(&b.1.file_name()));
    found
}

/// 钩子执行环境变量（guest 内可见，脚本据此自适配）。
pub fn hook_env(id: &str, kind: HookKind) -> Vec<(String, String)> {
    vec![
        ("MOXSH_PROOT".to_string(), "1".to_string()),
        ("MOXSH_CONTAINER_ID".to_string(), id.to_string()),
        ("MOXSH_HOOK_KIND".to_string(), kind.env_value().to_string()),
        ("MOXSH_HOOK_DIR".to_string(), GUEST_HOOK_DIR.to_string()),
    ]
}

/// 单个钩子的执行结果。
#[derive(Debug, Clone)]
pub struct HookOutcome {
    /// 脚本文件名（宿主侧 basename）。
    pub script: String,
    /// 运行时（python/lua）。
    pub runtime: Runtime,
    /// 退出码（0 = 成功）。
    pub code: i32,
}

/// 一轮钩子执行的汇总报告。
#[derive(Debug, Clone, Default)]
pub struct HookReport {
    /// 已执行的钩子（含退出码）。
    pub ran: Vec<HookOutcome>,
    /// 发现但因 rootfs 无对应解释器而跳过的脚本名。
    pub skipped: Vec<String>,
}

impl HookReport {
    /// 是否有失败钩子（非 0 退出码）。
    pub fn has_failure(&self) -> bool {
        self.ran.iter().any(|h| h.code != 0)
    }

    /// 一行式人类可读摘要（供安装进度回调/日志）。
    pub fn summary(&self) -> String {
        if self.ran.is_empty() && self.skipped.is_empty() {
            return "无钩子脚本".to_string();
        }
        let mut parts: Vec<String> = self
            .ran
            .iter()
            .map(|h| {
                format!(
                    "{}({}) {}",
                    h.script,
                    h.runtime,
                    if h.code == 0 { "✓" } else { "✗" }
                )
            })
            .collect();
        for s in &self.skipped {
            parts.push(format!("{}(跳过：缺解释器)", s));
        }
        parts.join(" ")
    }
}

/// 执行一轮钩子（引擎内直接跑，走 ptrace 翻译链）。
///
/// - 钩子目录经 `-b` 绑定到 guest 的 [`GUEST_HOOK_DIR`]；
/// - fake root + cwd=/root（供给脚本常要写 `/etc`、`/root` 下的文件）；
/// - rootfs 无对应解释器的脚本进 `skipped`——**优雅降级，不阻断安装**；
/// - 安装类钩子失败不回滚 rootfs（钩子是用户脚本，语义上不属引擎核心），
///   但会进报告由调用方决定是否提示。
///
/// `root` 为 `$MOXSH_ROOT`（钩子目录所在），`rootfs` 为容器 rootfs 宿主路径
/// （解释器探测与 ptrace 目标）。
pub fn run_hooks(root: &Path, rootfs: &Path, id: &str, kind: HookKind) -> ProotResult<HookReport> {
    let hooks = discover_hooks(root, kind);
    if hooks.is_empty() {
        return Ok(HookReport::default());
    }
    let host_hook_dir = hooks_dir(root);
    let available = probe_interpreters(rootfs);
    let mut report = HookReport::default();
    for (rt, script_path) in hooks {
        let script_name = script_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        if !available.contains(&rt) {
            report.skipped.push(script_name);
            continue;
        }
        // guest 命令：解释器（guest 路径，引擎解析 loader）+ 脚本（绑定目录内）
        let Some(interp) = rt
            .guest_candidates()
            .iter()
            .find(|c| rootfs.join(c.trim_start_matches('/')).is_file())
        else {
            report.skipped.push(script_name);
            continue;
        };
        let guest_script = format!("{}/{}", GUEST_HOOK_DIR, script_name);
        let command = vec![(*interp).to_string(), guest_script];
        let mut cfg = ProotConfig::new(rootfs);
        cfg.fake_root = true;
        cfg.cwd = "/root".to_string();
        cfg.bindings
            .push((GUEST_HOOK_DIR.to_string(), host_hook_dir.display().to_string()));
        cfg.env = hook_env(id, kind);
        let env_kv: Vec<String> = cfg.env.iter().map(|(k, v)| format!("{}={}", k, v)).collect();
        let code = crate::proot::cli::run_config(&cfg, &command, &env_kv);
        report.ran.push(HookOutcome {
            script: script_name,
            runtime: rt,
            code,
        });
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn hook_kind_parsing() {
        assert_eq!(HookKind::from_stem("post-install"), Some(HookKind::PostInstall));
        assert_eq!(HookKind::from_stem("pre-login"), Some(HookKind::PreLogin));
        // 数字排序前缀
        assert_eq!(HookKind::from_stem("10-post-install"), Some(HookKind::PostInstall));
        assert_eq!(HookKind::from_stem("20-pre-login"), Some(HookKind::PreLogin));
        // 大小写不敏感
        assert_eq!(HookKind::from_stem("Post-Install"), Some(HookKind::PostInstall));
        // 无关文件
        assert_eq!(HookKind::from_stem("README"), None);
        assert_eq!(HookKind::from_stem("post-install-bak"), None);
    }

    #[test]
    fn runtime_suffix_roundtrip() {
        assert_eq!(Runtime::from_suffix("py"), Some(Runtime::Python));
        assert_eq!(Runtime::from_suffix("PY"), Some(Runtime::Python));
        assert_eq!(Runtime::from_suffix("lua"), Some(Runtime::Lua));
        assert_eq!(Runtime::from_suffix("txt"), None);
        assert_eq!(Runtime::Python.suffix(), "py");
        assert_eq!(Runtime::Lua.suffix(), "lua");
    }

    #[test]
    fn probe_interpreters_real_fs() {
        let tmp = std::env::temp_dir().join("moxsh-proot-probe-test");
        let _ = fs::remove_dir_all(&tmp);
        let rootfs = tmp.join("rootfs");
        // 空 rootfs：无解释器
        assert!(probe_interpreters(&rootfs).is_empty());
        // 造 python3（merged-usr 布局）
        touch(&rootfs.join("usr/bin/python3"));
        assert_eq!(probe_interpreters(&rootfs), vec![Runtime::Python]);
        // 再造 lua（/bin 非合并布局兜底）
        touch(&rootfs.join("bin/lua"));
        assert_eq!(probe_interpreters(&rootfs), vec![Runtime::Python, Runtime::Lua]);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_hooks_real_fs_ordering() {
        let tmp = std::env::temp_dir().join("moxsh-proot-hooks-test");
        let _ = fs::remove_dir_all(&tmp);
        let dir = hooks_dir(&tmp);
        fs::create_dir_all(&dir).unwrap();
        // 同类两个脚本 + 异类一个 + 干扰文件
        fs::write(dir.join("20-post-install.py"), b"# py\n").unwrap();
        fs::write(dir.join("10-post-install.lua"), b"-- lua\n").unwrap();
        fs::write(dir.join("pre-login.py"), b"# py\n").unwrap();
        fs::write(dir.join("README.md"), b"hi\n").unwrap();
        fs::write(dir.join("post-install.txt"), b"nope\n").unwrap();

        let post = discover_hooks(&tmp, HookKind::PostInstall);
        assert_eq!(post.len(), 2);
        // 排序：10-post-install.lua 先于 20-post-install.py
        assert_eq!(post[0].0, Runtime::Lua);
        assert_eq!(post[1].0, Runtime::Python);

        let pre = discover_hooks(&tmp, HookKind::PreLogin);
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0].0, Runtime::Python);

        // 空目录 / 不存在目录：零钩子不报错
        let empty = std::env::temp_dir().join("moxsh-proot-hooks-empty");
        let _ = fs::remove_dir_all(&empty);
        assert!(discover_hooks(&empty, HookKind::PostInstall).is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn hook_env_contract() {
        let env = hook_env("ubuntu", HookKind::PostInstall);
        let get = |k: &str| env.iter().find(|(ek, _)| ek == k).map(|(_, v)| v.clone());
        assert_eq!(get("MOXSH_PROOT").as_deref(), Some("1"));
        assert_eq!(get("MOXSH_CONTAINER_ID").as_deref(), Some("ubuntu"));
        assert_eq!(get("MOXSH_HOOK_KIND").as_deref(), Some("post-install"));
        assert_eq!(get("MOXSH_HOOK_DIR").as_deref(), Some(GUEST_HOOK_DIR));
    }

    #[test]
    fn report_summary_and_failure() {
        let mut r = HookReport::default();
        assert_eq!(r.summary(), "无钩子脚本");
        r.ran.push(HookOutcome {
            script: "post-install.py".into(),
            runtime: Runtime::Python,
            code: 0,
        });
        r.skipped.push("post-install.lua".into());
        let s = r.summary();
        assert!(s.contains("post-install.py(python) ✓"), "{}", s);
        assert!(s.contains("post-install.lua(跳过：缺解释器)"), "{}", s);
        assert!(!r.has_failure());
        r.ran.push(HookOutcome {
            script: "pre-login.py".into(),
            runtime: Runtime::Python,
            code: 1,
        });
        assert!(r.has_failure());
    }
}
