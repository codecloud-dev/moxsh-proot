//! `proot` / `proot-distro` 等价 CLI（脚本兼容层，老用户零迁移，M4）。
//!
//! 定位：把文本形式的命令路由到 [`crate::proot::distro::DistroManager`]，
//! 返回执行结果文本（由 PTY 层回显）。参数解析手写（不引 clap，依赖极简），
//! 仅覆盖 proot-distro 的常用面：list/install/login/remove/backup/restore/shell。
//!
//! 透传约定：`compat::classify` 返回 `Shell` 的命令里，`proot` 与
//! `proot-distro` 由执行引擎先经 [`route`] 进入本模块（见 compat.rs 注释）。
//!
//! ## 独立二进制入口（[`run_native`]）
//!
//! 除 App 内嵌路径外，本 crate 还编出 `mox-proot` 独立可执行文件
//! （`src/bin/mox-proot.rs`）：让**原版 Termux 用户**不装 mox-proot App 也能
//! 用上同一套自研引擎（Rust ptrace 路径翻译，非上游 C 版 proot 的封装）。
//! 参数面与上游 proot 常用选项对齐：`-r/-0/-b/-w/-R/-S/-e`，`--` 后接命令。

use std::path::{Path, PathBuf};

use crate::proot::console;
use crate::proot::distro::DistroManager;
use crate::proot::elf::{resolve_exec_chain, ExecPlan};
use crate::proot::fan;
use crate::proot::launcher;
use crate::proot::links::{self, ResolvedLink};
use crate::proot::locale;
use crate::proot::net;
use crate::proot::recipe::RecipeIndex;
use crate::proot::ptrace::Tracer;
use crate::proot::redeem;
use crate::proot::script;
use crate::proot::settings::{self, Settings};
use crate::proot::translate::TranslateRules;
use crate::proot::{ProotConfig, ProotError, ProotResult};

/// 命令路由入口：`command` 为 `proot` 或 `proot-distro`，`args` 为其余参数。
pub fn route(command: &str, args: &[&str], mox_proot_root: &Path) -> String {
    match command {
        "proot-distro" => proot_distro(args, mox_proot_root),
        // `proot` 原生参数面：mox-proot 引擎内嵌（无独立二进制），给出等效用法说明
        // 并支持 -r/-0/-b/-w 组装预览；实际启动走 mox-proot UI 或 login 命令。
        "proot" => proot_native(args, mox_proot_root),
        other => format!("proot: 未知命令 {}", other),
    }
}

/// 独立二进制入口：解析上游风格参数，真起 ptrace 引擎跑目标命令。
///
/// 返回进程退出码（0 = 成功；非 0 = 引擎报错或目标命令退出码）。
/// 错误信息写 stderr（不混入 stdout，管道/重定向不受污染）。
///
/// 与 [`route`] 的分工：`route` 服务 App 内嵌 shell 的**文本交互**；
/// 本函数服务**真执行**（原版 Termux 用户直接 `mox-proot ...`）。
pub fn run_native(args: &[String]) -> i32 {
    // `tui` 子命令（路线图 0.4.0，特性门控）：仅桌面 `--features tui` 构建提供，
    // 拉起 ratatui 控制台。原版 Termux 无此子命令（App 内自有原生 UI）。
    // 【P1】与 mox-proot-distro 对齐：--version / -V 是纯文本路径，rc=0，
    // 不触网、不进引擎。安装脚本收尾自检依赖此语义（不再用 `|| true` 吞退出码）。
    if args.len() == 1 && matches!(args[0].as_str(), "--version" | "-V") {
        println!("{}", engine_version_line());
        return 0;
    }
    #[cfg(feature = "tui")]
    if args.first().map(|s| s.as_str() == "tui").unwrap_or(false) {
        return crate::proot::tui::run_tui();
    }
    // login（设备码授权扫码绑定，后端 mox-id Workers）：**默认构建即提供**。
    // 设备码模块只依赖 curl，不再被 tui feature 门禁挡在发布产物之外。
    if args.first().map(|s| s.as_str() == "login").unwrap_or(false) {
        return crate::proot::device_code::run_login_flow();
    }
    // mx: 短链启动（0.2.0）：-r/-R/-S 的值是 mx: 形式时，本二进制直接从
    // 短链走「未装自动装 → 进容器」，无需用户先跑 distro install。
    if let Some(spec) = find_mx_rootfs_arg(args) {
        let root = Settings::load_user()
            .map(|s| s.resolve_root_dir())
            .unwrap_or_else(|_| settings::default_root_dir());
        let tail = tail_after_dashdash(args);
        return match launch_spec(&root, &spec, &tail) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("mox-proot: mx: 启动失败: {}", e);
                2
            }
        };
    }
    // 友好帮助（doc §A#16：CLI 文案引导）。裸 `help` / 无参数 → 打印用法到
    // stdout 并退出 0，避免被当成「要执行的命令」因缺 -r 而 exit 2
    // （Termux 下表现为「输入 help 没反应就退出」）。
    // （`--help`/`-h` 已由 parse_native_args 返回空错误走用法分支，这里只补裸 `help` 与无参。）
    if args.is_empty() || args.first().is_some_and(|a| a == "help") {
        print_usage(&mut std::io::stdout());
        return 0;
    }
    let cfg = match parse_native_args(args) {
        Ok(c) => c,
        Err(e) => {
            if e.is_empty() {
                // -h/--help：正常请求而非错误——用法走 stdout、退出 0
                // （对齐上游 proot 与常见 CLI 惯例；安装脚本自检也依赖此语义）。
                print_usage(&mut std::io::stdout());
                return 0;
            }
            eprintln!("mox-proot: {}", e);
            usage_to_stderr();
            return 2;
        }
    };
    let mut cmd = cfg.command.clone();
    if cmd.is_empty() {
        // 对齐上游：未给命令时默认 /bin/sh。
        cmd.push("/bin/sh".to_string());
    }
    run_config(&cfg.config, &cmd, &cfg.env)
}

/// 用结构化配置直接起引擎跑目标命令（login/shell 与 [`run_native`] 共用）。
///
/// 与 `run_native` 的分工：后者解析上游风格 CLI 参数面；本函数服务
/// "已有 [`ProotConfig`] 就直接执行"的场景（`mox-proot-distro login`）。
/// 返回进程退出码（0 = 成功；非 0 = 引擎报错或目标命令退出码）。
pub fn run_config(
    config: &crate::proot::ProotConfig,
    command: &[String],
    env: &[String],
) -> i32 {
    // 【P1 修复】诊断开关必须在 `Tracer::new` / `spawn` **之前**落地：
    // 旧实现放在 spawn 之后，`Tracer::new` 里 `seccomp_active()` 读到的
    // `diag_enabled()` 还是 false，于是 `-v` 与 `MOX_SECCOMP=1` 叠加时
    // tracer 侧开了 seccomp、子进程侧也装了过滤器，但诊断日志走双停路径，
    // 状态机交错导致 SIGABRT。提前设置后 seccomp_active() 两侧一致地让位。
    crate::proot::ptrace::set_verbose(config.verbose);
    let rules = config.to_rules();
    let initial_cwd = initial_cwd_for(config, &rules);
    // 初始 execve 宿主侧预解析（见 prepare_initial_exec 文档）：
    // 子进程首停是 exec 后的 plain SIGTRAP，syscall-enter 停被取代——
    // 初始命令无法在 tracer 内拦截，必须在这里解析成内核能直接加载的形式。
    let plan = prepare_initial_exec(command, &rules, &config.cwd);
    let mut tracer = Tracer::new(rules);
    if let Some(p) = plan.guest_prog {
        tracer.set_initial_guest_prog(p);
    }
    let cmd_refs: Vec<&str> = plan.argv.iter().map(String::as_str).collect();
    let env_refs: Vec<&str> = env.iter().map(String::as_str).collect();
    let child = match Tracer::spawn_traced_with_cwd(
        &cmd_refs,
        &env_refs,
        initial_cwd.as_deref(),
        plan.exec_path.as_deref(),
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("mox-proot: 启动失败: {}", e);
            return 126;
        }
    };
    match tracer.run(child) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("mox-proot: 引擎错误: {}", e);
            125
        }
    }
}

/// 初始 execve 的宿主侧预解析（guest 解释器问题的入口解法，M6）。
///
/// 子进程 TRACEME 后的首个停止是 exec **之后** 的 plain SIGTRAP——
/// syscall-enter 停被 exec 陷阱取代，初始 execve 无法在 tracer 内拦截
/// （SIGSTOP 握手方案在本环境已被实测证伪，见 ptrace.rs 首停注释）。
/// 因此初始命令必须在宿主侧解析成「内核能直接加载的形式」：
///
/// - **动态 ELF**（带 PT_INTERP）⇒ 显式调用 guest 自带 loader：
///   `execve(host_ldso, [ldso, prog, ...原参数])`——与 qemu-user 同款。
///   musl/glibc 的 loader 均 standalone（无 PT_INTERP，已实证），内核
///   可直接加载；此后 loader 的所有 openat 都落在引擎翻译链上；
/// - **shebang 脚本** ⇒ 解释器进链；解释器若仍是动态 ELF，继续上溯到
///   它的 loader（链式走查，与内核 shebang 语义同构）；
/// - **静态 ELF / 非 ELF / 链尾 loader 缺失** ⇒ Direct：以翻译后的
///   host 路径 exec（文件不存在时内核给出权威 ENOENT，不会静默落到
///   宿主同名文件——`/bin/sh` 在 glibc 宿主上真实存在，直穿会酿成
///   「guest sh 变宿主 dash」类事故）；
/// - **符号链接**（Alpine minirootfs 的 applet 链接是绝对 target）⇒
///   DirectResolved：exec 目标换成链接解析后的宿主路径，但 **argv[0]
///   保持调用方的 guest 路径**——busybox 多二进制按 argv[0] basename
///   选 applet，argv[0] 换了 `/bin/ls` 就变 ls 而不是别的 applet。
///
/// 返回 [`InitialExec`]：`argv` = spawn 用的最终参数向量；
/// `guest_prog` 非空时调用方应设入
/// [`Tracer::set_initial_guest_prog`]（comm 伪装 + `/proc/self/exe`
/// 还原）——exec 的是 loader/解析目标时，这两处不修则 guest 视角全错；
/// `exec_path` 非空时覆盖 exec 的文件（argv 不动）。
struct InitialExec {
    argv: Vec<String>,
    guest_prog: Option<String>,
    exec_path: Option<String>,
}

fn prepare_initial_exec(
    command: &[String],
    rules: &TranslateRules,
    guest_cwd: &str,
) -> InitialExec {
    let Some(prog) = command.first() else {
        return InitialExec {
            argv: vec!["/bin/sh".to_string()],
            guest_prog: None,
            exec_path: None,
        };
    };
    // 裸命令名（无 `/`）：无 PATH 信息无法可靠解析，Direct 交内核。
    if !prog.contains('/') {
        return InitialExec {
            argv: command.to_vec(),
            guest_prog: None,
            exec_path: None,
        };
    }
    // guest 视角的规范绝对路径（相对路径按 guest 初始 cwd 拼）。
    let guest_path = if prog.starts_with('/') {
        crate::proot::translate::normalize(prog)
            .to_string_lossy()
            .into_owned()
    } else {
        let joined = if guest_cwd.starts_with('/') {
            format!("{}/{}", guest_cwd.trim_end_matches('/'), prog)
        } else {
            prog.clone()
        };
        crate::proot::translate::normalize(&joined)
            .to_string_lossy()
            .into_owned()
    };
    let initial_host = rules.guest_to_host(&guest_path);
    match resolve_exec_chain(&guest_path, &initial_host, rules) {
        ExecPlan::Rewrite { exec_host, argv_prefix } => {
            // argv 布局：[loader 宿主路径, loader guest 路径, ...链上程序, 原参数]。
            // argv[0] 用宿主路径（内核 exec 需要真实文件）；argv[1] 起是
            // guest 视角——loader 按 argv[1] 找目标程序，其 openat 被翻译。
            let mut out = Vec::with_capacity(argv_prefix.len() + command.len());
            out.push(exec_host);
            out.extend(argv_prefix);
            out.extend(command[1..].iter().cloned());
            InitialExec {
                argv: out,
                guest_prog: Some(guest_path),
                exec_path: None,
            }
        }
        ExecPlan::DirectResolved { exec_host } => {
            // 符号链接解析后的静态目标：exec 目标文件 = 解析后的宿主路径，
            // 但 **argv[0] 保持调用方的 guest 路径**（busybox 等多二进制
            // 依赖 argv[0] basename 选 applet——`/bin/ls` 链接到 busybox
            // 时 argv[0] 必须是 `/bin/ls`）。comm/exe 同样按 guest 路径伪装。
            let mut out = command.to_vec();
            out[0] = prog.clone();
            InitialExec {
                argv: out,
                guest_prog: Some(guest_path),
                exec_path: Some(exec_host),
            }
        }
        ExecPlan::Direct => {
            // host 路径 exec：不存在 → 内核 ENOENT（exit 127），
            // 杜绝静默落到宿主同名文件。comm/exe 天然正确，无需覆盖。
            let mut out = vec![initial_host.to_string_lossy().into_owned()];
            out.extend(command[1..].iter().cloned());
            InitialExec {
                argv: out,
                guest_prog: None,
                exec_path: None,
            }
        }
    }
}

