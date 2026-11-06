//! `mox-proot-distro` 独立可执行文件入口（原版 Termux 用户专用）。
//!
//! 与 `mox-proot` 的分工：后者按上游 proot 参数面跑**已有 rootfs**；
//! 本二进制管理 rootfs 本身（list/install/login/shell/remove/backup/restore），
//! 语义对齐上游 `proot-distro` 子命令集。安装由一键脚本
//! `scripts/install-mox-proot.sh` 一并落盘。
//!
//! 目录布局（Termux 默认）：`$HOME/.mox-proot/`
//!
//! - `containers/<id>/rootfs`   发行版根文件系统
//! - `cache/`                   下载的归档缓存
//!
//! 可用 `MOX_ROOT` 环境变量覆盖根目录。

use std::env;
use std::path::{Path, PathBuf};
use std::process;

use mox_proot::proot::distro::DistroManager;
use mox_proot::proot::cli;
use mox_proot::proot::links;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let root = default_root();
    // 【P1】`--version`/`-V`：纯文本路径，不触网/不触引擎（musl 静态构建
    // 也能安全返回），与 `-h/--help` 同一层级。
    if args.first().map(String::as_str).is_some_and(|a| matches!(a, "--version" | "-V")) {
        println!("{}", cli::version_line());
        process::exit(0);
    }
    let code = match args.first().map(String::as_str) {
        // login/shell/launch 需要真起引擎（其余子命令是文本输出，走共享路由）。
        Some("login") | Some("shell") => exec_login(&root, &args),
        Some("launch") => exec_launch(&root, &args),
        Some("tui") => {
            #[cfg(feature = "tui")]
            {
                mox_proot::proot::tui::run_tui()
            }
            #[cfg(not(feature = "tui"))]
            {
                eprintln!("mox-proot-distro: 本构建未启用 tui feature，无法进入 TUI 控制台。");
                2
            }
        }
        _ => {
            let refs: Vec<&str> = args.iter().map(String::as_str).collect();
            print!("{}", cli::proot_distro(&refs, &root));
            // 【P1】未知子命令必须返回非 0：旧实现恒 0，脚本/CI 会把
            // 「敲错命令」误判为成功（报告实测 exit code 0）。
            if cli::is_unknown_subcommand(&refs) { 2 } else { 0 }
        }
    };
    process::exit(code);
}

/// 默认根目录：`$MOX_ROOT` > `$HOME/.mox-proot`。
/// Termux 的 `$HOME` 即 `/data/data/com.termux/files/home`，落在 App 私有
/// 目录内（exec 允许、noexec 外部存储之外），符合 Android W^X 约束。
fn default_root() -> PathBuf {
    if let Some(r) = env::var_os("MOX_ROOT") {
        return PathBuf::from(r);
    }
    let home = env::var_os("HOME")
        .unwrap_or_else(|| PathBuf::from("/data/data/com.termux/files/home").into_os_string());
    Path::new(&home).join(".mox-proot")
}

