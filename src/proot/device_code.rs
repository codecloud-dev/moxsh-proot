//! 设备码授权客户端（RFC 8628）。
//!
//! 流程（与 mox-id 后端 `/device/code` `/device/token` `/activate` 对齐）：
//!   ① [`request_code`] → device_code + user_code + verification_uri_complete
//!   ② 把 verification_uri_complete 渲染成终端二维码（或降级为纯文本 URL）
//!   ③ 手机扫码打开 /activate，用 GitHub / Gitee 登录并确认绑定
//!   ④ [`poll_token`] 轮询：pending 返回 `None`，authorized 返回 device_token 并落盘
//!
//! HTTP 走系统 `curl`（CLI 工具本就依赖 curl，零 TLS 依赖）。
//! JSON 解析为手写最小提取器（响应为扁平字段，无需 serde），且**与网络调用分离**便于单测。
//!
//! **接线说明**：本模块不依赖任何可选 crate，因此**不对 `tui` feature 设门禁**——
//! 默认发布的 `mox-proot` / `mox-proot-distro` 同样具备 `login` 子命令。此前整个模块
//! 被 `#[cfg(feature = "tui")]` 挡在发布产物之外，后端虽已部署却等于没接上。
//! 二维码只是可选增强：无 tui 时降级为「链接 + 验证码」文本，绑定流程照样走得通。

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use crate::proot::backend;
use crate::proot::device_claim::{self, DeviceClaim};

/// 后端基址默认常量：取 [`backend::ID_BASE`] 的真实部署地址。
/// 可用环境变量 `MOX_ID_BASE` 覆盖（自托管 / 联调）。
pub const DEFAULT_BASE: &str = backend::ID_BASE;

#[derive(Debug, Clone)]
pub struct DeviceCodeResp {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone)]
pub struct TokenResp {
    pub device_token: String,
    pub user_id: Option<i64>,
}

/// 轮询结果：仍等待 / 已就绪 / 出错。
#[derive(Debug)]
pub enum PollOutcome {
    Pending,
    Ready(TokenResp),
    Error(String),
}

/// 当前生效的后端根地址（`MOX_ID_BASE` 可覆盖）。公开给 TUI 账号页复用，
/// 避免界面层再硬编码一遍域名。
pub fn base_url() -> String {
    backend::id_base()
}

/// 取 device_token 落盘路径：`$MOX_ROOT/device_token`（MOX_ROOT 缺省 ~/.mox）。
fn token_path() -> PathBuf {
    let root = std::env::var("MOX_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".mox")
        });
    root.join("device_token")
}

/// 持久化 device_token（绑定的终端凭据）。
pub fn store_device_token(token: &str) -> Result<(), String> {
    let p = token_path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let mut f = std::fs::File::create(&p).map_err(|e| format!("写入 {p:?} 失败: {e}"))?;
    f.write_all(token.trim().as_bytes())
        .and_then(|_| f.write_all(b"\n"))
        .map_err(|e| format!("写 device_token 失败: {e}"))?;
    set_readonly(&p);
    Ok(())
}

