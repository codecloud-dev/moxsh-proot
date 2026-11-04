//! moxsh proot 品牌字标（ASCII 图标），用于 README、容器控制台欢迎与诊断输出。
//!
//! 用户要求「进入是我们 moxsh proot，这种画」——即一个可被「画」出来的图标/字标。

/// 多架构容器引擎字标（figlet 风格）。
pub const BANNER: &str = "\
        __  __           __  ____
       / / / /_  ______/ /_/ __ \\ ____  _____
      / /_/ / / / / __/ __/ /_/ / __ \\/ ___/
     / __  / /_/ / /_/ /_/ ____/ /_/ (__  )
    /_/ /_/\\__,_/\\__/\\__/_/    \\____/____/   proot
";

/// 单行标识（日志/欢迎用，不占多行）。
pub const TAGLINE: &str = "moxsh · proot — 多架构容器引擎";

/// 返回字标 + 版本占位（调用方填入实际版本）。
pub fn banner_with(version: &str) -> String {
    format!("{}\n{}  v{}\n", BANNER, TAGLINE, version)
}