/// 初始 cwd 落地（M5.2：此前 -w 只存进规则集、从未真正切换子进程 cwd）。
///
/// - 显式 `-w`：绝对路径按规则翻译（走 rootfs/绑定）；相对路径——有 rootfs
///   时按 rootfs 解析（guest 视角相对量），无 rootfs 时 `None`（继承 tracer
///   cwd，按宿主当前目录解析相对量）；
/// - 未给 `-w` 但有 rootfs：cwd = rootfs 根（对齐 proot `-r` 语义，否则
///   子进程继承 tracer 宿主 cwd，guest 直接看到 rootfs 外真实路径）；
/// - 两者皆无：`None` = 继承 tracer cwd（对齐上游 proot 无 `-r` 行为）。
///
/// 纯函数（除 `guest_to_host` 的规则查询外无副作用），便于单测钉死语义。
fn initial_cwd_for(
    cfg: &crate::proot::ProotConfig,
    rules: &crate::proot::translate::TranslateRules,
) -> Option<std::path::PathBuf> {
    if cfg.cwd != "/" {
        let p = std::path::Path::new(&cfg.cwd);
        if p.is_absolute() {
            Some(rules.guest_to_host(&cfg.cwd))
        } else if !cfg.rootfs.as_os_str().is_empty() {
            Some(cfg.rootfs.join(p))
        } else {
            None
        }
    } else if !cfg.rootfs.as_os_str().is_empty() {
        Some(rules.guest_to_host("/"))
    } else {
        None
    }
}

/// `run_native` 的解析产物（与 [`ProotConfig`] + 待执行命令分离，便于单测）。
#[derive(Debug)]
struct NativeArgs {
    config: crate::proot::ProotConfig,
    command: Vec<String>,
    env: Vec<String>,
}

/// 解析 `mox-proot` 参数面（对齐上游 proot 常用选项）。
///
/// `-b` 语法与上游一致：`host_path:guest_location`（host 在前）。
/// guest_location 尾缀 `!` = 不反向还原（沿用本仓 translate 层语义）。
fn parse_native_args(args: &[String]) -> Result<NativeArgs, String> {
    let mut rootfs: Option<PathBuf> = None;
    let mut fake_root = false;
    let mut fake_uid: u32 = 0;
    let mut fake_gid: u32 = 0;
    let mut verbose: u8 = 0;
    let mut hidden: Vec<String> = Vec::new();
    let mut cwd = "/".to_string();
    let mut binds: Vec<(String, String)> = Vec::new();
    let mut env: Vec<String> = Vec::new();
    let mut recommended = false;
    let mut minimal = false;
    let mut command: Vec<String> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => {
                return Err(String::new()); // 空错误 → 调用方只打印用法
            }
            "-r" | "--rootfs" => {
                i += 1;
                let v = args.get(i).ok_or("-r 缺少参数")?;
                rootfs = Some(PathBuf::from(v));
            }
            "-0" | "--root-id" => fake_root = true,
            "-v" | "--verbose" => {
                // 诊断日志级别累加：每多一个 -v 升一级（`-vvv` 对应 3 级）。
                // 引擎内部仅「开/关」两档（见 ptrace::diag_enabled），多档保留
                // 给将来更细分层（如 syscall 级 / scratch 级）。
                verbose = verbose.saturating_add(1);
            }
            "-i" | "--fake-id" => {
                // 上游 `-i uid[:gid[:gid_min:gid_max]]`：本引擎实现 `uid[:gid]`
                // （省略 gid 时等于 uid）。补全 fake root 身份——不再固定 0:0。
                i += 1;
                let v = args.get(i).ok_or("-i 缺少参数（期望 uid 或 uid:gid）")?;
                let (u, g) = parse_fake_id(v)?;
                fake_root = true;
                fake_uid = u;
                fake_gid = g;
            }
            "-H" | "--hide" => {
                // 隐藏指定 guest 路径（可重复）：访问即 ENOENT。用于把宿主/容器
                // 敏感文件从 guest 视野抹除（P2 小改动，各解决一个真实痛点）。
                i += 1;
                let v = args.get(i).ok_or("-H/--hide 缺少参数（期望 guest 路径）")?;
                hidden.push(v.to_string());
            }
            "-b" | "--bind" | "-m" | "--mount" => {
                i += 1;
                let v = args.get(i).ok_or("-b 缺少参数")?;
                binds.push(parse_binding(v)?);
            }
            "-w" | "--pwd" | "--cwd" => {
                i += 1;
                let v = args.get(i).ok_or("-w 缺少参数")?;
                cwd = v.to_string();
            }
            "-e" | "--env" => {
                i += 1;
                let v = args.get(i).ok_or("-e 缺少参数")?;
                if !v.contains('=') {
                    return Err(format!("-e 需要 KEY=VALUE 形式，得到 {}", v));
                }
                env.push(v.clone());
            }
            "-R" => {
                i += 1;
                let v = args.get(i).ok_or("-R 缺少参数")?;
                rootfs = Some(PathBuf::from(v));
                recommended = true;
            }
            "-S" => {
                i += 1;
                let v = args.get(i).ok_or("-S 缺少参数")?;
                rootfs = Some(PathBuf::from(v));
                minimal = true;
            }
            "--" => {
                i += 1;
                command.extend_from_slice(&args[i..]);
                break;
            }
            other if other.starts_with("--rootfs=") => {
                rootfs = Some(PathBuf::from(&other["--rootfs=".len()..]));
            }
            other if other.starts_with("--bind=") || other.starts_with("--mount=") => {
                let v = other.split_once('=').map(|x| x.1).unwrap_or("");
                binds.push(parse_binding(v)?);
            }
            other if other.starts_with("--cwd=") => {
                cwd = other["--cwd=".len()..].to_string();
            }
            other if other.starts_with("--pwd=") => {
                cwd = other["--pwd=".len()..].to_string();
            }
            other if other.starts_with("--env=") => {
                let v = &other["--env=".len()..];
                if !v.contains('=') {
                    return Err(format!("-e 需要 KEY=VALUE 形式，得到 {}", v));
                }
                env.push(v.to_string());
            }
            "--kill-on-exit" => {
                // 接受：mox-proot 作为 ptrace tracer 在退出时统一回收所有子 tracee，
                // 等价于 proot-distro 的 --kill-on-exit，无需额外 flag（login_cmd 默认带此旗标）。
            }
            "--link2symlink" => {
                // 接受：绝对符号链接规范化由 symlink_fix 模块在落盘/运行时处理
                // （proot-distro link2symlink 的升级解法），无需运行时 flag。
            }
            other if other.starts_with("--change-id=") => {
                // --change-id=uid:gid：对齐上游 fake_id0。本引擎仅实现 0:0（fake root）。
                let v = &other["--change-id=".len()..];
                if v.split(':').next().map(|u| u == "0").unwrap_or(false) {
                    fake_root = true;
                }
            }
            other if other.starts_with("--fake-id=") => {
                // --fake-id=uid[:gid]：补全 fake root 身份（见 -i 分支）。
                let v = &other["--fake-id=".len()..];
                let (u, g) = parse_fake_id(v)?;
                fake_root = true;
                fake_uid = u;
                fake_gid = g;
            }
            other if other.starts_with("--hide=") => {
                hidden.push(other["--hide=".len()..].to_string());
            }
            other if other.starts_with('-') => {
                // 注意：宏参数里的相邻字符串字面量**不会**隐式拼接（与普通表达式
                // 不同），必须写成单个字面量，否则 rustc 报 "expected `,`"。
                return Err(format!(
                    "不支持的选项 {}（可用项见 mox-proot --help；上游 -q/-k 等未实现，勿假装支持）",
                    other
                ));
            }
            // 首个非选项词即命令起点（与上游"选项后接命令"一致，无需强制 --）。
            _ => {
                command = args[i..].to_vec();
                break;
            }
        }
        i += 1;
    }

    let Some(rf) = rootfs else {
        return Err("缺少 -r/--rootfs（或用 -R/-S 简写）".into());
    };
    if !rf.is_dir() {
        return Err(format!("rootfs 目录不存在: {}", rf.display()));
    }

    let mut config = if recommended {
        crate::proot::ProotConfig::with_recommended_bindings(&rf)
    } else if minimal {
        crate::proot::ProotConfig::with_minimal_bindings(&rf)
    } else {
        crate::proot::ProotConfig::new(&rf)
    };
    config.fake_root = fake_root;
    config.fake_uid = fake_uid;
    config.fake_gid = fake_gid;
    config.verbose = verbose;
    config.hidden = hidden;
    config.cwd = cwd;
    for (guest, host) in binds {
        config.bind(&guest, &host);
    }
    // 命令显式指定 -e 时以所给环境为准；否则 CLI 走"继承 tracer 环境"路径
    // （env 为空时 spawn_traced_with_env 沿用历史 NULL envp 语义——
    //  注意：那会得到空环境。故 CLI 在未给 -e 时主动填入 std::env::vars）。
    let final_env = if env.is_empty() {
        std::env::vars()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect()
    } else {
        env
    };
    Ok(NativeArgs {
        config,
        command,
        env: final_env,
    })
}

/// 解析上游 `-b host:guest`（host 在前）。返回 `(guest, host)`——
/// 与 [`crate::proot::ProotConfig::bind`] 的参数顺序一致。
fn parse_binding(spec: &str) -> Result<(String, String), String> {
    // 单路径形式（无冒号）：host 与 guest 同路径（上游默认语义）。
    let (host, guest) = match spec.split_once(':') {
        Some((h, g)) => (h.to_string(), g.to_string()),
        None => (spec.to_string(), spec.to_string()),
    };
    if host.is_empty() || guest.is_empty() {
        return Err(format!("绑定参数不合法: {}（期望 host:guest）", spec));
    }
    // guest 尾缀 ! = 不反向还原（沿用 translate 层 no-isolate 语义）。
    if let Some(g) = guest.strip_suffix('!') {
        return Ok((format!("!{}", g), host));
    }
    Ok((guest, host))
}

/// 解析 `-i`/`--fake-id` 的身份串 `uid[:gid]`。
///
/// - `uid`：必填，1–65535（0 由 `-0` 语义覆盖，但此处允许 0 以兼容
///   `--fake-id=0:0` 显式写法）；
/// - `gid`：可省略，省略时等于 `uid`；
/// - 不支持的 `gid_min:gid_max` 扩展段：文档仅要求补齐 `uid:gid`，多余段报错。
fn parse_fake_id(spec: &str) -> Result<(u32, u32), String> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() > 2 {
        return Err(format!(
            "fake-id 仅支持 uid[:gid]，得到 {}（gid_min:gid_max 暂不支持）",
            spec
        ));
    }
    let uid: u32 = parts[0]
        .parse()
        .map_err(|_| format!("-i uid 非法: {}", parts[0]))?;
    let gid: u32 = if let Some(g) = parts.get(1) {
        g.parse().map_err(|_| format!("-i gid 非法: {}", g))?
    } else {
        uid
    };
    Ok((uid, gid))
}

/// 用法文本（`-h` 走 stdout 退出 0；参数错误走 stderr 退出 2）。
fn print_usage(w: &mut dyn std::io::Write) {
    write!(
        w,
        "用法: mox-proot [-r rootfs | -R rootfs | -S rootfs] [-0] [-i uid:gid] \
         [-H path]... [-v]... [-b host:guest]... [-w dir] [-e K=V]... [--] 命令 [参数...]\n\
         \n\
           -r, --rootfs <path>   guest 根目录\n\
           -R <path>            -r + 推荐绑定集（/etc 元文件、/tmp、/run…）\n\
           -S <path>            -r + 最小绑定集（装包场景）\n\
           -0, --root-id        fake root（uid/gid 显示为 0，伪装特权成功）\n\
           -i, --fake-id <u[:g]>  fake root 身份（uid[:gid]，补全 -0 的固定 0:0）\n\
           -H, --hide <path>    隐藏 guest 路径（访问即 ENOENT，可重复）\n\
           -v, --verbose        诊断日志级别（累加，-v 开 -vv 更细）\n\
           -b, --bind <h:g>     绑定宿主路径到 guest（host:guest，可多次）\n\
           -w, --cwd <dir>      初始工作目录\n\
           -e, --env <K=V>      注入环境变量（给了则清空继承环境）\n\
           --                   其后按原样视为命令\n\
         \n\
         未给命令时默认 /bin/sh；未给 -e 时继承当前环境。\n"
    )
    .ok();
}

fn usage_to_stderr() {
    print_usage(&mut std::io::stderr());
}

// ==================== 颜色 / 进度条（T2：NO_COLOR 感知） ====================
//
// 遵循 no-color.org 约定：`NO_COLOR` 设了任意值即关闭全部 ANSI 着色；
// 另外只在 stderr 是终端时才上色（管道/CI 下保持纯文本，不污染重定向输出）。

use crate::proot::palette;
const C_CYAN: &str = palette::ANSI_INFO;
const C_GREEN: &str = palette::ANSI_SUCCESS;
const C_YELLOW: &str = palette::ANSI_WARNING;
const C_RED: &str = palette::ANSI_ERROR;
const C_BOLD: &str = palette::ANSI_BOLD;
const C_RESET: &str = palette::ANSI_RESET;

/// `--plain` 开关（仅影响本次 `proot_distro` 调用，不改环境变量）。
static PLAIN_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `--plain` / `--json` 只作用于单次命令：用 RAII 在返回时恢复旧值，
/// 避免进程内多次调用（TUI 内嵌路由、单测、REPL）互相串色。
struct PlainModeGuard(bool);

impl PlainModeGuard {
    fn apply(enable: bool) -> Self {
        let prev = PLAIN_MODE.swap(enable, std::sync::atomic::Ordering::Relaxed);
        PlainModeGuard(prev)
    }
}

