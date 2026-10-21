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
//! 除 App 内嵌路径外，本 crate 还编出 `moxsh-proot` 独立可执行文件
//! （`src/bin/moxsh-proot.rs`）：让**原版 Termux 用户**不装 moxsh App 也能
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
use crate::proot::script;
use crate::proot::settings::{self, Settings};
use crate::proot::translate::TranslateRules;
use crate::proot::{ProotConfig, ProotError, ProotResult};

/// 命令路由入口：`command` 为 `proot` 或 `proot-distro`，`args` 为其余参数。
pub fn route(command: &str, args: &[&str], moxsh_root: &Path) -> String {
    match command {
        "proot-distro" => proot_distro(args, moxsh_root),
        // `proot` 原生参数面：moxsh 引擎内嵌（无独立二进制），给出等效用法说明
        // 并支持 -r/-0/-b/-w 组装预览；实际启动走 moxsh UI 或 login 命令。
        "proot" => proot_native(args, moxsh_root),
        other => format!("proot: 未知命令 {}", other),
    }
}

/// 独立二进制入口：解析上游风格参数，真起 ptrace 引擎跑目标命令。
///
/// 返回进程退出码（0 = 成功；非 0 = 引擎报错或目标命令退出码）。
/// 错误信息写 stderr（不混入 stdout，管道/重定向不受污染）。
///
/// 与 [`route`] 的分工：`route` 服务 App 内嵌 shell 的**文本交互**；
/// 本函数服务**真执行**（原版 Termux 用户直接 `moxsh-proot ...`）。
pub fn run_native(args: &[String]) -> i32 {
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
                eprintln!("moxsh-proot: mx: 启动失败: {}", e);
                2
            }
        };
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
            eprintln!("moxsh-proot: {}", e);
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
/// "已有 [`ProotConfig`] 就直接执行"的场景（`moxsh-proot-distro login`）。
/// 返回进程退出码（0 = 成功；非 0 = 引擎报错或目标命令退出码）。
pub fn run_config(
    config: &crate::proot::ProotConfig,
    command: &[String],
    env: &[String],
) -> i32 {
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
            eprintln!("moxsh-proot: 启动失败: {}", e);
            return 126;
        }
    };
    match tracer.run(child) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("moxsh-proot: 引擎错误: {}", e);
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

