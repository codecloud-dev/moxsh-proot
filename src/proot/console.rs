//! 容器内控制台（0.2.0，roadmap §3：容器内执行管理控制台的短链）。
//!
//! 形态：容器内**固定命令 `mox`**（不开放自定义——与进/退命令的可定制
//! 语义区分，roadmap §3 明确「进入控制台的不给自定义」），拉起宿主侧
//! 管理程序（当前 = `moxsh-proot-distro`，0.4.0 起接 TUI 控制台）。
//!
//! 反穿透机制（tmoe/社区范式 + 引擎适配）：
//!
//! 1. 启动时把宿主 bin 目录 bind 进容器固定挂载点 `/moxsh-host`，
//!    `MOXSH_ROOT` bind 到 `/moxsh-root`；
//! 2. env 注入 `MOXSH_ROOT=/moxsh-root`、PATH 前置 `/moxsh-host`，
//!    并打标 `MOXSH_CONTAINER=1`（钩子/脚本可判断「已在容器内」）；
//! 3. `mox` 脚本先 `unset LD_PRELOAD`（Termux-exec 与 proot 冲突），
//!    再 exec 同目录的 `moxsh-proot-distro`——它看到的 `/moxsh-root`
//!    经 proot 自动翻译回宿主真实目录，管理操作天然穿透。这就是
//!    「反穿透」：**不绕过翻译，让翻译为我们工作**。
//!
//! 嵌套保护：ptrace 不允许 proot 套 proot，脚本检测到 `MOXSH_CONTAINER=1`
//! 时的 login/shell/launch 直接劝退（管理类子命令照常）。

use std::fs;
use std::path::{Path, PathBuf};

use super::launcher::default_bin_dir;
use super::{DEFAULT_CONTAINER_PATH, ProotConfig, ProotResult};

/// 容器内固定命令名（控制台入口，**不开放自定义**）。
pub const CONSOLE_CMD: &str = "mox";

/// 宿主 bin 目录在容器内的挂载点（容器内 `mox` 的所在目录）。
pub const GUEST_HOST_MOUNT: &str = "/moxsh-host";

/// MOXSH_ROOT 在容器内的挂载点。
pub const GUEST_ROOT_MOUNT: &str = "/moxsh-root";

/// 「当前进程在容器内」标记（bind_console 注入；mox 脚本据此劝退嵌套）。
pub const CONTAINER_MARK: &str = "MOXSH_CONTAINER";

/// 控制台总开关：`MOXSH_PROOT_CONSOLE=0` 关闭全部注入（老用户回归）。
pub const CONSOLE_DISABLE_ENV: &str = "MOXSH_PROOT_CONSOLE";

/// mox 脚本内容（写在宿主 bin 目录，随 bind-mount 进容器）。
///
/// 宿主侧直接跑（PATH 里的 `mox`）= distro CLI 的别名；容器内跑 =
/// 反穿透到宿主真实 MOXSH_ROOT 的管理控制台。
pub const CONSOLE_SCRIPT: &str = r#"#!/bin/sh
# moxsh 容器内控制台（固定命令 mox，不开放自定义——roadmap §3）。
# 由引擎/安装器自动生成；删除本文件不影响宿主侧其他命令。
unset LD_PRELOAD
if [ -n "${MOXSH_CONTAINER:-}" ]; then
  # 容器内：反穿透到宿主真实 MOXSH_ROOT（bind-mount 挂载点）。
  case "${1:-}" in
    login|shell|launch)
      echo "mox: 已在容器内，不能嵌套进入其他容器（ptrace 不支持套娃）。" >&2
      echo "mox: 请先退出当前容器，在宿主侧执行：mox $*" >&2
      exit 1
      ;;
  esac
  ROOT=/moxsh-root
else
  # 宿主侧直接跑：普通 distro CLI 包装。
  ROOT="${MOXSH_ROOT:-$HOME/.moxsh}"
fi
exec env MOXSH_ROOT="$ROOT" "$(dirname "$0")/moxsh-proot-distro" "$@"
"#;

/// 宿主 bin 目录（与落盘启动器同一落点：`$PREFIX/bin` > `~/.local/bin`）。
pub fn host_bin_dir() -> PathBuf {
    default_bin_dir()
}

/// 写 mox 脚本到 bin 目录（幂等；目录不存在则创建）。
pub fn write_console_script(bin_dir: &Path) -> ProotResult<PathBuf> {
    fs::create_dir_all(bin_dir)?;
    let path = bin_dir.join(CONSOLE_CMD);
    fs::write(&path, CONSOLE_SCRIPT)?;
    set_exec_bit(&path)?;
    Ok(path)
}

/// 控制台是否启用（`MOXSH_PROOT_CONSOLE=0` 关闭；默认开）。
pub fn console_enabled() -> bool {
    std::env::var(CONSOLE_DISABLE_ENV).as_deref() != Ok("0")
}