impl Drop for PlainModeGuard {
    fn drop(&mut self) {
        PLAIN_MODE.store(self.0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// 是否允许着色：遵循 no-color.org，且 `--plain` 可强制关闭。
fn use_color() -> bool {
    !PLAIN_MODE.load(std::sync::atomic::Ordering::Relaxed) && palette::enabled()
}

/// 给字符串包一层 ANSI 颜色（不开放则原样返回）。
fn paint(s: &str, code: &str) -> String {
    if use_color() {
        format!("{}{}{}", code, s, C_RESET)
    } else {
        s.to_string()
    }
}

/// 进度条：`[====>   ] 45%`；`NO_COLOR` 或总量为 0 时退化为纯百分比文本。
fn progress_bar(done: u64, total: u64) -> String {
    if total == 0 {
        return String::new();
    }
    let pct = ((done * 100) / total).min(100) as usize;
    if !use_color() {
        return format!("[{}%]", pct);
    }
    let width = 24usize;
    let filled = (pct * width) / 100;
    let mut bar = String::from("[");
    for i in 0..width {
        match i.cmp(&filled) {
            std::cmp::Ordering::Less => bar.push('='),
            std::cmp::Ordering::Equal => bar.push('>'),
            std::cmp::Ordering::Greater => bar.push(' '),
        }
    }
    bar.push(']');
    format!("{} {}%", bar, pct)
}

/// 统一的安装进度回调（彩色进度条 + 阶段说明，输出到 stderr）。
///
/// 供 `cmd_install` / `launch_spec` / `cmd_wizard` 共用，避免三处各写一遍。
fn install_progress(msg: &str, done: u64, total: u64) {
    if total > 0 {
        eprintln!(
            "{} {} {}",
            paint("[安装]", C_CYAN),
            progress_bar(done, total),
            msg
        );
    } else {
        eprintln!("{} {}", paint("[安装]", C_CYAN), msg);
    }
}

/// 已知子命令白名单（供 `--version` 与「未知子命令退出码」判定共用）。
/// 与下方 match 分支保持一致；新增子命令务必同步此处。
const KNOWN_SUBCOMMANDS: &[&str] = &[
    "list", "versions", "install", "login", "shell", "remove", "backup",
    "restore", "net", "env", "recipe", "fan", "redeem", "access", "time",
    "egg", "wizard", "launch", "help", "-h", "--help", "--json", "--plain",
    "--version", "-V", "tui",
];

/// 剥离 `--json`/`--plain` 等全局开关后，取真正的子命令。
fn primary_subcommand<'a>(args: &[&'a str]) -> &'a str {
    args.iter()
        .copied()
        .find(|a| !matches!(*a, "--json" | "--plain" | ""))
        .unwrap_or("")
}

/// 【P1】子命令是否为未知命令（供二进制入口设置非 0 退出码）。
/// 空参数视为 `help`（合法），其余不在白名单的一律判未知。
pub fn is_unknown_subcommand(args: &[&str]) -> bool {
    let sub = primary_subcommand(args);
    !sub.is_empty() && !KNOWN_SUBCOMMANDS.contains(&sub)
}

/// 【P1】`--version` / `-V` 的版本串（二进制入口与文本路由共用）。
pub fn version_line() -> String {
    format!("mox-proot-distro {}（mox-proot 兼容实现）", env!("CARGO_PKG_VERSION"))
}

/// 独立引擎主程序 `mox-proot` 的版本串。此前只有 distro 二进制实现了
/// `--version`/`-V`，引擎主程序落到未知子命令 exit 2，安装脚本收尾自检一直假绿。
pub fn engine_version_line() -> String {
    format!("mox-proot {}（Rust ptrace 引擎）", env!("CARGO_PKG_VERSION"))
}

/// 空参数时的精简首屏：只给「装 → 自证 → 进管理台」三步，细节留给 `help`。
fn quick_start() -> String {
    "mox-proot-distro —— 三步开始：\n\
       mox-proot-distro install alpine\n\
       mox-proot-distro login alpine -- cat /etc/os-release   # 能输出系统信息即装好了\n\
       mox                                                    # 进容器后打开管理台\n\n\
     完整命令清单：mox-proot-distro help"
        .to_string()
}

/// `proot-distro` 主路由（对齐上游子命令集）。
pub fn proot_distro(args: &[&str], mox_proot_root: &Path) -> String {
    let mgr = DistroManager::new(mox_proot_root);
    // 机器可读输出层：`--json` / `--plain` 可出现在任意位置，剥离后不改变子命令路由。
    // --json 走纯 stdout 数据（无颜色、无装饰、可被 jq 解析）；
    // --plain 只关装饰，保留人读格式。二者同时出现时 --json 优先。
    let mut json_mode = false;
    let mut plain_mode = false;
    let filtered: Vec<&str> = args
        .iter()
        .copied()
        .filter(|a| match *a {
            "--json" => { json_mode = true; false }
            "--plain" => { plain_mode = true; false }
            _ => true,
        })
        .collect();
    // --json 天然不能有 ANSI，--plain 显式关装饰；二者任一命中都关色。
    // guard 只覆盖本次调用，返回时恢复旧值，不会污染后续命令。
    let _plain_guard = (plain_mode || json_mode).then(|| PlainModeGuard::apply(true));
    let (sub, rest) = filtered.split_first().map_or(("", &[][..]), |(s, r)| (*s, r));
    match sub {
        "list" => if json_mode { cmd_list_json(&mgr) } else { cmd_list(&mgr) },
        "versions" => if json_mode { cmd_versions_json(rest) } else { cmd_versions(rest) },
        "install" => cmd_install(&mgr, rest),
        "login" => cmd_login(&mgr, rest),
        "shell" => cmd_shell(&mgr, rest),
        "remove" => cmd_remove(&mgr, rest),
        "backup" => cmd_backup(&mgr, rest),
        "restore" => cmd_restore(&mgr, rest, mox_proot_root),
        "net" => cmd_net(&mgr, rest),
        "env" => cmd_env(&mgr, rest),
        "recipe" => cmd_recipe(rest, mox_proot_root),
        "fan" => cmd_fan(rest, mox_proot_root),
        "redeem" => cmd_redeem(rest, mox_proot_root),
        "access" => cmd_access(rest, mox_proot_root),
        "time" => cmd_time(),
        "egg" => cmd_egg(),
        "wizard" => cmd_wizard(rest),
        // launch 需真起引擎（未装自动装），不在文本路由内执行——
        // bin 层 main 把 login/shell/launch 一并路由到 exec 分支；
        // 走到这里（App 内嵌文本路由）只给用法提示。
        "launch" => "用法: mox-proot-distro launch mx:<名字> [-- 命令...]\n\
             （mx:alpine / mx:ubuntu / mx:debian / mx:kali；自定义见 ~/.config/mox-proot/links.toml）"
            .to_string(),
        // 【P1】`--version`/`-V`：与 `mox-proot --help` 一样的纯文本路径，
        // 不触网、不触引擎，musl 静态构建也能安全返回。
        "--version" | "-V" => version_line(),
        "help" | "--help" | "-h" => HELP.to_string(),
        // 【体验修复】用户直接敲 `mox-proot-distro` 回车时给最短上手路径，
        // 不再把空参数混进「未知子命令」分支，也不塞一整屏速查表。
        "" => quick_start(),
        other => {
            let known = KNOWN_SUBCOMMANDS;
            let lower = other.to_lowercase();
            let guess = known.iter().find(|k| !lower.is_empty() && (k.starts_with(&lower) || lower.starts_with(**k)));
            let mut out = format!(
                "✗ 未知子命令「{}」，没找到这条命令。\n  怎么办：先跑 `proot-distro list` 看可用发行版，或 `help` 看全部命令。",
                other
            );
            if let Some(g) = guess {
                out.push_str(&format!("\n  你是不是想输入：{} ？", g));
            }
            out
        }
    }
}

/// `net [id]`：容器网络边界报告（M7 网络栈重写）。
///
/// 无参数时报告宿主侧网络环境（DNS/IPv6/出网探测）；带发行版 id 时
/// 叠加该 rootfs 的 resolv.conf/hosts 现状。纯报告，不改配置。
fn cmd_net(mgr: &DistroManager, args: &[&str]) -> String {
    match args.first().copied() {
        None | Some("") => {
            // 宿主环境画像（未指定发行版）
            let profile = net::build_profile("");
            net::network_report(&profile, 0, net::probe_egress())
        }
        Some(id) => match mgr.net_report(id) {
            Ok(r) => r,
            Err(e) => format!("proot-distro net: {}\n", e),
        },
    }
}

/// `env <id>`：预览登录该容器时**实际注入**的环境变量（0.2.0，roadmap §2）。
///
/// 中文乱码排查第一站：看 LANG 是不是 `*.UTF-8`。合并口径与登录路径
/// 完全同源（[`ProotConfig::env_command`]），所见即所得。
fn cmd_env(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first().copied() else {
        return "用法: mox-proot-distro env <id>".to_string();
    };
    let rootfs = mgr.distro_rootfs(id);
    if !rootfs.is_dir() {
        return format!("proot-distro: 发行版未安装: {}（先 install {}）", id, id);
    }
    let cfg = ProotConfig::with_distro_defaults(&rootfs);
    let mut out = format!(
        "容器 {} 登录时注入的环境（引擎默认 < 宿主透传 < 容器 env 文件 < 命令行 -e）：\n",
        id
    );
    for kv in cfg.env_command().into_iter().skip(2) {
        out.push_str(&format!("  {}\n", kv));
    }
    let file = locale::env_file_path(&rootfs);
    out.push_str(&format!("\n容器 env 文件：{}", file.display()));
    if file.exists() {
        out.push_str("（存在，可编辑定制——改完下次登录生效）");
    } else {
        out.push_str("（未创建，当前用引擎默认值）");
    }
    out.push_str(
        "\n中文乱码排查：LANG 应为 *.UTF-8；容器内安装 locales 包并\n\
         locale-gen zh_CN.UTF-8 后，把该文件里的 LANG 改成 zh_CN.UTF-8 即完整中文。",
    );
    out
}

/// `access status|sync`：统一访问面（粉丝专属 + 商品授权融合；粉丝默认隐藏）。
///
/// - `status`：本地视图（灰度关时粉丝档位不显示）。
/// - `sync`：管理后台统一刷新（授权重拉 + 粉丝状态重读，使服务端吊销即时生效）。
fn cmd_access(args: &[&str], mox_proot_root: &Path) -> String {
    match args.first().copied() {
        Some("status") => {
            let s = crate::proot::access::status(mox_proot_root);
            let mut out = String::from("mox-proot 访问状态：\n");
            if let Some(tier) = &s.fan_tier {
                out.push_str(&format!("  粉丝档位: {}\n", tier));
            } else if s.fan_enabled {
                out.push_str("  粉丝档位: 未解锁\n");
            } else {
                out.push_str("  粉丝档位: （灰度未开启/隐藏）\n");
            }
            if s.entitlements.is_empty() {
                out.push_str("  商品授权: （无）\n");
            } else {
                out.push_str("  商品授权:\n");
                for (pid, tier) in &s.entitlements {
                    out.push_str(&format!("    {}  [{}]\n", pid, tier));
                }
            }
            out
        }
        Some("sync") => match crate::proot::access::sync(mox_proot_root) {
            Ok(s) => format!(
                "已同步访问状态：粉丝{}，商品授权 {} 项",
                if s.fan_tier.is_some() {
                    "已解锁"
                } else {
                    "未解锁/隐藏"
                },
                s.entitlements.len()
            ),
            Err(e) => format!("access sync 失败: {}", e),
        },
        _ => "用法: proot-distro access status | sync".to_string(),
    }
}

/// `time`：时间上下文感知问候 + 古诗（#450 惊喜三件套的 Rust 单一来源）。
///
/// 与容器内控制台 `mox` 的无参问候、TUI 欢迎屏同源；本子命令便于
/// 脚本/测试直接取用，且永远与 delight 模块保持一致。
fn cmd_time() -> String {
    let phase = crate::proot::delight::current_phase();
    let mut out = String::from(paint("mox-proot 时间上下文", C_BOLD).as_str());
    out.push('\n');
    out.push_str(&format!("  时相：{}\n", phase.name()));
    out.push_str(&format!("  {}\n", phase.greeting()));
    out.push_str(&format!("  {} {}\n", phase.poem(), phase.poem_from()));
    out
}

/// `egg`：隐藏彩蛋（#450）。命中暗号才吐文案；平时不显。
fn cmd_egg() -> String {
    match crate::proot::delight::easter_egg("egg") {
        Some(t) => t.to_string(),
        None => "（没有彩蛋）".to_string(),
    }
}

/// `recipe list|show <id>|apply <id> <distro>`：配方系统（0.3.0，roadmap §4）。
///
/// - `list`：枚举可用配方（id/名称/描述）。
/// - `show <id>`：配方详情 + 应用命令。
/// - `apply <id> <distro>`：把脚本落进容器 /tmp 后在容器内执行（复用引擎）。
fn cmd_recipe(args: &[&str], mox_proot_root: &Path) -> String {
    let idx = RecipeIndex::load();
    match args.first().copied() {
        None | Some("list") => {
            if idx.all().is_empty() {
                return "配方：当前无可用配方（recipes/index.toml 为空）".to_string();
            }
            let mut out = String::from(
                "可用配方（应用：mox-proot-distro recipe apply <id> <distro>）：\n",
            );
            for r in idx.all() {
                let lock = if r.fan_only {
                    if fan::is_fan(mox_proot_root) {
                        " [✓]"
                    } else {
                        " [🔒]"
                    }
                } else if !r.requires.is_empty() {
                    if redeem::has_entitlement(mox_proot_root, &r.requires) {
                        " [✓]"
                    } else {
                        " [🔒]"
                    }
                } else {
                    ""
                };
                out.push_str(&format!(
                    "  {:<8} {:<16} {}{}\n",
                    r.id, r.name, r.description, lock
                ));
            }
            // 新手看到 🔒 不知道去哪开通：直接给可点的地址与容器内命令。
            if out.contains("[🔒]") {
                out.push_str(
                    "\n🔒 = 尚未解锁：访问 https://mox-site.pages.dev/fan 兑换，\n\
                     或在容器内执行 `mox redeem login` 完成授权后重新 list。\n",
                );
            }
            out
        }
        Some("show") => match args.get(1) {
            Some(id) => match idx.find(id) {
                Ok(r) => {
                    let mut s = format!(
                        "配方 {}\n  名称：{}\n  描述：{}\n  标签：{}\n  脚本：{}\n  应用：mox-proot-distro recipe apply {} <distro>",
                        r.id, r.name, r.description, r.tags.join("/"), r.script, r.id
                    );
                    if r.fan_only {
                        s.push_str(&format!(
                            "\n  专属：{}",
                            if fan::is_fan(mox_proot_root) {
                                "已解锁".to_string()
                            } else {
                                "未解锁".to_string()
                            }
                        ));
                    }
                    if !r.requires.is_empty() {
                        s.push_str(&format!(
                            "\n  商品：{}（{}）",
                            r.requires,
                            if redeem::has_entitlement(mox_proot_root, &r.requires) {
                                "已授权".to_string()
                            } else {
                                "未授权，mox redeem status 刷新".to_string()
                            }
                        ));
                    }
                    s
                }
                Err(e) => format!("recipe: {}", e),
            },
            None => "用法: mox-proot-distro recipe show <id>".to_string(),
        },
        Some("apply") => {
            let rid = args.get(1).copied();
            let did = args.get(2).copied();
            match (rid, did) {
                (Some(rid), Some(did)) => match idx.apply(mox_proot_root, rid, did) {
                    Ok(code) => format!("配方 {} 已应用（容器内退出码 {}）", rid, code),
                    Err(e) => format!("recipe: 应用失败: {}", e),
                },
                _ => "用法: mox-proot-distro recipe apply <id> <distro>".to_string(),
            }
        }
        Some(other) => format!(
            "recipe: 未知子命令 {}\n{}",
            other,
            "用法: mox-proot-distro recipe list|show <id>|apply <id> <distro>"
        ),
    }
}