/// 解析 `moxsh-proot` 参数面（对齐上游 proot 常用选项）。
///
/// `-b` 语法与上游一致：`host_path:guest_location`（host 在前）。
/// guest_location 尾缀 `!` = 不反向还原（沿用本仓 translate 层语义）。
fn parse_native_args(args: &[String]) -> Result<NativeArgs, String> {
    let mut rootfs: Option<PathBuf> = None;
    let mut fake_root = false;
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
            other if other.starts_with('-') => {
                // 注意：宏参数里的相邻字符串字面量**不会**隐式拼接（与普通表达式
                // 不同），必须写成单个字面量，否则 rustc 报 "expected `,`"。
                return Err(format!(
                    "不支持的选项 {}（可用项见 moxsh-proot --help；上游 -q/-k/-v 等未实现，勿假装支持）",
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

/// 用法文本（`-h` 走 stdout 退出 0；参数错误走 stderr 退出 2）。
fn print_usage(w: &mut dyn std::io::Write) {
    write!(
        w,
        "用法: moxsh-proot [-r rootfs | -R rootfs | -S rootfs] [-0] \
         [-b host:guest]... [-w dir] [-e K=V]... [--] 命令 [参数...]\n\
         \n\
           -r, --rootfs <path>   guest 根目录\n\
           -R <path>            -r + 推荐绑定集（/etc 元文件、/tmp、/run…）\n\
           -S <path>            -r + 最小绑定集（装包场景）\n\
           -0, --root-id        fake root（uid/gid 显示为 0，伪装特权成功）\n\
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

/// `proot-distro` 主路由（对齐上游子命令集）。
pub fn proot_distro(args: &[&str], moxsh_root: &Path) -> String {
    let mgr = DistroManager::new(moxsh_root);
    let (sub, rest) = args.split_first().map_or(("", &[][..]), |(s, r)| (*s, r));
    match sub {
        "list" => cmd_list(&mgr),
        "install" => cmd_install(&mgr, rest),
        "login" => cmd_login(&mgr, rest),
        "shell" => cmd_shell(&mgr, rest),
        "remove" => cmd_remove(&mgr, rest),
        "backup" => cmd_backup(&mgr, rest),
        "restore" => cmd_restore(&mgr, rest, moxsh_root),
        "net" => cmd_net(&mgr, rest),
        "env" => cmd_env(&mgr, rest),
        "recipe" => cmd_recipe(rest, moxsh_root),
        "fan" => cmd_fan(rest, moxsh_root),
        "wizard" => cmd_wizard(rest),
        // launch 需真起引擎（未装自动装），不在文本路由内执行——
        // bin 层 main 把 login/shell/launch 一并路由到 exec 分支；
        // 走到这里（App 内嵌文本路由）只给用法提示。
        "launch" => "用法: moxsh-proot-distro launch mx:<名字> [-- 命令...]\n\
             （mx:alpine / mx:ubuntu / mx:debian / mx:kali；自定义见 ~/.config/moxsh-proot/links.toml）"
            .to_string(),
        "help" | "--help" | "-h" | "" => HELP.to_string(),
        other => format!("proot-distro: 未知子命令 {}\n{}", other, HELP),
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
        return "用法: moxsh-proot-distro env <id>".to_string();
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

/// `recipe list|show <id>|apply <id> <distro>`：配方系统（0.3.0，roadmap §4）。
///
/// - `list`：枚举可用配方（id/名称/描述）。
/// - `show <id>`：配方详情 + 应用命令。
/// - `apply <id> <distro>`：把脚本落进容器 /tmp 后在容器内执行（复用引擎）。
fn cmd_recipe(args: &[&str], moxsh_root: &Path) -> String {
    let idx = RecipeIndex::load();
    match args.first().copied() {
        None | Some("list") => {
            if idx.all().is_empty() {
                return "配方：当前无可用配方（recipes/index.toml 为空）".to_string();
            }
            let mut out = String::from(
                "可用配方（应用：moxsh-proot-distro recipe apply <id> <distro>）：\n",
            );
            for r in idx.all() {
                let lock = if r.fan_only {
                    if fan::is_fan(moxsh_root) {
                        " [粉丝✓]"
                    } else {
                        " [粉丝🔒 mox fan redeem]"
                    }
                } else {
                    ""
                };
                out.push_str(&format!(
                    "  {:<8} {:<16} {}{}\n",
                    r.id, r.name, r.description, lock
                ));
            }
            out
        }
        Some("show") => match args.get(1) {
            Some(id) => match idx.find(id) {
                Ok(r) => {
                    let mut s = format!(
                        "配方 {}\n  名称：{}\n  描述：{}\n  标签：{}\n  脚本：{}\n  应用：moxsh-proot-distro recipe apply {} <distro>",
                        r.id, r.name, r.description, r.tags.join("/"), r.script, r.id
                    );
                    if r.fan_only {
                        s.push_str(&format!(
                            "\n  粉丝专属：{}",
                            if fan::is_fan(moxsh_root) {
                                "已解锁".to_string()
                            } else {
                                "未解锁（mox fan login 领兑换码 → mox fan redeem <code>）".to_string()
                            }
                        ));
                    }
                    s
                }
                Err(e) => format!("recipe: {}", e),
            },
            None => "用法: moxsh-proot-distro recipe show <id>".to_string(),
        },
        Some("apply") => {
            let rid = args.get(1).copied();
            let did = args.get(2).copied();
            match (rid, did) {
                (Some(rid), Some(did)) => match idx.apply(moxsh_root, rid, did) {
                    Ok(code) => return format!("配方 {} 已应用（容器内退出码 {}）", rid, code),
                    Err(e) => return format!("recipe: 应用失败: {}", e),
                },
                _ => return "用法: moxsh-proot-distro recipe apply <id> <distro>".to_string(),
            }
        }
        Some(other) => format!(
            "recipe: 未知子命令 {}\n{}",
            other,
            "用法: moxsh-proot-distro recipe list|show <id>|apply <id> <distro>"
        ),
    }
}

/// `fan login|redeem <code>|status`：粉丝专属（隐藏粉丝页 + 回环自动收码 + 在线校验）。
///
/// - `login`（默认）：本机 `127.0.0.1` 起一次性回环监听 → 唤起手机浏览器打开
///   **隐藏粉丝页**（官网导航点不到那个，入口命令由公众号告知）→ 页内 GitHub
///   登录 + 点「确认领取」= 后端签发激活码 → 页面带 `code` 跳回回环，脚本
///   **自动收码** → `POST /fan/verify` 经官方服务端**联网真实校验** → 落盘
///   `$MOXSH_ROOT/fan.toml` 解锁 fan_only 配方。全程无需手输兑换码；
/// - `redeem <code>`：手动路径（桌面 Linux 没浏览器可调时）——官网粉丝页
///   手动领取后回来 `mox fan redeem <code>`，同样联网验；
/// - `status`：当前粉丝状态与验签模式（纯在线，客户端零密钥）。
///
/// 灰度：`MOXSH_FAN_ENABLE=1` 才开放（后端 `/fan/issue` 另有白名单，双保险）。
fn cmd_fan(args: &[&str], moxsh_root: &Path) -> String {
    if !fan::fan_enabled() {
        return format!(
            "粉丝专属：灰度测试中（{}=1 开启，正式版自动开放）。\n\
             \x20 领取入口与激活方式请关注公众号，按自动回复指引操作。",
            fan::FAN_ENABLE_ENV
        );
    }
    match args.first().copied() {
        None | Some("login") => match fan::login(moxsh_root) {
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
                     \x20 moxsh-proot-distro recipe list   # 粉丝专属配方已亮起 🔓",
                    o.tier
                ));
                out
            }
            Err(e) => format!("fan: {}\n（重试：mox fan login；桌面 Linux 可先开粉丝页 URL 手动领取，再 mox fan redeem <code>）", e),
        },
        Some("redeem") => match args.get(1).copied() {
            Some(code) => match fan::redeem(moxsh_root, code) {
                Ok(tier) => format!(
                    "兑换成功，粉丝专属已解锁（档位：{}）。\n\
                     \x20 mox fan status                        # 查看状态\n\
                     \x20 moxsh-proot-distro recipe list        # 粉丝专属配方已亮起",
                    tier
                ),
                Err(e) => format!("fan: {}", e),
            },
            None => "用法: mox fan redeem <兑换码>\n（桌面 Linux：mox fan login 会给出粉丝页 URL，手动领取后回来兑换）".to_string(),
        },
        Some("status") => {
            let mode = fan::verify_mode();
            match fan::load_state(moxsh_root) {
                Some(st) => format!(
                    "粉丝专属：已解锁\n  档位：{}\n  验签模式：{}\n  兑换码：{}\n  令牌文件：{}\n  （删除本文件 = 解除绑定，需重新 mox fan login）",
                    st.tier,
                    mode,
                    st.code,
                    fan::fan_file_path(moxsh_root).display()
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

/// `install <id>`：安装（缓存归档优先，否则内部下载兜底）。
fn cmd_install(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro install <id>".into();
    };
    let spec = match DistroManager::find_spec(id) {
        Ok(s) => s,
        Err(e) => return format!("proot-distro: {}", e),
    };
    // --cache <path>：指定外部已下载归档（Android 场景由 UI 层下载）。
    let mut cache: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if *a == "--cache" {
            cache = it.next().map(PathBuf::from);
        }
    }
    let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
        let _ = (msg, done, total); // 文本 CLI 简化：完成时统一输出
    });
    match mgr.install(&spec, cache.as_deref(), &mut progress) {
        Ok(()) => install_hint(spec.id, mgr),
        Err(e) => format!("proot-distro: 安装失败: {}", e),
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
            "警告：启动器写入失败（{}），可用 moxsh-proot-distro launch mx:{} 进入\n",
            e, id
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
            "警告：控制台脚本写入失败（{}），容器内可用 moxsh-proot-distro 排障\n",
            e
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
///   ① 数据根：`--root` 或保持现值（默认 `~/.moxsh`）→ upsert 进 config.toml；
///   ② 发行版：`--distro`（默认 alpine）→ 已装跳过，未装走完整安装链路；
///   ③ 教学提示：换源 / 中文 / 控制台 / 启动器一次说清。
/// 幂等：重复跑只补缺失步骤，不重复下载、不重复提问。
fn cmd_wizard(args: &[&str]) -> String {
    let mut root_arg: Option<String> = None;
    let mut distro = "alpine".to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i] {
            "--root" => match args.get(i + 1) {
                Some(v) => {
                    root_arg = Some(v.to_string());
                    i += 2;
                }
                None => {
                    return "用法: moxsh-proot-distro wizard [--root <目录>] [--distro <id>]（--root 缺参数）"
                        .to_string()
                }
            },
            "--distro" => match args.get(i + 1) {
                Some(v) => {
                    distro = v.to_string();
                    i += 2;
                }
                None => {
                    return "用法: moxsh-proot-distro wizard [--root <目录>] [--distro <id>]（--distro 缺参数）"
                        .to_string()
                }
            },
            other => {
                return format!(
                    "wizard: 未知参数 {}\n用法: moxsh-proot-distro wizard [--root <目录>] [--distro <id>]",
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

    let mut out = String::from("moxsh-proot 初始化向导\n\n");

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
                    if total > 0 && done >= total {
                        eprintln!("[安装] {}", msg);
                    } else {
                        eprintln!("[安装] {}（{}/{}）", msg, done, total);
                    }
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
        "\n接下来：\n  moxsh-proot-distro launch mx:{d}    # 直接进容器\n  {d}                                  # 启动器（已落盘）\n  moxsh-proot-distro env {d}            # 查中文环境\n  容器内输入 mox                        # 管理控制台（固定命令）\n",
        d = distro
    ));
    out
}

/// `login <id>`：输出可执行的 proot 命令行（由 PTY 层执行）。
fn cmd_login(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro login <id>".into();
    };
    match mgr.login_cmd(id) {
        Ok(cmd) => cmd,
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `shell <id>`：非登录 shell 快捷方式（等价 login 但用 /bin/sh）。
fn cmd_shell(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro shell <id>".into();
    };
    let rootfs = mgr.distro_rootfs(id);
    if !rootfs.is_dir() {
        return format!("proot-distro: 发行版未安装: {}", id);
    }
    // 与 login 相同骨架，仅 shell 换 /bin/sh（非 -l）。
    match mgr.login_cmd(id) {
        Ok(mut cmd) => {
            // 把末尾 shell 段替换为 /bin/sh。
            if let Some(pos) = cmd.rfind(" /bin/") {
                cmd.truncate(pos);
                cmd.push_str(" /bin/sh");
            }
            cmd
        }
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `remove <id>`：删除。
fn cmd_remove(mgr: &DistroManager, args: &[&str]) -> String {
    let Some(id) = args.first() else {
        return "用法: proot-distro remove <id>".into();
    };
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
        Ok(()) => format!("已备份到 {}", out),
        Err(e) => format!("proot-distro: {}", e),
    }
}

/// `restore <id> <tar>`：恢复。
fn cmd_restore(mgr: &DistroManager, args: &[&str], _moxsh_root: &Path) -> String {
    let (Some(id), Some(tar)) = (args.first(), args.get(1)) else {
        return "用法: proot-distro restore <id> <tar.tar.gz>".into();
    };
    match mgr.restore(id, Path::new(tar)) {
        Ok(()) => format!("已恢复 {}", id),
        Err(e) => format!("proot-distro: {}", e),
    }
}

// ==================== mx: 短链启动（0.2.0，docs/roadmap.md §1） ====================

/// 短链启动核心：解析 `mx:xxx` → 未装自动装 → 起引擎进容器。
///
/// 两层调用方共用：`moxsh-proot -r mx:alpine -- bash`（选项值 scheme）与
/// `moxsh-proot-distro launch mx:alpine`（子命令）。
/// 返回进程退出码。
pub fn launch_spec(root: &Path, spec: &str, tail: &[String]) -> ProotResult<i32> {
    let user = links::load_user_links().unwrap_or_default();
    let resolved = links::resolve(spec, &user)?;
    let mgr = DistroManager::new(root);
    let id = match resolved {
        ResolvedLink::Distro(id) => {
            if !mgr.distro_rootfs(&id).is_dir() {
                let spec = DistroManager::find_spec(&id)?;
                eprintln!("moxsh-proot: mx:{} 未安装，开始自动安装（{} {}）…", id, spec.name, spec.version);
                let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
                    if total > 0 && done >= total {
                        eprintln!("[安装] {}", msg);
                    } else {
                        eprintln!("[安装] {}（{}/{}）", msg, done, total);
                    }
                });
                mgr.install(&spec, None, &mut progress)?;
            }
            id
        }
        ResolvedLink::Url(urls) => {
            let name = links::strip_mx(spec).unwrap_or("").to_string();
            if !mgr.distro_rootfs(&name).is_dir() {
                eprintln!("moxsh-proot: mx:{} 未安装，开始自动安装（自定义 rootfs）…", name);
                let tar = download_first_available(&mgr, &urls)?;
                let mut progress: crate::ProgressCallback = Box::new(|msg, done, total| {
                    if total > 0 && done >= total {
                        eprintln!("[安装] {}", msg);
                    } else {
                        eprintln!("[安装] {}（{}/{}）", msg, done, total);
                    }
                });
                mgr.install_custom(&name, &tar, None, &mut progress)?;
            }
            name
        }
    };
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
        let st = std::process::Command::new("curl")
            .args(["-fL", "--retry", "2", "-o"])
            .arg(&out)
            .arg(url)
            .status();
        if matches!(st, Ok(s) if s.success()) {
            return Ok(out);
        }
        let _ = std::fs::remove_file(&out);
    }
    Err(ProotError::DownloadRequired(urls.first().cloned().unwrap_or_default()))
}

fn fs_create_dir_all(p: &Path) -> ProotResult<()> {
    std::fs::create_dir_all(p).map_err(ProotError::Io)
}

/// 起引擎进容器（提炼自 `moxsh-proot-distro login/shell` 的 exec 路径，
/// 叠加 0.2.0 的进/退命令语义，roadmap §1）：
///
/// - `tail` 非空 = 用户显式命令（`mx:alpine -- vim`），替代登录 shell；
/// - 否则 `enter_cmd(id)`（每容器覆盖 > 全局默认）= 容器内首个命令
///   （`/bin/sh -c` 执行，防误触确认写在这里面）；
/// - 再否则引擎默认探测：bash -l 优先，sh -l 兜底；
/// - 会话结束后 `exit_cmd(id)` 在**宿主侧**执行（收尾提示/清理）。
pub fn exec_into(root: &Path, id: &str, tail: &[String]) -> ProotResult<i32> {
    let mgr = DistroManager::new(root);
    let rootfs = mgr.distro_rootfs(id);
    if !rootfs.is_dir() {
        return Err(ProotError::DistroNotInstalled(id.to_string()));
    }
    // pre-login 钩子：失败仅警告不阻断（钩子是用户脚本，不该把人挡在容器外）。
    match script::run_hooks(root, &rootfs, id, script::HookKind::PreLogin) {
        Ok(r) if r.has_failure() => {
            eprintln!("moxsh-proot: pre-login 钩子存在失败项：{}", r.summary())
        }
        Ok(_) => {}
        Err(e) => eprintln!("moxsh-proot: pre-login 钩子异常：{}", e),
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
    // 容器内控制台：bind 宿主 bin + MOXSH_ROOT，注入 mox 固定命令
    // （roadmap §3；MOXSH_PROOT_CONSOLE=0 可关）。
    console::bind_console(&mut cfg, root, &console::host_bin_dir());
    // env_command() 形如 ["/usr/bin/env", "-i", "HOME=...", ...]，跳过前两个
    // 前缀词即纯 KEY=VALUE 列表（spawn 直接 execve 用）。
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
/// 找到即短链启动路径（`moxsh-proot -r mx:alpine`）。
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

/// `proot` 原生参数面：识别 -r/-0/-b/-w 并输出等效 moxsh 引擎配置预览。
fn proot_native(args: &[&str], _moxsh_root: &Path) -> String {
    let mut rootfs: Option<String> = None;
    let mut fake_root = false;
    let mut cwd = "/".to_string();
    let mut binds: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match *a {
            "-r" | "--rootfs" => rootfs = it.next().map(|s| s.to_string()),
            "-0" | "--root-id" => fake_root = true,
            "-w" | "--cwd" => cwd = it.next().map(|s| s.to_string()).unwrap_or_else(|| "/".into()),
            "-b" | "--bind" => {
                if let Some(b) = it.next() {
                    binds.push(b.to_string());
                }
            }
            "-h" | "--help" => return PROOT_HELP.to_string(),
            other => return format!("proot: 暂不支持参数 {}（moxsh 引擎内嵌，UI 启动或 proot-distro login）", other),
        }
    }
    let Some(rootfs) = rootfs else {
        return "用法: proot -r <rootfs> [-0] [-b host:guest]... [-w /dir] -- cmd".into();
    };
    let mut out = format!(
        "moxsh proot 引擎配置预览：\n  rootfs: {}\n  fake_root(-0): {}\n  cwd(-w): {}\n  绑定(-b):\n",
        rootfs, fake_root, cwd
    );
    for b in binds {
        out.push_str(&format!("    {}\n", b));
    }
    out.push_str("（实际启动：moxsh 图形界面选择发行版，或 proot-distro login <id>）");
    out
}

/// proot-distro 帮助文本（对齐上游子命令集）。
const HELP: &str = "proot-distro（moxsh 兼容实现）
用法:
  proot-distro list                      列出已安装与可安装发行版
  proot-distro install <id> [--cache <tar>]   安装（--cache 指定已下载归档）
  proot-distro login <id>                进入发行版登录 shell
  proot-distro shell <id>                进入发行版非登录 shell
  proot-distro remove <id>               删除发行版
  proot-distro backup <id> <out.tar.gz>  备份
  proot-distro restore <id> <tar.tar.gz> 恢复
  proot-distro net [id]                  网络边界报告（不带 id 看宿主）
  proot-distro env <id>                  预览容器登录时注入的环境（中文排查）
  proot-distro launch mx:<名字> [-- cmd...]  短链启动（未装自动装）
  proot-distro recipe list|show <id>|apply <id> <distro>  一键配方（桌面/VNC/noVNC）
  proot-distro fan login|redeem <code>|status  粉丝专属（回环自动收码+在线校验）
  proot-distro wizard [--root <目录>] [--distro <id>]  一次性初始化（数据根+发行版）";

/// proot 原生参数帮助。
const PROOT_HELP: &str = "proot（moxsh 引擎内嵌版，支持参数：-r/--rootfs -0/--root-id -b/--bind -w/--cwd）";

#[cfg(test)]
mod tests {
    use super::*;

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

    /// login 未安装报错。
    #[test]
    fn login_not_installed() {
        let out = proot_distro(&["login", "ubuntu"], Path::new("/nonexistent"));
        assert!(out.contains("未安装"));
    }

    /// help 与未知子命令。
    #[test]
    fn help_and_unknown() {
        assert!(proot_distro(&["help"], Path::new("/x")).contains("proot-distro"));
        assert!(proot_distro(&["frobnicate"], Path::new("/x")).contains("未知子命令"));
    }

    /// fan 子命令：灰度关时任何子命令都给灰度提示（默认关，不误触粉丝流）。
    ///
    /// 三个改 `MOXSH_FAN_ENABLE` 的用例共用一把测试锁串行化——env 是进程级
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
        let rf = std::env::temp_dir().join("moxsh-test-rf");
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
        let rf = std::env::temp_dir().join("moxsh-test-cwd");
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
        let root = std::env::temp_dir().join("moxsh-cli-envtest");
        let _ = std::fs::remove_dir_all(&root);
        let rootfs = root.join("containers/fake/rootfs");
        std::fs::create_dir_all(rootfs.join("etc/moxsh")).unwrap();
        std::fs::write(rootfs.join("etc/moxsh/env"), "LANG=zh_CN.UTF-8\n").unwrap();

        // 未安装 id：明确报错。
        let out = proot_distro(&["env", "nosuch"], &root);
        assert!(out.contains("未安装"), "{}", out);

        // 已安装：预览里应含文件里的 LANG 与文件路径提示。
        let out = proot_distro(&["env", "fake"], &root);
        assert!(out.contains("LANG=zh_CN.UTF-8"), "env 文件值应进预览: {}", out);
        assert!(out.contains("etc/moxsh/env"), "应提示 env 文件路径: {}", out);
        assert!(out.contains("存在"), "应标记文件已存在: {}", out);

        // 无参数：用法提示。
        let out = proot_distro(&["env"], &root);
        assert!(out.contains("用法"), "{}", out);

        let _ = std::fs::remove_dir_all(&root);
    }
}
