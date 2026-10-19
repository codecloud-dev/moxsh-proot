//! 落盘启动器 + shell 函数注入（0.2.0，roadmap §1）。
//!
//! 「安装了 kali，输入 `kali` 就进容器」的两层落地（项目主人定：
//! 两层都做，函数优先、命令兜底）：
//!
//! 1. **落盘启动器**：install 后在 bin 目录写一个 `<id>` 可执行脚本，
//!    `exec moxsh-proot-distro launch mx:<id> "$@"`。不加载 rc 的环境
//!    （cron、部分 IDE 终端）也能用；
//! 2. **shell 函数注入**：往 `.bashrc`/`.zshrc` 追加一段带标记的函数块
//!    （`alpine() { moxsh-proot-distro launch mx:alpine "$@"; }`），
//!    函数优先于 PATH 查找，且便于将来扩展（如 `--ro` 参数透传）。
//!
//! 幂等性：函数块用 `# >>> moxsh-proot >>>` / `# <<< moxsh-proot <<<`
//! 标记包裹，重复注入先删旧块再写新块（容器增减后刷新）。

use std::path::{Path, PathBuf};

use crate::proot::ProotResult;

/// shell 函数块的起止标记（幂等识别）。
pub const BLOCK_BEGIN: &str = "# >>> moxsh-proot (自动生成) >>>";
pub const BLOCK_END: &str = "# <<< moxsh-proot <<<";

/// 启动器 bin 目录：`$PREFIX/bin`（Termux 安装器落点）优先，
/// 其次 `~/.local/bin`（桌面 Linux 惯例；不存在则创建）。
pub fn default_bin_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("PREFIX") {
        if !p.is_empty() {
            return Path::new(&p).join("bin");
        }
    }
    let home = std::env::var_os("HOME").unwrap_or_default();
    Path::new(&home).join(".local/bin")
}

/// 写单个启动器 `bin_dir/<name>`（`exec moxsh-proot-distro launch mx:<name> "$@"`）。
///
/// `name` 为短链名（内置四大发行版 id 或 links.toml 自定义名）；
/// 由调用方保证已过 [`crate::proot::distro::DistroManager::valid_id`]
/// 白名单（install 路径的 id 必然已校验）。
pub fn write_launcher(bin_dir: &Path, name: &str) -> ProotResult<PathBuf> {
    std::fs::create_dir_all(bin_dir)?;
    let path = bin_dir.join(name);
    let script = format!(
        "#!/bin/sh\n\
         # moxsh-proot 启动器（安装时自动生成）。\n\
         # 删除容器（moxsh-proot-distro remove {name}）后可随手删掉本文件。\n\
         exec moxsh-proot-distro launch mx:{name} \"$@\"\n",
        name = name
    );
    std::fs::write(&path, script)?;
    // 0755：类 Unix 直接生效；Android 的 W^X 只拦 system 分区，App 私有
    // 目录与 Termux PREFIX 均可执行（与安装器 install -m 0755 同语义）。
    set_exec_bit(&path)?;
    Ok(path)
}

/// 生成 shell 函数块（含起止标记）。
pub fn function_block(names: &[String]) -> String {
    let mut out = String::new();
    out.push_str(BLOCK_BEGIN);
    out.push('\n');
    for n in names {
        out.push_str(&format!(
            "{n}() {{ command moxsh-proot-distro launch mx:{n} \"$@\"; }}\n",
            n = n
        ));
    }
    out.push_str(BLOCK_END);
    out.push('\n');
    out
}

/// 幂等注入：rc 文件里已有标记块则整块替换，否则追加到末尾。
///
/// `rc_path` 不存在则创建。返回是否有写入。
pub fn inject_shell_functions(rc_path: &Path, names: &[String]) -> ProotResult<bool> {
    if names.is_empty() {
        return Ok(false);
    }
    let block = function_block(names);
    let existing = std::fs::read_to_string(rc_path).unwrap_or_default();
    let merged = if let (Some(b), Some(e)) = (existing.find(BLOCK_BEGIN), existing.find(BLOCK_END)) {
        if b < e {
            let mut s = String::with_capacity(existing.len() + block.len());
            s.push_str(&existing[..b]);
            s.push_str(&block);
            // 旧块尾部换行一并吃掉（block 自带收尾换行）——否则每次
            // 刷新都多一个空行，破坏幂等。
            let tail = &existing[e + BLOCK_END.len()..];
            s.push_str(tail.strip_prefix('\n').unwrap_or(tail));
            s
        } else {
            // 标记乱序（用户手改坏了）：不猜，直接追加新块。
            format!("{}{}", existing, block)
        }
    } else {
        // 无标记块：追加（保证前文换行结尾）。
        if existing.is_empty() || existing.ends_with('\n') {
            format!("{}{}", existing, block)
        } else {
            format!("{}\n{}", existing, block)
        }
    };
    if merged == existing {
        return Ok(false);
    }
    if let Some(parent) = rc_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(rc_path, merged)?;
    Ok(true)
}

/// 收集需要注入的 rc 文件：`.bashrc` + `.zshrc`（存在或 HOME 可写即写；
/// 不存在的也写——用户随后装 zsh 也能用上）。
pub fn shell_rc_paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").unwrap_or_default();
    let home = Path::new(&home);
    vec![home.join(".bashrc"), home.join(".zshrc")]
}

/// Unix 置可执行位（0o755）。
fn set_exec_bit(path: &Path) -> ProotResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = std::fs::metadata(path)?.permissions();
    perm.set_mode(0o755);
    std::fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moxsh-launcher-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn write_launcher_content_and_exec_bit() {
        let dir = tmpdir("write");
        let p = write_launcher(&dir, "alpine").unwrap();
        assert_eq!(p, dir.join("alpine"));
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("#!/bin/sh"), "启动器要可执行: {}", text);
        assert!(text.contains("moxsh-proot-distro launch mx:alpine"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o755, "启动器要 0755");
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_is_idempotent_and_replaces_stale_block() {
        let dir = tmpdir("inject");
        let rc = dir.join(".bashrc");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&rc, "export PS1='x'\n").unwrap();

        assert!(inject_shell_functions(&rc, &["alpine".to_string()]).unwrap());
        let after_first = std::fs::read_to_string(&rc).unwrap();
        assert!(after_first.contains("alpine() { command moxsh-proot-distro launch mx:alpine"));
        assert!(after_first.contains("export PS1='x'"), "原内容要保留");

        // 再注入同一批：无变化（幂等）。
        assert!(!inject_shell_functions(&rc, &["alpine".to_string()]).unwrap());

        // 容器增减后刷新：旧块被整块替换，不残留。
        assert!(inject_shell_functions(
            &rc,
            &["alpine".to_string(), "kali".to_string()]
        )
        .unwrap());
        let after_refresh = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(
            after_refresh.matches(BLOCK_BEGIN).count(),
            1,
            "标记块只能有一份: {}",
            after_refresh
        );
        assert!(after_refresh.contains("kali() {"));
        assert!(after_refresh.matches("export PS1='x'").count() == 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inject_creates_missing_rc() {
        let dir = tmpdir("missing");
        let rc = dir.join("sub/.zshrc");
        assert!(inject_shell_functions(&rc, &["ubuntu".to_string()]).unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.starts_with(BLOCK_BEGIN));
        assert!(text.contains("ubuntu() {"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_names_no_write() {
        let dir = tmpdir("empty");
        let rc = dir.join(".bashrc");
        assert!(!inject_shell_functions(&rc, &[]).unwrap());
        assert!(!rc.exists(), "空名单不该创建文件");
    }
}