/// `fan login|redeem <code>|status`：专属解锁（回环自动收码 + 在线校验）。
///
/// - `login`（默认）：本机 `127.0.0.1` 起一次性回环监听 → 唤起手机浏览器打开
///   专属页面（入口命令由运营渠道告知）→ 页内 GitHub 登录 + 点「确认领取」
///   = 后端签发激活码 → 页面带 `code` 跳回回环，脚本 **自动收码** →
///   `POST /fan/verify` 经服务端**联网真实校验** → 落盘
///   `$MOX_ROOT/fan.toml` 解锁 fan_only 配方。全程无需手输兑换码；
/// - `redeem <code>`：手动路径（桌面 Linux 没浏览器可调时）——页面
///   手动领取后回来 `mox fan redeem <code>`，同样联网验；
/// - `status`：当前解锁状态与验签模式（纯在线，客户端零密钥）。
///
/// 灰度：`MOX_FAN_ENABLE=1` 才开放（后端 `/fan/issue` 另有白名单，双保险）。
fn cmd_fan(args: &[&str], mox_proot_root: &Path) -> String {
    if !fan::fan_enabled() {
        return format!(
            "粉丝专属：灰度测试中（{}=1 开启，正式版自动开放）。\n\
             \x20 该功能灰度测试中，暂未开放。",
            fan::FAN_ENABLE_ENV
        );
    }
    match args.first().copied() {
        None | Some("login") => match fan::login(mox_proot_root) {
            Ok(o) => {
                let mut out = String::new();
                match o.opened {
                    fan::Opened::Via(via) => {
                        out.push_str(&format!("已通过 {} 打开官网粉丝专区：\n  {}\n", via, o.url))
                    }
                    fan::Opened::Failed => out.push_str(&format!(
                        "未能自动唤起浏览器，请手动打开：\n  {}\n",
                        o.url
                    )),
                }
                out.push_str(&format!(
                    "\n页面步骤：登录（GitHub）→ 点「确认领取」→ 自动回到本终端。\n\
                     ✅ 官方在线校验已通过，粉丝专属已解锁（档位：{}）。\n\
                     \x20 mox fan status                   # 查看状态\n\
                     \x20 mox-proot-distro recipe list   # 粉丝专属配方已亮起 🔓",
                    o.tier
                ));
                out
            }
            Err(e) => format!("fan: {}\n（重试：mox fan login；桌面 Linux 可先开专属页面 URL 手动领取，再 mox fan redeem <code>）", e),
        },
        Some("redeem") => match args.get(1).copied() {
            Some(code) => match fan::redeem(mox_proot_root, code) {
                Ok(tier) => format!(
                    "兑换成功，粉丝专属已解锁（档位：{}）。\n\
                     \x20 mox fan status                        # 查看状态\n\
                     \x20 mox-proot-distro recipe list        # 粉丝专属配方已亮起",
                    tier
                ),
                Err(e) => format!("fan: {}", e),
            },
            None => "用法: mox fan redeem <兑换码>\n（桌面 Linux：mox fan login 会给出专属页面 URL，手动领取后回来兑换）".to_string(),
        },
        Some("status") => {
            let mode = fan::verify_mode();
            match fan::load_state(mox_proot_root) {
                Some(st) => format!(
                    "粉丝专属：已解锁\n  档位：{}\n  验签模式：{}\n  兑换码：{}\n  令牌文件：{}\n  （删除本文件 = 解除绑定，需重新 mox fan login）",
                    st.tier,
                    mode,
                    st.code,
                    fan::fan_file_path(mox_proot_root).display()
                ),
                None => format!(
                    "粉丝专属：未解锁\n  验签模式：{}\n  mox fan login    # 唤起浏览器 → 登录 → 确认领取 → 自动收码解锁",
                    mode
                ),
            }
        }
        Some(other) => format!(
            "fan: 未知子命令 {}\n用法: mox fan login|redeem <code>|status",
            other
        ),
    }
}

/// `redeem login|status|check`：商品授权（设备绑定 + 在线校验，正式功能无灰度）。
///
/// 与 fan 并列互不覆盖：fan 是账号级粉丝福利（`fan.toml`），本命令是设备级
/// 商品授权（`device.toml` + `entitlements.toml`，见 redeem.rs）：
/// - `login`（默认）：起一次性回环 → 唤起官网 bind.html → GitHub 登录 + 点
///   「绑定此设备」→ 后端把本机设备令牌挂到账号 → 自动回到本终端；
/// - `status`：联网 `GET /entitlement/check` 拉全部授权 → 落盘缓存
///   （一行一个 `ok <product_id> <tier> <exp>`）；
/// - `check`：只读本地缓存（离线可用，不联网）——门禁判定同源。
///
/// 购买/兑换在官网 redeem 页（兑换码或爱发电订单号 → 选设备赋予）；
/// 换设备每月 1 次，也在官网操作。客户端零密钥，判定全在服务端。
fn cmd_redeem(args: &[&str], mox_proot_root: &Path) -> String {
    match args.first().copied() {
        None | Some("login") => match redeem::login(mox_proot_root) {
            Ok(o) => {
                let mut out = String::new();
                match o.opened {
                    redeem::Opened::Via(via) => {
                        out.push_str(&format!("已通过 {} 打开官网设备绑定页：\n  {}\n", via, o.url))
                    }
                    redeem::Opened::Failed => out.push_str(&format!(
                        "未能自动唤起浏览器，请手动打开：\n  {}\n",
                        o.url
                    )),
                }
                out.push_str(&format!(
                    "\n页面步骤：登录（GitHub）→ 点「绑定此设备」→ 自动回到本终端。\n\
                     ✅ 设备已绑定（{}，{}）。\n\
                     \x20 mox redeem status                  # 查看/刷新商品授权\n\
                     \x20 购买后在官网 redeem 页选本设备兑换（兑换码或爱发电订单号）",
                    o.device.name, o.device.platform
                ));
                out
            }
            Err(e) => format!("redeem: {}\n（重试：mox redeem login）", e),
        },
        Some("status") => match redeem::refresh_ents(mox_proot_root) {
            Ok(ents) => {
                let mut out = String::from("商品授权（已联网刷新）：\n");
                if ents.is_empty() {
                    out.push_str("  （无授权——未购买/未兑换，或授权已吊销）\n");
                } else {
                    for e in &ents {
                        out.push_str(&format!("  {}  档位 {}  {}\n", e.product_id, e.tier, exp_text(e.exp)));
                    }
                }
                if let Some(d) = redeem::load_device(mox_proot_root) {
                    out.push_str(&format!(
                        "\n设备：{}（{}）\n设备文件：{}\n",
                        d.name,
                        d.platform,
                        redeem::device_file_path(mox_proot_root).display()
                    ));
                }
                out.push_str(&format!(
                    "授权缓存：{}\n（超过 7 天未刷新门禁显示 🔒；换设备每月 1 次，官网 redeem 页操作）",
                    redeem::ents_file_path(mox_proot_root).display()
                ));
                out
            }
            Err(e) => {
                // 联网失败不空手：有缓存就摆出来（标注可能过期），并提示重试。
                let mut out = format!("商品授权：联网刷新失败（{}）\n", e);
                match redeem::load_ents(mox_proot_root) {
                    Some(c) if !c.ents.is_empty() => {
                        out.push_str("本地缓存授权（可能已过期，恢复网络后刷新）：\n");
                        for e in &c.ents {
                            out.push_str(&format!("  {}\n", e.product_id));
                        }
                    }
                    _ => out.push_str("本地也没有授权缓存。\n"),
                }
                out.push_str("重试：mox redeem status");
                out
            }
        },
        Some("check") => {
            if !redeem::ents_fresh(mox_proot_root) {
                return "商品授权：无有效缓存\n  mox redeem status    # 联网刷新授权".to_string();
            }
            let c = redeem::load_ents(mox_proot_root).unwrap_or_default();
            let mut out = String::from("商品授权（本地缓存，离线只读）：\n");
            if c.ents.is_empty() {
                out.push_str("  （无）\n");
            } else {
                for e in &c.ents {
                    out.push_str(&format!(
                        "  {}  档位 {}  {}\n",
                        e.product_id,
                        e.tier,
                        exp_text(e.exp)
                    ));
                }
            }
            out.push_str("（刷新：mox redeem status）");
            out
        }
        Some(other) => format!(
            "redeem: 未知子命令 {}\n用法: mox redeem login|status|check",
            other
        ),
    }
}

/// 授权到期的人类可读文本（exp 0 = 永久）。
fn exp_text(exp: u64) -> String {
    if exp == 0 {
        "永久".to_string()
    } else {
        format!("unix {} 过期", exp)
    }
}

/// `list`：已装/可装一览。
fn cmd_list(mgr: &DistroManager) -> String {
    let installed = mgr.list_installed().unwrap_or_default();
    let mut out = String::from("已安装发行版：\n");
    if installed.is_empty() {
        out.push_str("  （无）\n");
    }
    for d in &installed {
        out.push_str(&format!(
            "  {:<12} {}\n",
            d.id,
            d.rootfs.display()
        ));
    }
    out.push_str("\n可安装（proot-distro install <id>）：\n");
    for s in crate::proot::distro::builtin_specs() {
        let mark = if installed.iter().any(|d| d.id == s.id) { "（已安装）" } else { "" };
        out.push_str(&format!(
            "  {:<12} {:<8} {}{} 约{}MB\n",
            s.id, s.version, s.name, mark, s.size_mb
        ));
    }
    out
}

/// `install <id> [--cache <tar>] [--version <v>]`：安装（缓存归档优先，否则内部下载兜底）。
///
/// `--version` 校验必须落在 [`DistroSpec::versions`] 清单内（T3）；
/// 当前仅默认版本接入下载 URL，非默认版本在选择时被记录并提示。
fn cmd_install(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro install <id> [--cache <tar>] [--version <v>]".into();
    };
    let spec = match DistroManager::find_spec(id) {
        Ok(s) => s,
        Err(e) => return format!("proot-distro: {}", e),
    };
    // --cache <path>：指定外部已下载归档；--version <v>：选择版本（须清单内）。
    let mut cache: Option<PathBuf> = None;
    let mut version: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "--cache" => cache = it.next().map(PathBuf::from),
            "--version" => version = it.next().map(|s| s.to_string()),
            _ => {}
        }
    }
    // 版本校验：给出的值必须在清单内（否则提示可用项）。
    if let Some(v) = &version {
        if !spec.versions.iter().any(|x| *x == v) {
            return format!(
                "proot-distro: 版本 {} 不在 {} 的可选清单（{}）",
                v,
                spec.id,
                spec.versions.join(" / ")
            );
        }
    }
    // 等待区内容池：低频出现、非 TTY/CI 自动禁用、当日不重复（手册 §9）。
    // 只写 stderr，绝不混入 --json 的 stdout 数据流。
    if let Some(line) = crate::proot::delight::pick_line(
        crate::proot::delight::ContentScene::DownloadWait,
    ) {
        eprintln!("  {}", line);
    }
    let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
        install_progress(msg, done, total);
    });
    match mgr.install(&spec, cache.as_deref(), &mut progress) {
        Ok(()) => {
            let mut h = install_hint(spec.id, mgr);
            if let Some(v) = &version {
                h.push_str(&format!("已选版本：{}（默认：{}）\n", v, spec.version));
            }
            h
        }
        Err(e) => format!("{} {}", paint("proot-distro: 安装失败:", C_RED), e),
    }
}

/// `versions <id>`：列出该发行版的可选版本（T3）。
///
/// 与 `DistroSpec::versions` 同源；默认版本标「（默认）」。
fn cmd_versions(args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro versions <id>".into();
    };
    match DistroManager::find_spec(id) {
        Ok(spec) => {
            let mut out = format!("发行版 {} 可选版本：\n", spec.id);
            for v in spec.versions {
                let mark = if *v == spec.version {
                    paint("（默认）", C_GREEN).as_str().to_string()
                } else {
                    String::new()
                };
                out.push_str(&format!("  {:<24} {}{}\n", v, spec.name, mark));
            }
            out
        }
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// 最小 JSON 字符串转义（零依赖；只处理脚本会解析到的控制字符）。
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

/// `--json list`：机器可读，stdout 纯净（绝不上色、绝不放诗句）。
fn cmd_list_json(mgr: &DistroManager) -> String {
    let installed = mgr.list_installed().unwrap_or_default();
    let mut out = String::from("{\"installed\":[");
    for (i, d) in installed.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&format!(
            "{{\"id\":\"{}\",\"rootfs\":\"{}\",\"ready\":{}}}",
            json_escape(&d.id),
            json_escape(&d.rootfs.display().to_string()),
            d.initialized
        ));
    }
    out.push_str("],\"available\":[");
    for (i, s) in crate::proot::distro::builtin_specs().iter().enumerate() {
        if i > 0 { out.push(','); }
        let ready = installed.iter().any(|d| d.id == s.id && d.initialized);
        out.push_str(&format!(
            "{{\"id\":\"{}\",\"name\":\"{}\",\"version\":\"{}\",\"size_mb\":{},\"installed\":{}}}",
            json_escape(s.id), json_escape(s.name), json_escape(s.version), s.size_mb, ready
        ));
    }
    out.push_str("]}");
    out
}