/// 给会话配置追加控制台绑定 + env 注入（roadmap §3 的运行时半边）。
///
/// `bin_dir`：宿主 bin 目录（[`host_bin_dir`] 的返回值；显式传入便于测试）。
///
/// - 绑定：`<bin_dir> → /moxsh-host`、`<root> → /moxsh-root`；
///   bin 目录缺失或没有 mox 脚本时跳过 host 绑定（老安装无伤），
///   MOXSH_ROOT 绑定照常（排查/排障时容器内看得见其他 rootfs）；
/// - env：`MOXSH_ROOT=/moxsh-root`、`MOXSH_CONTAINER=1`、PATH 前置
///   `/moxsh-host`（用户显式 `-e` 给的 PATH 只前置不覆盖）。
pub fn bind_console(cfg: &mut ProotConfig, root: &Path, bin_dir: &Path) {
    if !console_enabled() {
        return;
    }
    if bin_dir.join(CONSOLE_CMD).is_file() {
        cfg.bindings
            .push((GUEST_HOST_MOUNT.to_string(), bin_dir.display().to_string()));
    }
    cfg.bindings
        .push((GUEST_ROOT_MOUNT.to_string(), root.display().to_string()));

    // env 注入（后写覆盖先写 = 控制台语义优先；用户 -e 的 PATH 除外）。
    cfg.env.push(("MOXSH_ROOT".to_string(), GUEST_ROOT_MOUNT.to_string()));
    cfg.env
        .push((CONTAINER_MARK.to_string(), "1".to_string()));
    // PATH 前置 /moxsh-host：有则前置，无则垫一份含挂载点的默认 PATH。
    if let Some(slot) = cfg.env.iter_mut().find(|(k, _)| k == "PATH") {
        if !slot.1.contains(GUEST_HOST_MOUNT) {
            slot.1 = format!("{}:{}", GUEST_HOST_MOUNT, slot.1);
        }
    } else {
        cfg.env.push((
            "PATH".to_string(),
            format!("{}:{}", GUEST_HOST_MOUNT, DEFAULT_CONTAINER_PATH),
        ));
    }
}

fn set_exec_bit(path: &Path) -> ProotResult<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perm = fs::metadata(path)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(path, perm)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moxsh-console-test-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn console_script_content_contract() {
        // 脚本必须：unset LD_PRELOAD、嵌套劝退、exec distro、挂载点路径。
        assert!(CONSOLE_SCRIPT.contains("unset LD_PRELOAD"));
        assert!(CONSOLE_SCRIPT.contains("MOXSH_CONTAINER"));
        assert!(CONSOLE_SCRIPT.contains("/moxsh-root"));
        assert!(CONSOLE_SCRIPT.contains("moxsh-proot-distro"));
        // 固定命令名，不开放自定义。
        assert_eq!(CONSOLE_CMD, "mox");
    }

    #[test]
    fn write_console_script_idempotent_and_exec() {
        let dir = tmpdir("write");
        let p = write_console_script(&dir).unwrap();
        assert_eq!(p, dir.join("mox"));
        // 幂等：再写一次不炸。
        write_console_script(&dir).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.starts_with("#!/bin/sh"));
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&p).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bind_console_adds_bindings_and_env() {
        let dir = tmpdir("bind");
        let bin = dir.join("bin");
        let root = dir.join("moxsh");
        fs::create_dir_all(&bin).unwrap();
        fs::create_dir_all(&root).unwrap();
        write_console_script(&bin).unwrap();

        // console_enabled 默认开（CI 上无 MOXSH_PROOT_CONSOLE=0）。
        if !console_enabled() {
            return;
        }
        let mut cfg = ProotConfig::new(&root);
        bind_console(&mut cfg, &root, &bin);

        // 绑定两条：host mount + root mount。
        assert!(cfg
            .bindings
            .iter()
            .any(|(g, h)| g == GUEST_HOST_MOUNT && h == &bin.display().to_string()));
        assert!(cfg
            .bindings
            .iter()
            .any(|(g, h)| g == GUEST_ROOT_MOUNT && h == &root.display().to_string()));

        // env：MOXSH_ROOT 指挂载点、MOXSH_CONTAINER=1、PATH 前置挂载点。
        let get = |k: &str| {
            cfg.env
                .iter()
                .find(|(ek, _)| ek == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("MOXSH_ROOT").as_deref(), Some(GUEST_ROOT_MOUNT));
        assert_eq!(get("MOXSH_CONTAINER").as_deref(), Some("1"));
        let path = get("PATH").expect("PATH 应被注入");
        assert!(
            path.starts_with(&format!("{}:", GUEST_HOST_MOUNT)),
            "PATH 应前置挂载点: {}",
            path
        );
        assert!(path.contains("/usr/bin"), "PATH 应含默认路径: {}", path);

        // 用户显式 PATH 只前置不覆盖。
        let mut cfg2 = ProotConfig::new(&root);
        cfg2.env.push(("PATH".to_string(), "/bin".to_string()));
        bind_console(&mut cfg2, &root, &bin);
        assert_eq!(
            cfg2.env.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.as_str()),
            Some("/moxsh-host:/bin")
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn bind_console_skips_host_bind_without_script() {
        let dir = tmpdir("noscript");
        let root = dir.join("moxsh");
        let bin = dir.join("bin"); // 存在但空的 bin 目录（无 mox 脚本）
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&bin).unwrap();
        if !console_enabled() {
            return;
        }
        let mut cfg = ProotConfig::new(&root);
        bind_console(&mut cfg, &root, &bin);
        // 没有 mox 脚本：不绑 host 目录，但 MOXSH_ROOT 绑定照常。
        assert!(!cfg.bindings.iter().any(|(g, _)| g == GUEST_HOST_MOUNT));
        assert!(cfg.bindings.iter().any(|(g, _)| g == GUEST_ROOT_MOUNT));
        let _ = fs::remove_dir_all(&dir);
    }
}
