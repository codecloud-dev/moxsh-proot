//! 后端基址集中配置：设备码、授权等客户端统一从这里取端点，支持环境变量覆盖。

/// 对外统一入口（客户端请求一律走这里）。
pub const PUBLIC_BASE: &str = "https://mox-site.pages.dev";
/// 兼容常量：服务端端点基址（即对外入口）。
pub const ID_BASE: &str = PUBLIC_BASE;
/// 兼容常量：页面基址（与对外入口同源）。
pub const SITE_BASE: &str = PUBLIC_BASE;

/// 覆盖服务端入口（自托管 / 联调）。
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

/// 服务端端点（如 `endpoint("/device/code")`）。
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
        // 占位域名与废弃 Pages 地址不得回流
        assert!(!PUBLIC_BASE.contains("mox-proot.app"));
        assert!(!PUBLIC_BASE.contains("github.io"));
        // 客户端请求目标保持为统一入口
        assert!(!id_base().is_empty());
        assert!(!endpoint("/device/code").is_empty());
    }

    #[test]
    fn join_handles_slashes() {
        assert_eq!(join("https://x.dev/", "/device/code"), "https://x.dev/device/code");
        assert_eq!(join("https://x.dev", "device/token"), "https://x.dev/device/token");
        assert_eq!(join("https://x.dev///", "/fan/verify"), "https://x.dev/fan/verify");
    }

    #[test]
    fn endpoints_match_backend_route_table() {
        // 路径表与服务端路由保持一致。
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