/// `--json versions <id>`：版本清单 + 默认标记，无 ANSI。
fn cmd_versions_json(args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "{\"error\":\"missing_id\"}".to_string();
    };
    match DistroManager::find_spec(id) {
        Ok(spec) => {
            let vs: Vec<String> = spec.versions.iter().map(|v| format!("\"{}\"", json_escape(v))).collect();
            format!(
                "{{\"id\":\"{}\",\"default\":\"{}\",\"versions\":[{}]}}",
                json_escape(spec.id), json_escape(spec.version), vs.join(",")
            )
        }
        Err(e) => format!("{{\"error\":\"{}\"}}", json_escape(&e.to_string())),
    }
}

/// 安装后的收尾（换源提示 + 落盘启动器 + shell 函数刷新，0.2.0）。
fn install_hint(id: &str, mgr: &DistroManager) -> String {
    let repo = match id {
        "ubuntu" | "debian" => "sed -i 's|deb.debian.org\\|archive.ubuntu.com|mirrors.tuna.tsinghua.edu.cn|g' /etc/apt/sources.list*",
        "kali" => "sed -i 's|http.kali.org|mirrors.ustc.edu.cn|g' /etc/apt/sources.list",
        "alpine" => "sed -i 's|dl-cdn.alpinelinux.org|mirrors.tuna.tsinghua.edu.cn|g' /etc/apk/repositories",
        _ => "",
    };
    let mut out = String::from("安装完成。\n");
    if !repo.is_empty() {
        out.push_str(&format!(
            "提示：国内网络建议切换镜像源（登录后执行）：\n  {}\n",
            repo
        ));
    }
    // 落盘启动器 + shell 函数（roadmap §1：装了 kali 输 kali 就进容器）。
    // 失败仅警告——启动器是便利层，不该阻断安装本身。
    let bin_dir = launcher::default_bin_dir();
    match launcher::write_launcher(&bin_dir, id) {
        Ok(p) => out.push_str(&format!("启动器已就绪：直接输入 {} 即可进容器（{}）\n", id, p.display())),
        Err(e) => out.push_str(&format!(
            "{} 启动器写入失败（{}），可用 mox-proot-distro launch mx:{} 进入\n",
            paint("警告：", C_YELLOW), e, id
        )),
    }
    // 容器内控制台入口 mox（roadmap §3：固定命令，不开放自定义）。
    // 与启动器同目录落盘；失败仅警告（老安装无 mox 也有 launch/login）。
    match console::write_console_script(&bin_dir) {
        Ok(p) => out.push_str(&format!(
            "容器内控制台已就绪：进容器后输入 {} 即可管理（{}）\n",
            console::CONSOLE_CMD,
            p.display()
        )),
        Err(e) => out.push_str(&format!(
            "{} 控制台脚本写入失败（{}），容器内可用 mox-proot-distro 排障\n",
            paint("警告：", C_YELLOW), e
        )),
    }
    // 函数块覆盖全部已装容器（旧块整块替换，幂等）。
    let ids: Vec<String> = mgr
        .list_installed()
        .unwrap_or_default()
        .into_iter()
        .map(|d| d.id)
        .collect();
    let mut injected = Vec::new();
    for rc in launcher::shell_rc_paths() {
        if launcher::inject_shell_functions(&rc, &ids).unwrap_or(false) {
            injected.push(rc.display().to_string());
        }
    }
    if !injected.is_empty() {
        out.push_str(&format!(
            "shell 函数已刷新（{}）：新开终端或 source 后，输入 {} 直接进容器\n",
            injected.join("、"),
            id
        ));
    }
    out
}

/// `wizard [--root <目录>] [--distro <id>]`：一次性初始化向导（0.2.0，roadmap §5）。
///
/// 默认值驱动、**无 stdin 阻塞**（脚本/CI 友好；交互式问答是 0.4.0 TUI 的事）：
///   ① 数据根：`--root` 或保持现值（默认 `~/.mox-proot`）→ upsert 进 config.toml；
///   ② 发行版：`--distro`（默认 alpine）→ 已装跳过，未装走完整安装链路；
///   ③ 教学提示：换源 / 中文 / 控制台 / 启动器一次说清。
/// 幂等：重复跑只补缺失步骤，不重复下载、不重复提问。
fn cmd_wizard(args: &[&str]) -> String {
    let mut root_arg: Option<String> = None;
    // 默认 ubuntu（glibc 系，兼容性最好）；不再默认 alpine——
    // alpine 是 musl rootfs，proot 在 musl 宿主/特定环境下有坑（doc §A#6）。
    let mut distro = "ubuntu".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--root" => match args.get(i + 1) {
                Some(v) => {
                    root_arg = Some(v.to_string());
                    i += 2;
                }
                None => {
                    return "用法: mox-proot-distro wizard [--root <目录>] [--distro <id>]（--root 缺参数）"
                        .to_string()
                }
            },
            "--distro" => match args.get(i + 1) {
                Some(v) => {
                    distro = v.to_string();
                    i += 2;
                }
                None => {
                    return "用法: mox-proot-distro wizard [--root <目录>] [--distro <id>]（--distro 缺参数）"
                        .to_string()
                }
            },
            other => {
                return format!(
                    "wizard: 未知参数 {}\n用法: mox-proot-distro wizard [--root <目录>] [--distro <id>]",
                    other
                )
            }
        }
    }

    // P1：distro id 路径白名单。install_hint 会拿 id 拼启动器/控制台文件名
    // （`bin/<id>`），不校验会把 `../evil` 写出 bin 目录。
    if !DistroManager::valid_id(&distro) {
        return format!(
            "wizard: 非法发行版 id {}（仅允许字母数字与 . _ -，且不以 . 开头）",
            distro
        );
    }

    let mut out = String::from("mox-proot 初始化向导\n\n");

    // 1) 数据根：upsert 进 config.toml（保留用户其余配置）。
    let cfg_path = Settings::user_path();
    let settings = Settings::load_user().unwrap_or_default();
    let current = settings.resolve_root_dir();
    let target = root_arg
        .as_deref()
        .map(settings::expand_tilde)
        .unwrap_or_else(|| current.clone());
    let target_disp = target.display().to_string();
    let existing = std::fs::read_to_string(&cfg_path).unwrap_or_default();
    let new_text = settings::upsert_general_root_dir(&existing, &target_disp);
    if new_text != existing {
        if let Some(parent) = cfg_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&cfg_path, &new_text) {
            Ok(()) => out.push_str(&format!(
                "① 数据根目录：{}（已写入 {}）\n",
                target_disp,
                cfg_path.display()
            )),
            Err(e) => out.push_str(&format!(
                "① 警告：配置写入失败（{}），数据根仍为 {}\n",
                e,
                current.display()
            )),
        }
    } else {
        out.push_str(&format!("① 数据根目录：{}（配置未变）\n", target_disp));
    }

    // 2) 发行版：按目标数据根管理（config 可能刚改，mgr 要按新 root 建）。
    let mgr = DistroManager::new(&target);
    if mgr.distro_rootfs(&distro).is_dir() {
        out.push_str(&format!("② 发行版 {}：已安装，跳过\n", distro));
    } else {
        match DistroManager::find_spec(&distro) {
            Ok(spec) => {
                out.push_str(&format!(
                    "② 安装发行版 {}（{} {}）…\n",
                    spec.id, spec.name, spec.version
                ));
                let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
                    install_progress(msg, done, total);
                });
                match mgr.install(&spec, None, &mut progress) {
                    Ok(()) => out.push_str(&format!("② {} 安装完成\n", spec.id)),
                    Err(e) => {
                        return format!("{}② 安装失败：{}\n", out, e);
                    }
                }
            }
            Err(e) => {
                out.push_str(&format!("② 未知发行版 {}（{}），跳过安装\n", distro, e));
            }
        }
    }

    // 3) 落盘启动器 + 控制台 + 教学提示（install_hint 复用同一套）。
    out.push_str(&str::replace(&install_hint(&distro, &mgr), "安装完成。\n", ""));
    out.push_str(&format!(
        "\n接下来：\n  mox-proot-distro launch mx:{d}    # 直接进容器\n  {d}                                  # 启动器（已落盘）\n  mox-proot-distro env {d}            # 查中文环境\n  容器内输入 mox                        # 管理控制台（固定命令）\n",
        d = distro
    ));
    out
}