#[cfg(unix)]
fn set_readonly(p: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_readonly(_p: &std::path::Path) {}

/// 读取已落盘的 device_token（已绑定返回 Some）。
pub fn load_device_token() -> Option<String> {
    let p = token_path();
    std::fs::read_to_string(&p).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

// ───────────────────────── 网络层（curl） ─────────────────────────

/// POST JSON；`claim` 非空时同时带 `X-Mox-Device-Claim` 头。
///
/// 后端已改为**强制设备声明**：不带声明的 `/device/code` 直接 400 不发码。
/// header + body 双通道是为了兼容会剥离自定义头的反代/CDN。
fn curl_post_json(url: &str, body: &str, claim: Option<&DeviceClaim>) -> Result<String, String> {
    let header;
    // 不用 -f：/device/token 的 authorization_pending / expired_token 等业务状态
    // 走 HTTP 400 + JSON body，`-f` 会把它们一律变成 curl 22，轮询永远「网络失败」。
    // 改用 -w 取状态码，4xx 的 body 交回解析层，只有 5xx 与传输层失败才报错。
    let mut args: Vec<&str> = vec![
        "-sS", "-X", "POST", "-H", "content-type: application/json", "-w", "\n%{http_code}",
    ];
    if let Some(c) = claim {
        header = format!("X-Mox-Device-Claim: {}", c.header_value());
        args.push("-H");
        args.push(&header);
    }
    args.extend_from_slice(&["--max-time", "20", "-d", body, url]);
    let out = Command::new("curl")
        .args(&args)
        .output()
        .map_err(|e| format!("调用 curl 失败（请确认系统已安装 curl）: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "请求 {} 失败 (curl {})：{}",
            url,
            out.status.code().unwrap_or(-1),
            stderr.trim()
        ));
    }
    let raw = String::from_utf8(out.stdout).map_err(|_| "响应不是合法 UTF-8".to_string())?;
    let (body, code_s) = raw.rsplit_once('\n').unwrap_or((raw.as_str(), "000"));
    let code: u32 = code_s.trim().parse().unwrap_or(0);
    if code >= 500 || code == 0 {
        return Err(format!("请求 {} 失败 (HTTP {})：{}", url, code, body.trim()));
    }
    Ok(body.to_string())
}

/// 当前设备声明（申请码与轮询共用同一份，服务端据此比对设备一致性）。
fn claim_for_request() -> Result<DeviceClaim, String> {
    device_claim::build()
}

/// 请求设备码（联网）。**必须携带设备声明**，否则后端拒绝签发。
pub fn request_code(base: &str) -> Result<DeviceCodeResp, String> {
    let claim = claim_for_request()?;
    let url = backend::join(base, "/device/code");
    let body = format!("{{\"device_claim\":{}}}", claim.canonical_json());
    let text = curl_post_json(&url, &body, Some(&claim))?;
    parse_device_code(&text)
}

/// 轮询换取 device_token（联网）。带同一设备声明，防止设备码被别的设备冒领。
pub fn poll_token(base: &str, device_code: &str) -> Result<PollOutcome, String> {
    let claim = claim_for_request()?;
    let url = backend::join(base, "/device/token");
    let body = format!(
        "{{\"device_code\":\"{}\",\"device_claim\":{}}}",
        escape_json(device_code),
        claim.canonical_json()
    );
    let text = curl_post_json(&url, &body, Some(&claim))?;
    Ok(parse_token(&text))
}

// ───────────────────────── 解析层（纯函数，可单测） ─────────────────────────

/// 从扁平 JSON 提取字符串字段 `"key":"value"`（处理转义引号）。
pub fn json_str(json: &str, key: &str) -> Option<String> {
    let pat = format!("\"{key}\"");
    let idx = json.find(&pat)? + pat.len();
    let rest = &json[idx..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    if !after.starts_with('"') {
        return None;
    }
    let mut chars = after[1..].chars();
    let mut val = String::new();
    while let Some(c) = chars.next() {
        if c == '"' {
            break;
        } else if c == '\\' {
            if let Some(n) = chars.next() {
                val.push(if n == 'n' { '\n' } else if n == 't' { '\t' } else { n });
            }
        } else {
            val.push(c);
        }
    }
    Some(val)
}

/// 从扁平 JSON 提取整数（可能为负数）。
pub fn json_i64(json: &str, key: &str) -> Option<i64> {
    let pat = format!("\"{key}\"");
    let idx = json.find(&pat)? + pat.len();
    let rest = &json[idx..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '-' && c != '+')?;
    after[..end].parse::<i64>().ok()
}

fn escape_json(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o
}

/// 解析 `/device/code` 响应。
pub fn parse_device_code(json: &str) -> Result<DeviceCodeResp, String> {
    let device_code = json_str(json, "device_code").ok_or("响应缺少 device_code")?;
    let user_code = json_str(json, "user_code").ok_or("响应缺少 user_code")?;
    let verification_uri_complete = json_str(json, "verification_uri_complete").unwrap_or_default();
    let expires_in = json_i64(json, "expires_in").unwrap_or(600) as u64;
    let interval = json_i64(json, "interval").unwrap_or(5).max(1) as u64;
    Ok(DeviceCodeResp {
        device_code,
        user_code,
        verification_uri_complete,
        expires_in,
        interval,
    })
}

/// 解析 `/device/token` 响应。
pub fn parse_token(json: &str) -> PollOutcome {
    if json.contains("authorization_pending") {
        return PollOutcome::Pending;
    }
    if json.contains("expired_token") || json.contains("invalid_grant") {
        return PollOutcome::Error("绑定已过期或失效，请重新运行 login".to_string());
    }
    if let Some(tok) = json_str(json, "device_token") {
        let user_id = json_i64(json, "user_id");
        return PollOutcome::Ready(TokenResp { device_token: tok, user_id });
    }
    PollOutcome::Error(format!("未知的设备码响应: {json}"))
}

/// 打印「扫码 / 手输」引导（tui 构建画半块二维码，默认构建纯文本降级）。
#[cfg(feature = "tui")]
fn print_binding_hint(uri: &str, user_code: &str) {
    // verification_uri_complete 编码进图，手机扫码即跳到 /activate 确认页。
    match crate::proot::qrcode_term::render_uri(uri) {
        Ok(qr) => println!("{qr}"),
        Err(e) => eprintln!("⚠ 二维码渲染失败（{e}），请手动打开下方链接完成绑定"),
    }
    println!();
    println!("请用手机浏览器/相机扫描上方二维码，或在手机打开：");
    println!("  {uri}");
    println!("验证码（若需手动输入）：{user_code}");
    println!("（若扫不出，请把终端行距设为 1.0）");
    println!();
}

/// 无 tui 时的等价降级：链接 + 验证码，后端 `/activate?code=` 同样能完成绑定。
#[cfg(not(feature = "tui"))]
fn print_binding_hint(uri: &str, user_code: &str) {
    println!("本构建未启用二维码渲染，请用手机浏览器打开：");
    println!("  {uri}");
    println!("验证码（若需手动输入）：{user_code}");
    println!();
}

/// 完整登录流程（供 `mox-proot login` 子命令调用）：发码 → 终端二维码 → 轮询 → 落盘。
///
/// 不依赖 TUI；纯 stdout 输出（二维码 + 提示），可被脚本/管道安全使用（二维码走 stderr 之外的
/// 人眼区，符合文档「人眼区/数据流分离」军规）。返回进程退出码：0 成功，非 0 失败。
pub fn run_login_flow() -> i32 {
    let base = base_url();
    eprintln!("==> 正在向 mox-id 申请设备码…");
    let resp = match request_code(&base) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("✗ 申请设备码失败：{e}");
            return 2;
        }
    };
    print_binding_hint(&resp.verification_uri_complete, &resp.user_code);

    // 轮询（尊重 interval，超时按 expires_in 截止）
    eprintln!("⠸ 等待手机确认…（按 R 重新生成 / Ctrl-C 取消）");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(resp.expires_in);
    let interval = std::time::Duration::from_secs(resp.interval.max(1));
    loop {
        if std::time::Instant::now() >= deadline {
            eprintln!("✗ 等待超时（{}s），请重新运行 mox-proot login", resp.expires_in);
            return 2;
        }
        std::thread::sleep(interval);
        match poll_token(&base, &resp.device_code) {
            Ok(PollOutcome::Pending) => continue,
            Ok(PollOutcome::Ready(t)) => {
                if let Err(e) = store_device_token(&t.device_token) {
                    eprintln!("✗ 保存 device_token 失败：{e}");
                    return 2;
                }
                eprintln!("✓ 绑定成功，device_token 已保存。");
                return 0;
            }
            Ok(PollOutcome::Error(e)) => {
                eprintln!("✗ {e}");
                return 2;
            }
            Err(e) => {
                eprintln!("✗ 轮询失败：{e}");
                return 2;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_device_code_extracts_fields() {
        let j = r#"{"device_code":"abc123","user_code":"A7K2-9FQ1","verification_uri":"https://x.dev/activate","verification_uri_complete":"https://x.dev/activate?code=A7K2-9FQ1","expires_in":600,"interval":5}"#;
        let r = parse_device_code(j).unwrap();
        assert_eq!(r.device_code, "abc123");
        assert_eq!(r.user_code, "A7K2-9FQ1");
        assert_eq!(r.verification_uri_complete, "https://x.dev/activate?code=A7K2-9FQ1");
        assert_eq!(r.expires_in, 600);
        assert_eq!(r.interval, 5);
    }

    #[test]
    fn parse_token_pending_then_ready() {
        assert!(matches!(parse_token(r#"{"error":"authorization_pending"}"#), PollOutcome::Pending));
        let ready = parse_token(r#"{"access_token":"tok-1","token_type":"device","device_token":"tok-1","user_id":42,"expires_in":500}"#);
        match ready {
            PollOutcome::Ready(t) => {
                assert_eq!(t.device_token, "tok-1");
                assert_eq!(t.user_id, Some(42));
            }
            _ => panic!("应为 Ready"),
        }
        assert!(matches!(parse_token(r#"{"error":"expired_token"}"#), PollOutcome::Error(_)));
        assert!(matches!(parse_token(r#"{"error":"invalid_grant"}"#), PollOutcome::Error(_)));
    }

    #[test]
    fn json_str_handles_escaped_quotes() {
        let j = r#"{"url":"https://x.dev/a?b=\"c\""}"#;
        assert_eq!(json_str(j, "url").unwrap(), "https://x.dev/a?b=\"c\"");
    }

    #[test]
    fn json_i64_signed() {
        assert_eq!(json_i64(r#"{"user_id":-123}"#, "user_id"), Some(-123));
        assert_eq!(json_i64(r#"{"user_id":0}"#, "user_id"), Some(0));
    }

    #[test]
    fn escape_json_basic() {
        assert_eq!(escape_json("a\"b\\c"), "a\\\"b\\\\c");
    }
}
