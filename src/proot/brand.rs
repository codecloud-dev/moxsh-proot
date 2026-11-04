//! mox-proot 品牌字标（ASCII 图标），用于 README、容器控制台欢迎与诊断输出。
//!
//! 审美参照 opencode 的启动字标：实心块（block-pixel）字标 + 圆角信息框，
//! 紧凑、清晰、零 ANSI，保证 README / 终端日志都能正常渲染。

/// 字标主体：紧凑 `mox` 科技字标（3 行、14 宽，手机端 GitHub 不折行）
/// + 一行极简标签。纯文本、零 ANSI，保证 README / 终端日志可移植。
pub const BANNER: &str = r"_  _ ____ _  _
|\/| |  |  \/
|  | |__| _/\_
  mox-proot";

/// 单行标识（日志/欢迎用，不占多行）。
pub const TAGLINE: &str = "mox-proot — 多架构容器引擎";

/// 返回字标 + 版本占位（调用方填入实际版本）。
pub fn banner_with(version: &str) -> String {
    format!("{}\n      v{}\n", BANNER, version)
}
