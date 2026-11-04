//! `mox-proot-tui` 第三二进制入口（路线图 0.4.0）。
//!
//! 仅 `--features tui` 构建（见 `Cargo.toml` 的 `required-features = ["tui"]`）。
//! 逻辑全部在 [`mox_proot::proot::tui::run_tui`]，本文件只做退出码转发。

fn main() {
    std::process::exit(mox_proot::proot::tui::run_tui());
}