/// `login <id>`：输出可执行的 proot 命令行（由 PTY 层执行）。
/// `login <id|mx:名字>`：生成进容器命令串（供 App 终端会话的 PTY 直接执行）。
///
/// 与二进制 `exec_login` 互通：既接受普通发行版 id，也接受 `mx:` 短链
/// （解析成发行版 id 后走同一条 [`DistroManager::login_cmd`] 通道）。
fn cmd_login(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(spec) = args.first() else {
        return "用法: proot-distro login <id|mx:名字>".into();
    };
    let id = if links::is_mx(spec) {
        match links::resolve(spec, &links::load_user_links().unwrap_or_default()) {
            Ok(ResolvedLink::Distro(id)) => id,
            Ok(ResolvedLink::Url(_)) => {
                return format!("proot-distro: 自定义短链 {} 需先 install 再 login", spec)
            }
            Err(e) => return format!("proot-distro: {}", e),
        }
    } else {
        (*spec).to_string()
    };
    match mgr.login_cmd(&id, None) {
        Ok(cmd) => cmd,
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `shell <id|mx:名字>`：非登录 shell 快捷方式（等价 login 但用 /bin/sh）。
///
/// 与 [`cmd_login`] 同源：支持普通 id 与 `mx:` 短链。
fn cmd_shell(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(spec) = args.first() else {
        return "用法: proot-distro shell <id|mx:名字>".into();
    };
    let id = if links::is_mx(spec) {
        match links::resolve(spec, &links::load_user_links().unwrap_or_default()) {
            Ok(ResolvedLink::Distro(id)) => id,
            Ok(ResolvedLink::Url(_)) => {
                return format!("proot-distro: 自定义短链 {} 需先 install 再 shell", spec)
            }
            Err(e) => return format!("proot-distro: {}", e),
        }
    } else {
        (*spec).to_string()
    };
    // 与 login 相同骨架，仅 shell 换成非登录 /bin/sh（通过 shell_override 传参，
    // 不再用 rfind 字符串手术，避免 /usr/bin/zsh、--bind 含 /bin/ 等场景错位）。
    match mgr.login_cmd(&id, Some("/bin/sh")) {
        Ok(cmd) => cmd,
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `remove <id>`：删除。
fn cmd_remove(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro remove <id>".into();
    };
    // 危险操作二次确认（doc §A#7/#17）：未带 --yes/-y 时拒绝执行，
    // 避免手滑误删整个 rootfs（数据不可恢复）。
    let confirmed = args.iter().any(|a| *a == "--yes" || *a == "-y");
    if !confirmed {
        let rootfs = mgr.distro_rootfs(id);
        return format!(
            "⚠ 删除确认：将删除发行版 {}（{}），不可恢复。\n   确认请执行：proot-distro remove {} --yes",
            id, rootfs.display(), id
        );
    }
    match mgr.remove(id) {
        Ok(()) => format!("已删除 {}", id),
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `backup <id> <out.tar.gz>`：备份。
fn cmd_backup(mgr: &DistroManager, args: &[&str]) -> String {
    let (Some(id), Some(out)) = (args.first(), args.get(1)) else {
        return "用法: proot-distro backup <id> <out.tar.gz>".into();
    };
    match mgr.backup(id, Path::new(out)) {
        Ok(()) => {
            // 【体验修复】单独一行打印绝对路径与大小，用户不用再 find / du。
            let p = Path::new(out);
            let abs = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            format!(
                "✓ 已备份 {}\n  归档：{}\n  大小：{:.2} MiB（{} 字节）",
                id,
                abs.display(),
                size as f64 / 1_048_576.0,
                size
            )
        }
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// UTC 秒 → `YYYY-MM-DD HH:MM UTC`（civil-from-days，避免为此引入 chrono）。
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let sod = secs % 86_400;
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        year,
        m,
        d,
        sod / 3600,
        (sod % 3600) / 60
    )
}

/// `restore <id> <tar>`：恢复。
fn cmd_restore(mgr: &DistroManager, args: &[&str], _mox_proot_root: &Path) -> String {
    let (Some(id), Some(tar)) = (args.first(), args.get(1)) else {
        return "用法: proot-distro restore <id> <tar.tar.gz>".into();
    };
    match mgr.restore(id, Path::new(tar)) {
        Ok(()) => {
            // 【体验修复】恢复到「哪一天哪一个备份」要明确，避免多份归档时搞混。
            let when = std::fs::metadata(tar)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| utc_stamp(d.as_secs()));
            match when {
                Some(ts) => format!("✓ 已恢复 {} 到 {} 的备份（归档 {}）", id, ts, tar),
                None => format!("✓ 已恢复 {}（归档 {}）", id, tar),
            }
        }
        Err(e) => format!("proot-distro: {}", e),
    }
}

// ==================== mx: 短链启动（0.2.0，docs/roadmap.md §1） ====================

/// 启动核心：解析 `mx:xxx` 或普通发行版 id → 未装自动装 → 起引擎进容器。
///
/// 两套登录入口在此汇合（roadmap §1，呼应文心手册"登录融合成一个"诉求）：
/// - `mox-proot -r mx:alpine -- bash`（引擎侧选项值 scheme）；
/// - `mox-proot-distro launch mx:alpine`（短链子命令）；
/// - `mox-proot-distro login ubuntu` / `shell ubuntu`（传统 id，与 launch 互通）；
/// - `mox-proot-distro launch ubuntu`（普通 id 也放行，等价于 mx: 短链）。
///
/// 无论哪种入口，最终都走「解析 → 未装自动装 → exec_into」，行为完全一致。
/// 返回进程退出码。
pub fn launch_spec(root: &Path, spec: &str, tail: &[String]) -> ProotResult<i32> {
    let mgr = DistroManager::new(root);
    // 兼容两种形态：mx: 短链（解析 + 自定义 rootfs）或 普通发行版 id。
    let (id, custom_urls): (String, Option<Vec<String>>) = if links::is_mx(spec) {
        let user = links::load_user_links().unwrap_or_default();
        match links::resolve(spec, &user)? {
            ResolvedLink::Distro(id) => (id, None),
            ResolvedLink::Url(urls) => (links::strip_mx(spec).unwrap_or("").to_string(), Some(urls)),
        }
    } else {
        mgr.check_id(spec)?; // P1：id 白名单（防路径穿越，与 DistroManager 同源）
        (spec.to_string(), None)
    };

    let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
        install_progress(msg, done, total);
    });
    if !mgr.distro_rootfs(&id).is_dir() {
        if let Some(urls) = custom_urls {
            let tar = download_first_available(&mgr, &urls)?;
            mgr.install_custom(&id, &tar, None, &mut progress)?;
        } else {
            let specd = DistroManager::find_spec(&id)?;
            eprintln!("mox-proot: {} 未安装，开始自动安装（{} {}）…", id, specd.name, specd.version);
            mgr.install(&specd, None, &mut progress)?;
        }
    }
    exec_into(root, &id, tail)
}

/// URL 型短链下载：按序尝试多源，全部失败报 `DownloadRequired`。
fn download_first_available(mgr: &DistroManager, urls: &[String]) -> ProotResult<PathBuf> {
    fs_create_dir_all(&mgr.cache_dir())?;
    for url in urls {
        // 资产名取 URL 末段（去 query）；重名缓存直接复用。
        let fname = url
            .rsplit('/')
            .next()
            .unwrap_or("rootfs.tar")
            .split('?')
            .next()
            .unwrap_or("rootfs.tar");
        let fname = if fname.is_empty() { "rootfs.tar" } else { fname };
        let out = mgr.cache_dir().join(fname);
        if out.exists() {
            return Ok(out);
        }
        eprintln!("[下载] {}", url);
        // 与 rootfs 下载共用续传实现：.part 保留、Range 续传、停滞检测。
        let part = mgr.cache_dir().join(format!("{}.part", fname));
        let mut nop = |_m: &str, _d: u64, _t: u64| {};
        if crate::proot::distro::curl_fetch(url, &part, &mut nop, "短链", 0).unwrap_or(false) {
            std::fs::rename(&part, &out)?;
            return Ok(out);
        }
        // 失败保留 .part 供下次续传，但清掉可能被误判为完整的正式文件
        let _ = std::fs::remove_file(&out);
    }
    Err(ProotError::DownloadRequired(urls.first().cloned().unwrap_or_default()))
}

fn fs_create_dir_all(p: &Path) -> ProotResult<()> {
    std::fs::create_dir_all(p).map_err(ProotError::Io)
}

/// 起引擎进容器（提炼自 `mox-proot-distro login/shell` 的 exec 路径，
/// 叠加 0.2.0 的进/退命令语义，roadmap §1）：
///
/// - `tail` 非空 = 用户显式命令（`mx:alpine -- vim`），替代登录 shell；
/// - 否则 `enter_cmd(id)`（每容器覆盖 > 全局默认）= 容器内首个命令
///   （`/bin/sh -c` 执行，防误触确认写在这里面）；
/// - 再否则引擎默认探测：bash -l 优先，sh -l 兜底；
/// - 会话结束后 `exit_cmd(id)` 在**宿主侧**执行（收尾提示/清理）。
///
/// `login`/`shell`/`launch` 在 `--` 之前可追加的引擎选项。
///
/// 【渗透复测 P1 修复】旧实现只取 `--` 之后的命令，`login <id> -b /sdcard:/sdcard`
/// 里的绑定被**静默丢弃**——用户以为挂载生效，实际没有，属于危险的认知错觉。
/// 现在把这些选项解析成结构化覆盖，真正注入 `ProotConfig`；遇到不支持的
/// 选项则明确报错（而非静默忽略）。
#[derive(Debug, Clone, Default)]
pub struct ExecIntoOpts {
    /// 追加绑定 (guest, host)。
    pub binds: Vec<(String, String)>,
    /// 覆盖容器内初始工作目录。
    pub cwd: Option<String>,
    /// 追加环境变量 KEY=VALUE。
    pub env: Vec<(String, String)>,
    /// 诊断级别（`-v` 累加）。
    pub verbose: u8,
    /// fake root 显式开关（`-0`）。
    pub fake_root: Option<bool>,
}

/// 解析 `--` 之前的引擎选项；不支持的选项直接返回错误文案。
pub fn parse_engine_opts(args: &[String]) -> Result<ExecIntoOpts, String> {
    let mut o = ExecIntoOpts::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-b" | "--bind" | "-m" | "--mount" => {
                i += 1;
                let v = args.get(i).ok_or("-b 缺少参数（期望 host:guest）")?;
                o.binds.push(parse_binding(v)?);
            }
            "-w" | "--pwd" | "--cwd" => {
                i += 1;
                o.cwd = Some(args.get(i).ok_or("-w 缺少参数")?.clone());
            }
            "-e" | "--env" => {
                i += 1;
                let v = args.get(i).ok_or("-e 缺少参数")?;
                let (k, val) = v.split_once('=').ok_or("-e 需要 KEY=VALUE 形式")?;
                if k.is_empty() {
                    return Err("-e 的 KEY 不能为空".to_string());
                }
                o.env.push((k.to_string(), val.to_string()));
            }
            "-v" | "--verbose" => o.verbose = o.verbose.saturating_add(1),
            "-0" | "--root-id" => o.fake_root = Some(true),
            other => {
                return Err(format!(
                    "login/shell 不支持选项 {}（可用 -b/-w/-e/-v/-0，或放到 `--` 之后作为容器内命令）",
                    other
                ))
            }
        }
        i += 1;
    }
    Ok(o)
}

pub fn exec_into(root: &Path, id: &str, tail: &[String]) -> ProotResult<i32> {
    exec_into_with_opts(root, id, tail, &ExecIntoOpts::default())
}

/// 带引擎选项覆盖的 `exec_into`（`--` 之前的 -b/-w/-e/-v 真正生效）。
pub fn exec_into_with_opts(
    root: &Path,
    id: &str,
    tail: &[String],
    opts: &ExecIntoOpts,
) -> ProotResult<i32> {
    let mgr = DistroManager::new(root);
    let rootfs = mgr.distro_rootfs(id);
    if !rootfs.is_dir() {
        return Err(ProotError::DistroNotInstalled(id.to_string()));
    }
    // pre-login 钩子：失败仅警告不阻断（钩子是用户脚本，不该把人挡在容器外）。
    match script::run_hooks(root, &rootfs, id, script::HookKind::PreLogin) {
        Ok(r) if r.has_failure() => {
            eprintln!("mox-proot: pre-login 钩子存在失败项：{}", r.summary())
        }
        Ok(_) => {}
        Err(e) => eprintln!("mox-proot: pre-login 钩子异常：{}", e),
    }

    // 命令三源：显式 tail > 配置 enter > 引擎默认探测。
    let shell: Vec<String> = if !tail.is_empty() {
        tail.to_vec()
    } else if let Some(enter) = Settings::load_user()
        .ok()
        .as_ref()
        .and_then(|s| s.enter_cmd(id).map(str::to_string))
    {
        vec!["/bin/sh".into(), "-c".into(), enter]
    } else if rootfs.join("bin/bash").exists() {
        vec!["/bin/bash".into(), "-l".into()]
    } else {
        vec!["/bin/sh".into(), "-l".into()]
    };

    let mut cfg = ProotConfig::with_distro_defaults(&rootfs);
    // 【P1】`--` 之前的 -b/-w/-e/-v/-0 真正注入引擎配置（旧实现静默丢弃）。
    // 绑定「后加优先」→ 放在控制台绑定之后，保证用户 -b 覆盖默认绑定。
    // 容器内控制台：bind 宿主 bin + MOX_ROOT，注入 mox 固定命令
    // （roadmap §3；MOX_PROOT_CONSOLE=0 可关）。
    console::bind_console(&mut cfg, root, &console::host_bin_dir());
    for (guest, host) in &opts.binds {
        cfg.bind(guest, host);
    }
    if let Some(cwd) = &opts.cwd {
        cfg.cwd = cwd.clone();
    }
    for (k, v) in &opts.env {
        cfg.env.push((k.clone(), v.clone()));
    }
    if opts.verbose > 0 {
        cfg.verbose = cfg.verbose.max(opts.verbose);
    }
    if let Some(fr) = opts.fake_root {
        cfg.fake_root = fr;
    }
    // env_command() 形如 ["/usr/bin/env", "-i", "HOME=...", ...]，跳过前两个
    // 前缀词即纯 KEY=VALUE 列表（spawn 直接 execve 用）。用户 -e 已在上面
    // 追加到 cfg.env，env_command 会按「用户显式 -e 最高」合入。
    let env: Vec<String> = cfg.env_command().into_iter().skip(2).collect();
    let code = run_config(&cfg, &shell, &env);

    // 宿主侧收尾（exit 语义降级说明见 settings.rs 模块文档）。
    if let Ok(s) = Settings::load_user() {
        if let Some(exit) = s.exit_cmd(id) {
            let _ = std::process::Command::new("sh").arg("-c").arg(exit).status();
        }
    }
    Ok(code)
}

/// 在参数表里找 `mx:` 形式的 rootfs 值（-r/-R/-S 的后一个词，或 --rootfs=）。
/// 找到即短链启动路径（`mox-proot -r mx:alpine`）。
pub fn find_mx_rootfs_arg(args: &[String]) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let v = if matches!(a, "-r" | "--rootfs" | "-R" | "-S") {
            args.get(i + 1).map(String::as_str)
        } else {
            a.strip_prefix("--rootfs=")
        };
        if let Some(v) = v {
            if links::is_mx(v) {
                return Some(v.to_string());
            }
        }
        i += 1;
    }
    None
}

/// 提取 `--` 之后的用户命令（短链启动的 tail）。
pub fn tail_after_dashdash(args: &[String]) -> Vec<String> {
    args.iter()
        .position(|a| a == "--")
        .map(|p| args[p + 1..].to_vec())
        .unwrap_or_default()
}

/// `proot` 原生参数面：识别 -r/-0/-i/-H/-v/-b/-w 并输出等效 mox-proot 引擎配置预览。
fn proot_native(args: &[&str], _mox_proot_root: &Path) -> String {
    let mut rootfs: Option<String> = None;
    let mut fake_root = false;
    let mut fake_id: Option<String> = None;
    let mut verbose = 0u8;
    let mut hidden: Vec<String> = Vec::new();
    let mut cwd = "/".to_string();
    let mut binds: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "-r" | "--rootfs" => rootfs = it.next().map(|s| s.to_string()),
            "-0" | "--root-id" => fake_root = true,
            "-i" | "--fake-id" => fake_id = it.next().map(|s| s.to_string()),
            "-H" | "--hide" => {
                if let Some(h) = it.next() {
                    hidden.push(h.to_string());
                }
            }
            "-v" | "--verbose" => verbose = verbose.saturating_add(1),
            "-w" | "--cwd" => cwd = it.next().map(|s| s.to_string()).unwrap_or_else(|| "/".into()),
            "-b" | "--bind" => {
                if let Some(b) = it.next() {
                    binds.push(b.to_string());
                }
            }
            "-h" | "--help" => return PROOT_HELP.to_string(),
            other => return format!("proot: 暂不支持参数 {}（mox-proot 引擎内嵌，UI 启动或 proot-distro login）", other),
        }
    }
    let Some(rootfs) = rootfs else {
        return "用法: proot -r <rootfs> [-0] [-b host:guest]... [-w /dir] -- cmd".into();
    };
    let mut out = format!(
        "mox-proot proot 引擎配置预览：\n  rootfs: {}\n  fake_root(-0): {}\n  fake_id(-i): {:?}\n  verbose(-v): {}\n  hide(-H): {:?}\n  cwd(-w): {}\n  绑定(-b):\n",
        rootfs, fake_root, fake_id, verbose, hidden, cwd
    );
    for b in binds {
        out.push_str(&format!("    {}\n", b));
    }
    out.push_str("（实际启动：mox-proot 图形界面选择发行版，或 proot-distro login <id>）");
    out
}

/// proot-distro 帮助文本（对齐上游子命令集）。
const HELP: &str = "proot-distro（mox-proot 兼容实现）
用法:
  proot-distro list                      列出已安装与可安装发行版
  proot-distro install <id> [--cache <tar>]   安装（--cache 指定已下载归档）
  proot-distro login <id|mx:名字>        进入发行版登录 shell（id 与短链互通）
  proot-distro shell <id|mx:名字>        进入发行版非登录 shell
  proot-distro remove <id>               删除发行版
  proot-distro backup <id> <out.tar.gz>  备份
  proot-distro restore <id> <tar.tar.gz> 恢复
  proot-distro net [id]                  网络边界报告（不带 id 看宿主）
  proot-distro env <id>                  预览容器登录时注入的环境（中文排查）
  proot-distro launch <mx:名字|id> [-- cmd...]  启动（未装自动装，id 与短链互通）
  proot-distro recipe list|show <id>|apply <id> <distro>  一键配方（桌面/VNC/noVNC）
  proot-distro fan login|redeem <code>|status   粉丝专属解锁（灰度中，默认隐藏）
  proot-distro redeem login|status|check       商品授权：绑定设备 + 在线校验
  proot-distro access status|sync             统一访问面（粉丝+授权融合；粉丝默认隐藏）
  proot-distro time                         时间上下文问候 + 古诗（惊喜三件套）
  proot-distro egg                          隐藏彩蛋（惊喜三件套）
  proot-distro wizard [--root <目录>] [--distro <id>]  一次性初始化（数据根+发行版）
  --json                                    机器可读输出（list/versions，纯 stdout）
  --plain                                   关闭全部颜色与装饰

