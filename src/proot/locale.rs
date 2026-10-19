//! 容器内环境注入（0.2.0，docs/roadmap.md §2：中文乱码根治）。
//!
//! 背景：`proot-distro login` 走 `env -i` 清空继承环境，早期只注入
//! HOME/PATH——TERM 与 LANG 双丢就是「容器内中文乱码 / TUI 降级」的
//! 根因（宿主的 locale 设置根本没机会进容器）。
//!
//! 两层机制：
//!
//! 1. **引擎默认 + 宿主透传**（见 [`super::ProotConfig::env_command`]）：
//!    HOME/PATH 垫底，LANG/LC_ALL/LC_CTYPE/TERM 从宿主透传，宿主没设
//!    才回落 `C.UTF-8` / `xterm-256color`（musl 与新版 glibc 均内建
//!    C.UTF-8，最通用）；
//! 2. **容器 env 文件**：`<rootfs>/etc/moxsh/env`，每行 `KEY=VALUE`
//!    （`#` 注释），登录时读入合并。优先级（低到高）：引擎默认、
//!    宿主透传、容器文件、用户显式 `-e`。文件随 rootfs 打包
//!    （backup/restore）走，即 tmoe 社区同款「rootfs 内 env 文件
//!    + 启动注入」范式。
//!
//! 字体与输入法不在本模块（属配方系统 0.3.0，roadmap §4）。

use std::fs;
use std::path::{Path, PathBuf};

use super::ProotResult;

/// 容器 env 文件相对 rootfs 的位置。
pub const ENV_FILE_REL: &str = "etc/moxsh/env";

/// 容器 env 文件绝对路径。
pub fn env_file_path(rootfs: &Path) -> PathBuf {
    rootfs.join(ENV_FILE_REL)
}

/// upsert：同 key 后写覆盖先写（env 注入的唯一合并语义）。
pub fn upsert(env: &mut Vec<(String, String)>, key: &str, value: impl Into<String>) {
    let value = value.into();
    if let Some(slot) = env.iter_mut().find(|(k, _)| k == key) {
        slot.1 = value;
    } else {
        env.push((key.to_string(), value));
    }
}

/// 解析容器 env 文件文本：每行 `KEY=VALUE`；
/// `#` 起头注释、空行、无等号行一律跳过（宽容——不因一行坏数据整体失败）。
pub fn parse_env_file(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let k = k.trim();
            if !k.is_empty() {
                out.push((k.to_string(), v.trim().to_string()));
            }
        }
    }
    out
}

/// 读容器 env 文件（不存在 / 不可读 = 空表——老 rootfs 无此文件也正常）。
pub fn load_container_env(rootfs: &Path) -> Vec<(String, String)> {
    fs::read_to_string(env_file_path(rootfs))
        .map(|t| parse_env_file(&t))
        .unwrap_or_default()
}

/// 默认 env 文件内容：装完即用（C.UTF-8 保底不乱码）。
/// 想要完整中文显示：容器内装 locales 包 → `locale-gen zh_CN.UTF-8`
/// → 把 LANG 改成 `zh_CN.UTF-8`（文件即定制点，无需改引擎）。
pub const DEFAULT_ENV_FILE: &str = "# moxsh-proot 容器环境（登录时由引擎自动注入，backup/restore 随包走）。
# 每行 KEY=VALUE；本文件优先于引擎默认，低于命令行 -e。
# 完整中文显示：容器内安装 locales 包后 locale-gen zh_CN.UTF-8，再把下行改成它。
LANG=C.UTF-8
";

/// 安装后写默认 env 文件（**已存在则不碰**——尊重用户/配方改过的配置）。
/// 返回是否新写入。
pub fn write_default_env_file(rootfs: &Path) -> ProotResult<bool> {
    let path = env_file_path(rootfs);
    if path.exists() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, DEFAULT_ENV_FILE)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_file_skips_comments_and_bad_lines() {
        let got = parse_env_file(
            "# 注释行\n\nLANG=C.UTF-8\n  TERM = xterm-256color  \n坏行没有等号\n=空key\n",
        );
        assert_eq!(
            got,
            vec![
                ("LANG".to_string(), "C.UTF-8".to_string()),
                ("TERM".to_string(), "xterm-256color".to_string()),
            ]
        );
    }

    #[test]
    fn load_container_env_missing_file_is_empty() {
        let dir = std::env::temp_dir().join("moxsh-locale-test-missing");
        let _ = fs::remove_dir_all(&dir);
        assert!(load_container_env(&dir).is_empty());
    }

    #[test]
    fn load_container_env_roundtrip() {
        let dir = std::env::temp_dir().join("moxsh-locale-test-rt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("etc/moxsh")).unwrap();
        fs::write(env_file_path(&dir), "LANG=zh_CN.UTF-8\n").unwrap();
        let got = load_container_env(&dir);
        assert_eq!(got, vec![("LANG".to_string(), "zh_CN.UTF-8".to_string())]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_default_env_file_idempotent_and_content() {
        let dir = std::env::temp_dir().join("moxsh-locale-test-write");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // 首次写入。
        assert!(write_default_env_file(&dir).unwrap());
        let text = fs::read_to_string(env_file_path(&dir)).unwrap();
        assert!(text.contains("LANG=C.UTF-8"));
        assert!(text.starts_with('#'), "默认文件应以注释开头");
        // 幂等：已存在不覆盖（用户改过就尊重）。
        fs::write(env_file_path(&dir), "LANG=zh_CN.UTF-8\n").unwrap();
        assert!(!write_default_env_file(&dir).unwrap());
        assert_eq!(
            fs::read_to_string(env_file_path(&dir)).unwrap(),
            "LANG=zh_CN.UTF-8\n"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn upsert_overrides_same_key() {
        let mut env = Vec::new();
        upsert(&mut env, "LANG", "C.UTF-8");
        upsert(&mut env, "TERM", "xterm-256color");
        upsert(&mut env, "LANG", "zh_CN.UTF-8");
        assert_eq!(
            env,
            vec![
                ("LANG".to_string(), "zh_CN.UTF-8".to_string()),
                ("TERM".to_string(), "xterm-256color".to_string()),
            ]
        );
    }
}
