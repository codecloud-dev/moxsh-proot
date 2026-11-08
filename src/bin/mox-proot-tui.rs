//! `mox-proot-tui` 第三二进制入口（路线图 0.4.0）。
//!
//! ⚠️ 参数必须在**任何终端初始化之前**解析：`--version` / `-V` / `-h` / `--help`
//! 要能在无 tty、管道、CI 自检里安全返回，且绝不改动终端状态。
//! 2026-11-08 真机实测：旧版四个参数全部先撞进初始化，返回「无法进入备用屏幕」；
//! musl 资产更因动态链接在启动期直接 SIGSEGV。本入口把这两类问题挡在最前面。

use std::io::IsTerminal;

const HELP: &str = "\
mox-proot-tui —— mox-proot 图形控制台

用法:
  mox-proot-tui                 打开全屏控制台（需要交互终端）
  mox-proot-tui --version       打印版本
  mox-proot-tui -V              打印版本
  mox-proot-tui -h, --help      打印本帮助

无交互终端时请用命令版:
  mox-proot-distro list
  mox-proot-distro install <id>
  mox-proot-distro login <id>
";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // 前置解析：只要出现版本/帮助标志就立即返回，绝不碰终端。
    for a in &args {
        match a.as_str() {
            "--version" | "-V" => {
                println!("mox-proot-tui {}（ratatui 控制台）", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "-h" | "--help" => {
                print!("{}", HELP);
                std::process::exit(0);
            }
            _ => {}
        }
    }
    if !args.is_empty() {
        eprintln!(
            "mox-proot-tui: 不支持的参数「{}」。可用：--version / -V / -h / --help",
            args.join(" ")
        );
        std::process::exit(2);
    }
    // 无 TTY：不进备用屏幕、不改 termios，明确降级而不是硬崩或 os error 11。
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        eprintln!("mox-proot-tui: 需要交互终端（TTY）才能打开控制台。");
        eprintln!("当前环境无 TTY，请改用命令版：mox-proot-distro list / install <id> / login <id>");
        std::process::exit(2);
    }
    std::process::exit(mox_proot::proot::tui::run_tui());
}