/// login/shell：解析 rootfs → 结构化配置 → 交引擎执行（不 textualize 成
/// 上游 proot 命令行再解析——结构化路径少一层往返，也不会丢选项）。
///
/// 0.2.0 起与 `launch` 共用 [`cli::exec_into`]（进/退命令、钩子、shell
/// 探测同源）；本函数只保留 login/shell 的 shell 形态差异（-l 登录 shell）。
///
/// 入口互通（文心手册"登录融合成一个"）：既接受普通发行版 id，也接受 `mx:`
/// 短链——`login mx:ubuntu` 与 `launch mx:ubuntu` 完全等价（后者走解析 +
/// 未装自动装的通道），不再有两套各跑各的登录逻辑。
fn exec_login(root: &Path, args: &[String]) -> i32 {
    let sub = args[0].as_str();
    let Some(spec) = args.get(1) else {
        eprintln!("用法: mox-proot-distro {} <id|mx:名字> [-- 命令...]", sub);
        return 2;
    };
    // 【P1】`--` 之前是引擎选项（-b/-w/-e/-v/-0），之后才是容器内命令。
    // 旧实现只取 `--` 之后，前面的选项被静默丢弃——用户以为 -b 挂载生效
    // 其实没有。现在解析成结构化覆盖并真正注入 ProotConfig。
    let rest = &args[2..];
    let dash = rest.iter().position(|a| a == "--");
    let (head, tail): (Vec<String>, Vec<String>) = match dash {
        Some(p) => (rest[..p].to_vec(), rest[p + 1..].to_vec()),
        None => (rest.to_vec(), Vec::new()),
    };
    let opts = match cli::parse_engine_opts(&head) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("mox-proot-distro: {}", e);
            return 2;
        }
    };
    // mx: 短链：复用 launch 通道（解析 + 未装自动装 + 进容器），与 launch 完全互通。
    // launch 通道自带「未装自动装」，不接受外部绑定覆盖；若用户确实给了引擎
    // 选项，明确报错而不是静默丢弃（避免"以为 -b 生效其实没有"）。
    if links::is_mx(spec) {
        if !head.is_empty() {
            eprintln!("mox-proot-distro: mx: 短链启动不支持 -- 之前的引擎选项（{}）；\
                       请改用普通发行版 id，或把参数放到 -- 之后作为容器内命令", head.join(" "));
            return 2;
        }
        return match cli::launch_spec(root, spec, &tail) {
            Ok(code) => code,
            Err(e) => {
                eprintln!("mox-proot-distro: {}", e);
                2
            }
        };
    }
    // 普通发行版 id
    let mgr = DistroManager::new(root);
    // id 白名单校验（P1 防路径穿越）：与 launch_spec / install / login_cmd 同源。
    // 缺这步时 `login ../../etc` 会绕过校验直接拼 rootfs 路径（distro_rootfs
    // 用 Path::join 拼接，.. 会逃出 containers 目录），埋下 chroot 越界隐患。
    if let Err(e) = mgr.check_id(spec) {
        eprintln!("mox-proot-distro: {}", e);
        return 2;
    }
    let rootfs = mgr.distro_rootfs(spec);
    if !rootfs.is_dir() {
        eprintln!(
            "mox-proot-distro: 发行版未安装: {}（先 install {} 或 launch mx:{}）",
            spec, spec, spec
        );
        return 2;
    }
    // 无显式命令（tail 空）时按登录/非登录 shell 形态；带 -- 命令则直接用。
    let shell: Vec<String> = if tail.is_empty() {
        if rootfs.join("bin/bash").exists() {
            if sub == "login" {
                vec!["/bin/bash".into(), "-l".into()]
            } else {
                vec!["/bin/bash".into()]
            }
        } else if sub == "login" {
            vec!["/bin/sh".into(), "-l".into()]
        } else {
            vec!["/bin/sh".into()]
        }
    } else {
        tail.clone()
    };
    match cli::exec_into_with_opts(root, spec, &shell, &opts) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mox-proot-distro: {}", e);
            2
        }
    }
}

/// `launch <mx:名字|id> [-- 命令...]`：启动（0.2.0）——未装自动装，
/// 装完直接进容器。既接受 `mx:` 短链也接受普通发行版 id（与 login 互通）。
/// 见 [`cli::launch_spec`]。
fn exec_launch(root: &Path, args: &[String]) -> i32 {
    let Some(spec) = args.get(1) else {
        eprintln!("用法: mox-proot-distro launch <mx:名字|id> [-- 命令...]");
        return 2;
    };
    // 【P1】`--` 之前若有引擎选项，明确报错而非静默丢弃（launch 通道
    // 自带"未装自动装"，不接受外部绑定覆盖）。`--` 之后才是容器内命令。
    let rest = &args[2..];
    let dash = rest.iter().position(|a| a == "--");
    let (head, tail): (Vec<String>, Vec<String>) = match dash {
        Some(p) => (rest[..p].to_vec(), rest[p + 1..].to_vec()),
        None => (rest.to_vec(), Vec::new()),
    };
    if !head.is_empty() {
        eprintln!("mox-proot-distro: launch 不支持 -- 之前的引擎选项（{}）；\
                   请用 login/shell 带 -b/-w/-e，或把参数放到 -- 之后作为容器内命令",
                  head.join(" "));
        return 2;
    }
    match cli::launch_spec(root, spec, &tail) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mox-proot-distro: {}", e);
            2
        }
    }
}