常用示例（先看这三个）：
  proot-distro list                         看看装了什么、还能装什么
  proot-distro install ubuntu               装一个 Ubuntu
  ubuntu                                    直接进容器
  proot-distro list --json | jq .available 给脚本用的结构化数据

说明：脚本里请加 --json；管道/重定向会自动关色，也可设 NO_COLOR=1。";

/// proot 原生参数帮助。
const PROOT_HELP: &str = "mox-proot — 在非 root 环境下运行 Linux 容器

常用命令（先看这几个）：
  mox-proot-distro list              列出可用/已装发行版
  mox-proot-distro install ubuntu    装一个发行版
  ubuntu                             直接进容器

proot 底层参数：
  -r <path>        指定 rootfs 路径
  -b <src>:<dst>   自定义绑定挂载（可多次）
  -w <path>        容器内工作目录（默认 /root）
  -R               ★推荐绑定集：自动挂 /dev /proc /sys /tmp + HOME，
                    几乎所有场景都用这个，新手默认选它
  -S               最小绑定集：只挂最必要的，用于调试
  -0 / --root-id   在容器内以 uid 0 运行
  -e / --env       追加环境变量
  -- <命令...>     把后续参数原样交给容器内执行";

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read as _, Write as _}; // redeem status stub 收发 HTTP 用

    #[test]
    fn json_list_is_machine_readable() {
        let out = proot_distro(&["--json", "list"], Path::new("/nonexistent"));
        assert!(out.starts_with("{\"installed\":["), "{}", out);
        assert!(out.contains("\"available\""));
        assert!(out.contains("\"id\":\"ubuntu\""));
        assert!(!out.contains("\x1b["), "--json 绝不能含 ANSI");
        assert!(!out.contains("——"), "--json 绝不能混入诗句");
        // 括号平衡（最小结构校验，不引 serde）
        assert_eq!(out.matches('{').count(), out.matches('}').count());
        assert_eq!(out.matches('[').count(), out.matches(']').count());
    }

    #[test]
    fn json_versions_has_default_marker() {
        let out = proot_distro(&["versions", "alpine", "--json"], Path::new("/tmp"));
        assert!(out.contains("\"default\":"));
        assert!(out.contains("3.20 (minirootfs)"));
        assert!(!out.contains("\x1b["));
    }

    #[test]
    fn plain_flag_does_not_break_human_output() {
        let out = proot_distro(&["--plain", "list"], Path::new("/nonexistent"));
        assert!(out.contains("（无）"));
        assert!(out.contains("ubuntu"));
    }

    #[test]
    fn unknown_subcommand_gives_next_step() {
        let out = proot_distro(&["instal"], Path::new("/tmp"));
        assert!(out.contains("怎么办"), "{}", out);
        assert!(out.contains("是不是想输入"), "应给相似建议：{}", out);
    }

    /// list 子命令：空目录输出"（无）"并列出可装清单。
    #[test]
    fn list_empty_shows_available() {
        let out = proot_distro(&["list"], Path::new("/nonexistent"));
        assert!(out.contains("（无）"));
        assert!(out.contains("ubuntu"));
        assert!(out.contains("alpine"));
    }

    /// 未知 id 安装报错文本。
    #[test]
    fn install_unknown_id() {
        let out = proot_distro(&["install", "windows"], Path::new("/tmp"));
        assert!(out.contains("未知发行版"));
    }

    /// versions 子命令：列出可选版本并标注默认。
    #[test]
    fn versions_lists_and_marks_default() {
        let out = proot_distro(&["versions", "ubuntu"], Path::new("/tmp"));
        assert!(out.contains("可选版本"), "{}", out);
        assert!(out.contains("24.04 LTS (base)"), "应含默认版本：{}", out);
        assert!(out.contains("22.04 LTS"), "应含可选版本：{}", out);
        assert!(out.contains("默认"), "应标注默认：{}", out);
    }

    /// 未知 id 的 versions 报错。
    #[test]
    fn versions_unknown_id() {
        let out = proot_distro(&["versions", "windows"], Path::new("/tmp"));
        assert!(out.contains("未知发行版"));
    }

    /// install --version 校验：不在清单内报错。
    #[test]
    fn install_invalid_version_rejected() {
        let out = proot_distro(
            &["install", "alpine", "--version", "9.99"],
            Path::new("/tmp"),
        );
        assert!(out.contains("不在"), "应提示版本不在清单：{}", out);
    }

    /// install --version 合法值落在清单内（纯成员判定，不触发下载）。
    #[test]
    fn versions_contains_valid_choice() {
        let spec = DistroManager::find_spec("alpine").unwrap();
        assert!(
            spec.versions.contains(&"3.19 (minirootfs)"),
            "清单应含可选版本：{:?}",
            spec.versions
        );
        assert!(spec.versions.contains(&spec.version), "清单应含默认版本");
    }

    /// login 未安装报错。
    #[test]
    fn login_not_installed() {
        let out = proot_distro(&["login", "ubuntu"], Path::new("/nonexistent"));
        assert!(out.contains("未安装"));
    }

    /// 登录融合：App 文本路由 `login mx:alpine` 应解析成发行版 alpine 再走
    /// login_cmd（与二进制 launch mx:alpine 同源），未装时给出 alpine 未安装。
    #[test]
    fn login_mx_shortlink_resolves() {
        let out = proot_distro(&["login", "mx:alpine"], Path::new("/nonexistent"));
        assert!(out.contains("未安装"), "mx: 未解析到 alpine：{}", out);
        assert!(out.contains("alpine"), "应点名 alpine：{}", out);
    }

    /// shell 同样支持 mx: 短链解析。
    #[test]
    fn shell_mx_shortlink_resolves() {
        let out = proot_distro(&["shell", "mx:ubuntu"], Path::new("/nonexistent"));
        assert!(out.contains("未安装"));
        assert!(out.contains("ubuntu"));
    }

    /// 登录融合：launch_spec 接受普通 id（与 login 互通），且非法 id 被
    /// check_id 白名单拦下（不触网、不越界）。
    #[test]
    fn launch_spec_plain_id_invalid_rejected() {
        let r = launch_spec(Path::new("/nonexistent"), "..", &[]);
        assert!(r.is_err(), "非法 id 应被 check_id 拦下");
    }

    /// launch_spec 普通合法 id（字符白名单通过）但非预置发行版：走到
    /// find_spec 直接报「未知发行版」，不触网下载（避免测试联网装包）。
    #[test]
    fn launch_spec_plain_id_unknown_rejected() {
        let r = launch_spec(Path::new("/nonexistent"), "zzz", &[]);
        assert!(r.is_err(), "未知 id 应被 find_spec 拦下");
        if let Err(e) = r {
            assert!(
                format!("{}", e).contains("未知发行版"),
                "应提示未知发行版而非尝试下载：{}",
                e
            );
        }
    }

    /// help 与未知子命令。
    #[test]
    fn help_and_unknown() {
        assert!(proot_distro(&["help"], Path::new("/x")).contains("proot-distro"));
        assert!(proot_distro(&["frobnicate"], Path::new("/x")).contains("未知子命令"));
    }

    /// time 子命令：应景问候 + 古诗（惊喜三件套，Rust 单一来源）。
    #[test]
    fn time_subcommand_shows_poem() {
        let out = proot_distro(&["time"], Path::new("/x"));
        assert!(out.contains("时相"), "应含时相：{}", out);
        assert!(out.contains("——"), "应含古诗出处：{}", out);
    }

    /// egg 子命令：命中暗号吐彩蛋文案。
    #[test]
    fn egg_subcommand_reveals() {
        let out = proot_distro(&["egg"], Path::new("/x"));
        assert!(out.contains("彩蛋"), "应含彩蛋文案：{}", out);
    }

    /// fan 子命令：灰度关时任何子命令都给灰度提示（默认关，不误触专属流）。
    ///
    /// 三个改 `MOX_FAN_ENABLE` 的用例共用一把测试锁串行化——env 是进程级
    /// 全局态，并行跑会互相覆盖（曾撞出「灰度关却进了 login 回环等 240s」）。
    static FAN_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn fan_grey_gate_closed_by_default() {
        let _g = FAN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(fan::FAN_ENABLE_ENV, "0");
        let out = proot_distro(&["fan", "login"], Path::new("/tmp"));
        assert!(out.contains("灰度"), "应提示灰度中：{}", out);
        assert!(!out.contains("127.0.0.1"), "灰度关不得起回环/给 URL");
        let out = proot_distro(&["fan", "status"], Path::new("/tmp"));
        assert!(out.contains("灰度"));
        std::env::remove_var(fan::FAN_ENABLE_ENV);
    }

    /// fan 未知子命令：给用法（灰度开时才看得到，先开开关）。
    #[test]
    fn fan_unknown_subcommand() {
        let _g = FAN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(fan::FAN_ENABLE_ENV, "1");
        let out = proot_distro(&["fan", "frobnicate"], Path::new("/tmp"));
        assert!(out.contains("未知子命令"));
        assert!(out.contains("mox fan login|redeem <code>|status"));
        std::env::remove_var(fan::FAN_ENABLE_ENV);
    }

    /// fan status：未解锁时提示 login 入口（灰度开）。
    #[test]
    fn fan_status_unlocked_hint() {
        let _g = FAN_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(fan::FAN_ENABLE_ENV, "1");
        let out = proot_distro(&["fan", "status"], Path::new("/nonexistent-root"));
        assert!(out.contains("未解锁"));
        assert!(out.contains("mox fan login"));
        std::env::remove_var(fan::FAN_ENABLE_ENV);
    }

    /// redeem 未知子命令：给用法（无灰度，随时可见）。
    #[test]
    fn redeem_unknown_subcommand() {
        let out = proot_distro(&["redeem", "frobnicate"], Path::new("/tmp"));
        assert!(out.contains("未知子命令"));
        assert!(out.contains("mox redeem login|status|check"));
    }

    /// redeem check：无缓存时提示联网刷新（离线路径，不打网络）。
    #[test]
    fn redeem_check_no_cache() {
        let out = proot_distro(&["redeem", "check"], Path::new("/nonexistent-redeem-root"));
        assert!(out.contains("无有效缓存"));
        assert!(out.contains("mox redeem status"));
    }

    /// redeem check：有新鲜缓存时离线展示授权（含「永久」文案）。
    #[test]
    fn redeem_check_reads_cache_offline() {
        let root = std::env::temp_dir().join(format!("mox-proot-redeem-check-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        // P0-1 后门禁 fail-closed：测试缓存也必须带本机设备身份 + 服务端签名，
        // 与真实 `mox redeem status` 落盘格式一致。
        redeem::testkit::seed_legit_cache(
            &root,
            &[redeem::Entitlement {
                product_id: "proot-pro".into(),
                tier: "pro".into(),
                exp: 0,
            }],
            redeem::testkit::SIG_PROOT_PRO,
        );
        let out = proot_distro(&["redeem", "check"], &root);
        assert!(out.contains("本地缓存"), "应标注离线只读：{}", out);
        assert!(out.contains("proot-pro"));
        assert!(out.contains("永久"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// redeem status：本地 HTTP stub 顶替 /entitlement/check，端到端验证
    /// curl 拉取 → 多行 ok 解析 → 落盘缓存 → 门禁生效（不打外网）。
    ///
    /// 与 fan 用例同理：env 是进程级全局态，用专用锁串行化。
    static REDEEM_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn redeem_status_end_to_end_with_local_stub() {
        let _g = REDEEM_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                // 与真实后端一致：清单 + 服务端 Ed25519 签名行（签名对应上面两行 + \n）。
                // 签名取自 redeem::testkit 单一来源，避免硬编码串与夹具漂移。
                let body = format!(
                    "ok proot-pro pro 0\nok mox-proot-plus plus 4102444800\nsig {}\n",
                    crate::proot::redeem::testkit::SIG_TWO
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = s.write_all(resp.as_bytes());
                let _ = s.flush();
            }
        });
        let root = std::env::temp_dir().join(format!("mox-proot-redeem-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var(
            redeem::ENTITLEMENT_API_ENV,
            format!("http://127.0.0.1:{}/entitlement/check", addr.port()),
        );
        let out = proot_distro(&["redeem", "status"], &root);
        std::env::remove_var(redeem::ENTITLEMENT_API_ENV);
        handle.join().unwrap();
        assert!(out.contains("已联网刷新"), "应报告联网刷新：{}", out);
        assert!(out.contains("proot-pro"));
        assert!(out.contains("mox-proot-plus"));
        assert!(out.contains("永久"));
        // 落盘缓存 → 门禁同源生效
        assert!(redeem::has_entitlement(&root, "proot-pro"));
        assert!(redeem::has_entitlement(&root, "mox-proot-plus"));
        assert!(!redeem::has_entitlement(&root, "no-such"));
        let _ = std::fs::remove_dir_all(&root);
    }

    /// proot 原生参数组装预览。
    #[test]
    fn proot_native_preview() {
        let out = route(
            "proot",
            &["-r", "/rf", "-0", "-b", "/dev:/dev", "-w", "/root"],
            Path::new("/x"),
        );
        assert!(out.contains("rootfs: /rf"));
        assert!(out.contains("fake_root(-0): true"));
        assert!(out.contains("cwd(-w): /root"));
        assert!(out.contains("/dev:/dev"));
    }

    /// -h/--help：哨兵空错误——run_native 据此走 stdout 用法 + 退出 0
    /// （安装脚本自检依赖此退出码语义）。
    #[test]
    fn parse_native_args_help_sentinel() {
        assert!(parse_native_args(&["--help".to_string()])
            .unwrap_err()
            .is_empty());
        assert!(parse_native_args(&["-h".to_string()])
            .unwrap_err()
            .is_empty());
    }

    /// -b 绑定语义：上游 `host:guest`（host 在前），parse_binding 返回
    /// `(guest, host)`——顺序错了会把宿主路径当 guest 挂，隔离直接反了。
    #[test]
    fn parse_binding_host_first() {        assert_eq!(
            parse_binding("/data/data/x:/rf").unwrap(),
            ("/rf".to_string(), "/data/data/x".to_string())
        );
        // 单路径形式（无冒号）：host 与 guest 同路径（上游默认语义）。
        assert_eq!(
            parse_binding("/proc").unwrap(),
            ("/proc".to_string(), "/proc".to_string())
        );
        // guest 尾缀 ! = 不反向还原：转成 "!guest" 前缀（translate 层 no-isolate）。
        assert_eq!(
            parse_binding("/host:/guest!").unwrap(),
            ("!/guest".to_string(), "/host".to_string())
        );
        // 空边非法。
        assert!(parse_binding(":/guest").is_err());
        assert!(parse_binding("/host:").is_err());
    }

    /// parse_native_args 全选项面：-R 推荐绑定 + -0 + -w + -e + `--` 命令切分。
    /// 依赖真实目录（rf.is_dir() 校验），用临时目录隔离。
    #[test]
    fn parse_native_args_full_surface() {
        let rf = std::env::temp_dir().join("mox-proot-test-rf");
        std::fs::create_dir_all(&rf).unwrap();
        let rf_s = rf.to_str().unwrap();

        // 缺 -r 报错（默认不假设 rootfs）。
        let err = parse_native_args(&["/bin/sh".to_string()]).unwrap_err();
        assert!(err.contains("缺少 -r"));

        // rootfs 不存在报错。
        let err = parse_native_args(&["-r".to_string(), "/no/such/dir".to_string()])
            .unwrap_err();
        assert!(err.contains("rootfs 目录不存在"));

        // -R + -0 + -w + -e + -- 命令切分。
        let na = parse_native_args(&[
            "-R".to_string(),
            rf_s.to_string(),
            "-0".to_string(),
            "-b".to_string(),
            "/storage/emulated/0:/sdcard".to_string(),
            "-w".to_string(),
            "/root".to_string(),
            "-e".to_string(),
            "TERM=xterm".to_string(),
            "--".to_string(),
            "/bin/sh".to_string(),
            "-c".to_string(),
            "echo hi".to_string(),
        ])
        .unwrap();
        assert_eq!(na.config.rootfs, rf);
        assert!(na.config.fake_root);
        assert_eq!(na.config.cwd, "/root");
        // 显式 -b 应叠加进绑定表（host:guest → (guest, host)）。
        assert!(
            na.config
                .bindings
                .iter()
                .any(|(g, h)| g == "/sdcard" && h == "/storage/emulated/0"),
            "显式 -b 应生效: {:?}",
            na.config.bindings
        );
        // -e 给了即清空继承：env 恰为所给一条。
        assert_eq!(na.env, vec!["TERM=xterm".to_string()]);
        // `--` 之后原样进命令（连 -c 也不解析）。
        assert_eq!(
            na.command,
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "echo hi".to_string()
            ]
        );
        // 未知选项报错且不带假支持。
        let err = parse_native_args(&["-q".to_string(), rf_s.to_string()]).unwrap_err();
        assert!(err.contains("不支持的选项"));

        // 未给命令时 command 为空（调用方默认 /bin/sh）。
        let na2 = parse_native_args(&["-r".to_string(), rf_s.to_string()]).unwrap();
        assert!(na2.command.is_empty());
        assert!(!na2.config.fake_root);
        assert_eq!(na2.config.cwd, "/");
        std::fs::remove_dir_all(&rf).ok();
    }

    /// 初始 cwd 语义（M5.2）四种组合：显式 -w 绝对 / 显式 -w 相对 /
    /// 未给 -w 有 rootfs / 两者皆无。
    #[test]
    fn initial_cwd_semantics() {
        let rf = std::env::temp_dir().join("mox-proot-test-cwd");
        std::fs::create_dir_all(&rf).unwrap();

        // 1) 显式 -w 绝对路径：走规则翻译（rootfs 前缀命中 → 原样；否则拼 rootfs）。
        let mut cfg = crate::proot::ProotConfig::new(&rf);
        cfg.cwd = "/home".to_string();
        let rules = cfg.to_rules();
        assert_eq!(
            initial_cwd_for(&cfg, &rules),
            Some(rf.join("home")),
            "-w 绝对路径应翻译进 rootfs"
        );

        // 2) 显式 -w 相对路径：按 rootfs 解析（guest 视角相对量）。
        let mut cfg = crate::proot::ProotConfig::new(&rf);
        cfg.cwd = "build".to_string();
        let rules = cfg.to_rules();
        assert_eq!(initial_cwd_for(&cfg, &rules), Some(rf.join("build")));

        // 3) 未给 -w（默认 "/"）且有 rootfs：cwd = rootfs 根。
        let cfg = crate::proot::ProotConfig::new(&rf);
        let rules = cfg.to_rules();
        assert_eq!(
            initial_cwd_for(&cfg, &rules),
            Some(rf.clone()),
            "未给 -w 时默认进 rootfs 根，不泄露 tracer 宿主 cwd"
        );

        // 4) 无 rootfs 也无 -w：None = 继承 tracer cwd（上游无 -r 行为）。
        let mut cfg = crate::proot::ProotConfig::new(Path::new(""));
        cfg.rootfs = PathBuf::new();
        let rules = cfg.to_rules();
        assert_eq!(initial_cwd_for(&cfg, &rules), None);

        // 5) 无 rootfs + 显式相对 -w：None（按 tracer 当前 cwd 解析）。
        let mut cfg = crate::proot::ProotConfig::new(Path::new(""));
        cfg.rootfs = PathBuf::new();
        cfg.cwd = "somedir".to_string();
        let rules = cfg.to_rules();
        assert_eq!(initial_cwd_for(&cfg, &rules), None);

        std::fs::remove_dir_all(&rf).ok();
    }
}

// ==================== mx: 短链参数识别单测（0.2.0） ====================

#[cfg(test)]
mod launch_tests {
    use super::*;

    fn v(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn find_mx_rootfs_all_option_forms() {
        assert_eq!(
            find_mx_rootfs_arg(&v(&["-r", "mx:alpine"])),
            Some("mx:alpine".into())
        );
        assert_eq!(
            find_mx_rootfs_arg(&v(&["--rootfs", "mx:ubuntu"])),
            Some("mx:ubuntu".into())
        );
        assert_eq!(
            find_mx_rootfs_arg(&v(&["-R", "mx:kali", "--", "bash"])),
            Some("mx:kali".into())
        );
        assert_eq!(
            find_mx_rootfs_arg(&v(&["-S", "mx:debian"])),
            Some("mx:debian".into())
        );
        assert_eq!(
            find_mx_rootfs_arg(&v(&["--rootfs=mx:alpine"])),
            Some("mx:alpine".into())
        );
    }

    #[test]
    fn find_mx_rootfs_ignores_plain_paths() {
        assert_eq!(find_mx_rootfs_arg(&v(&["-r", "/data/rootfs"])), None);
        assert_eq!(find_mx_rootfs_arg(&v(&["--rootfs=./ubuntu"])), None);
        assert_eq!(find_mx_rootfs_arg(&v(&["-r"])), None); // 缺参数不 panic
        assert_eq!(find_mx_rootfs_arg(&v(&[])), None);
        // -b/-w/-e 的值哪怕长得像 mx: 也不算（只认 rootfs 位）。
        assert_eq!(find_mx_rootfs_arg(&v(&["-w", "mx:alpine"])), None);
    }

    #[test]
    fn tail_after_dashdash_extraction() {
        assert_eq!(
            tail_after_dashdash(&v(&["-r", "mx:alpine", "--", "vim", "-R"])),
            vec!["vim".to_string(), "-R".to_string()]
        );
        assert_eq!(tail_after_dashdash(&v(&["-r", "mx:alpine"])), Vec::<String>::new());
        assert_eq!(tail_after_dashdash(&v(&["--"])), Vec::<String>::new());
        // 只认第一个 --。
        assert_eq!(
            tail_after_dashdash(&v(&["--", "a", "--", "b"])),
            vec!["a".to_string(), "--".to_string(), "b".to_string()]
        );
    }

    /// `env` 子命令：未安装报错；装好后预览含 LANG 与 env 文件路径提示。
    #[test]
    fn env_subcommand_preview() {
        let root = std::env::temp_dir().join("mox-proot-cli-envtest");
        let _ = std::fs::remove_dir_all(&root);
        let rootfs = root.join("containers/fake/rootfs");
        std::fs::create_dir_all(rootfs.join("etc/mox-proot")).unwrap();
        std::fs::write(rootfs.join("etc/mox-proot/env"), "LANG=zh_CN.UTF-8\n").unwrap();

        // 未安装 id：明确报错。
        let out = proot_distro(&["env", "nosuch"], &root);
        assert!(out.contains("未安装"), "{}", out);

        // 已安装：预览里应含文件里的 LANG 与文件路径提示。
        let out = proot_distro(&["env", "fake"], &root);
        assert!(out.contains("LANG=zh_CN.UTF-8"), "env 文件值应进预览: {}", out);
        assert!(out.contains("etc/mox-proot/env"), "应提示 env 文件路径: {}", out);
        assert!(out.contains("存在"), "应标记文件已存在: {}", out);

        // 无参数：用法提示。
        let out = proot_distro(&["env"], &root);
        assert!(out.contains("用法"), "{}", out);

        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 渗透复测 P1：CLI 行为 ----

    /// `--version`/`-V` 纯文本可用，不触网/不触引擎。
    #[test]
    fn version_subcommands_work() {
        for flag in ["--version", "-V"] {
            let out = proot_distro(&[flag], Path::new("/nonexistent"));
            assert!(out.contains(env!("CARGO_PKG_VERSION")), "{}: {}", flag, out);
            assert!(out.contains("mox-proot-distro"), "{}", out);
        }
        assert!(!is_unknown_subcommand(&["--version"]));
        assert!(!is_unknown_subcommand(&["-V"]));
    }

    /// 未知子命令判定：合法命令（含全局开关）放行，乱敲的判未知。
    #[test]
    fn unknown_subcommand_detection() {
        assert!(is_unknown_subcommand(&["frobnicate"]));
        assert!(is_unknown_subcommand(&["--json", "frobnicate"]));
        assert!(!is_unknown_subcommand(&[]));
        assert!(!is_unknown_subcommand(&["help"]));
        assert!(!is_unknown_subcommand(&["--help"]));
        assert!(!is_unknown_subcommand(&["--json", "list"]));
        assert!(!is_unknown_subcommand(&["recipe", "list"]));
        // 既有文案仍保留，别把用户引导弄丢。
        let out = proot_distro(&["frobnicate"], Path::new("/nonexistent"));
        assert!(out.contains("未知子命令"));
    }

    /// P1：`--` 之前的 -b/-w/-e/-v/-0 不再静默丢弃，而是解析成结构化覆盖。
    #[test]
    fn engine_opts_are_parsed_not_dropped() {
        let args: Vec<String> = ["-b", "/sdcard:/sdcard", "-w", "/root",
                                 "-e", "FOO=bar", "-v", "-v", "-0"]
            .iter().map(|s| s.to_string()).collect();
        let o = parse_engine_opts(&args).unwrap();
        assert_eq!(o.binds, vec![("/sdcard".to_string(), "/sdcard".to_string())]);
        assert_eq!(o.cwd.as_deref(), Some("/root"));
        assert_eq!(o.env, vec![("FOO".to_string(), "bar".to_string())]);
        assert_eq!(o.verbose, 2);
        assert_eq!(o.fake_root, Some(true));
        // 空选项 = 默认，行为与旧路径一致。
        assert!(parse_engine_opts(&[]).unwrap().binds.is_empty());
    }

    /// P1：不支持/缺参数的选项必须明确报错，绝不静默吞掉。
    #[test]
    fn engine_opts_reject_unknown_or_malformed() {
        assert!(parse_engine_opts(&["--bogus".into()]).is_err());
        assert!(parse_engine_opts(&["-b".into()]).is_err());
        assert!(parse_engine_opts(&["-e".into(), "NOEQUALS".into()]).is_err());
        assert!(parse_engine_opts(&["-w".into()]).is_err());
        // 合法空 -e 值仍可（KEY=）。
        assert!(parse_engine_opts(&["-e".into(), "K=".into()]).is_ok());
    }

    /// P1：诊断模式下 seccomp 自动让位（避免真机实测的 SIGABRT）。
    /// 该断言依赖进程级环境，用 MUtex 串行，且完整恢复现场。
    #[test]
    fn seccomp_yields_to_diagnostics() {
        static L: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = L.lock().unwrap_or_else(|e| e.into_inner());
        let old_v = crate::proot::ptrace::diag_enabled();
        let old_dbg = std::env::var_os("MOX_DBG");
        let old_sec = std::env::var_os("MOX_SECCOMP");
        std::env::set_var("MOX_SECCOMP", "1");
        std::env::remove_var("MOX_DBG");
        crate::proot::ptrace::set_verbose(0);
        assert!(crate::proot::ptrace::seccomp_active(), "仅 seccomp 时应启用");
        crate::proot::ptrace::set_verbose(1);
        assert!(!crate::proot::ptrace::seccomp_active(), "-v 时 seccomp 必须让位");
        // 恢复现场
        crate::proot::ptrace::set_verbose(if old_v { 1 } else { 0 });
        match old_dbg { Some(v) => std::env::set_var("MOX_DBG", v), None => std::env::remove_var("MOX_DBG") }
        match old_sec { Some(v) => std::env::set_var("MOX_SECCOMP", v), None => std::env::remove_var("MOX_SECCOMP") }
    }
}
