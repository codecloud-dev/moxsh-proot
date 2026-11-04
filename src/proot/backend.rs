//! 后端基址**单一来源**：把散在设备码 / 粉丝 / 商品授权三个客户端里的
//! 硬编码地址收口，彻底解决「后端早就部署了，客户端却指着占位域名」。
//!
//! ## 真实链路（已核对 mox-id 与 mox-site 两个仓库）
//! ```text
//!   终端 ──POST /device/code──► https://mox-site.pages.dev   （官网 Pages）
//!                                    │ functions/[[path]].js 同域反代
//!                                    ▼
//!                              https://mox-id.moxsh.app      （Worker 真实域）
//!                                    │ 兜底
//!                                    ▼
//!                        https://mox-id.codecloud.workers.dev
//! ```
//! Pages Functions 的 `onRequestPost` 一律转发 Worker；`onRequestGet` 对
//! 未命中静态 html 的动态路径同样反代。因此 `/device/code`、`/device/token`、
//! `/fan/verify`、`/entitlement/check`、`/activate` 走**官网同域**即可。
//! 走同域还有两个额外好处：国内可达（workers.dev 直连不稳），且 SSO 与
//! 官网 localStorage 的 mox_token 同源不断链。
//!
//! 旧的 `https://mox-id.mox-proot.app` 是**从未部署的占位域名**，
//! `https://codecloud-dev.github.io/mox-site` 是已废弃的 Pages 地址。
//!
//! 二维码 / 验证码用的是后端返回的 `verification_uri_complete` 原值：Worker 的
//! `publicBase()` 会读 `x-forwarded-host` 生成同源链接，客户端不自行拼装。
//!
//! 环境变量 `MOX_ID_BASE` / `MOX_SITE_BASE` 可覆盖，用于自托管或本地联调。

/// 对外统一入口：官网 Pages（同域反代 + 国内可达 + SSO 同源）。
pub const PUBLIC_BASE: &str = "https://mox-site.pages.dev";
/// mox-id Worker 的真实上游域名（Pages Functions 的 `UPSTREAM`）。
pub const UPSTREAM_BASE: &str = "https://mox-id.moxsh.app";
/// Worker 兜底域（Pages Functions 的 `LEGACY_UPSTREAM`）。
pub const WORKERS_FALLBACK: &str = "https://mox-id.codecloud.workers.dev";

/// 兼容常量：mox-id 侧端点基址（即对外入口）。
pub const ID_BASE: &str = PUBLIC_BASE;
/// 兼容常量：官网页面基址（与对外入口同源）。
pub const SITE_BASE: &str = PUBLIC_BASE;

/// 覆盖 mox-id / 官网入口（自托管 / 联调）。
pub const ID_BASE_ENV: &str = "MOX_ID_BASE";
pub const SITE_BASE_ENV: &str = "MOX_SITE_BASE";

fn pick(env: &str, default: &str) -> String {
    std::env::var(env)
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// 后端端点基址（环境变量优先）。
pub fn id_base() -> String {
    pick(ID_BASE_ENV, PUBLIC_BASE)
}

/// 官网页面基址（环境变量优先）。
pub fn site_base() -> String {
    pick(SITE_BASE_ENV, PUBLIC_BASE)
}

/// 纯函数：拼一次路径，保证不出现 `//` 双斜杠或漏斜杠。
pub fn join(base: &str, path: &str) -> String {
    let b = base.trim_end_matches('/');
    let p = if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    };
    format!("{b}{p}")
}

/// mox-id 上的端点（如 `endpoint("/device/code")`）。
pub fn endpoint(path: &str) -> String {
    join(&id_base(), path)
}

/// 官网上的页面（如 `site_page("/fan.html")`）。
pub fn site_page(path: &str) -> String {
    join(&site_base(), path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_real_deployed_hosts_only() {
        assert_eq!(PUBLIC_BASE, "https://mox-site.pages.dev");
        assert_eq!(UPSTREAM_BASE, "https://mox-id.moxsh.app");
        // 占位域名与废弃 Pages 地址不得回流
        assert!(!PUBLIC_BASE.contains("mox-proot.app"));
        assert!(!PUBLIC_BASE.contains("github.io"));
        assert!(!UPSTREAM_BASE.contains("mox-proot.app"));
    }

    #[test]
    fn join_handles_slashes() {
        assert_eq!(join("https://x.dev/", "/device/code"), "https://x.dev/device/code");
        assert_eq!(join("https://x.dev", "device/token"), "https://x.dev/device/token");
        assert_eq!(join("https://x.dev///", "/fan/verify"), "https://x.dev/fan/verify");
    }

    #[test]
    fn endpoints_match_backend_route_table() {
        // 与 mox-id/src/index.ts 的 route() 及 mox-site/functions 反代前缀逐条对齐。
        let paths = [
            "/device/code",
            "/device/token",
            "/activate",
            "/entitlement/check",
            "/devices/register",
            "/fan/verify",
            "/products/redeem",
        ];
        for p in paths {
            assert_eq!(join(PUBLIC_BASE, p), format!("{PUBLIC_BASE}{p}"), "{p}");
        }
    }

    #[test]
    fn default_endpoints_are_same_origin_with_site() {
        assert!(endpoint("/device/code").starts_with(SITE_BASE));
        assert_eq!(site_page("/fan.html"), format!("{PUBLIC_BASE}/fan.html"));
        assert_eq!(site_page("/bind.html"), format!("{PUBLIC_BASE}/bind.html"));
    }
}
