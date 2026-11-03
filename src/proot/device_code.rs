//! 设备码授权客户端（RFC 8628），仅 `tui` feature 启用。
//!
//! 流程（与 mox-id 后端 `/device/code` `/device/token` `/activate` 对齐）：
//!   ① [`request_code`] → device_code + user_code + verification_uri_complete
//!   ② 把 verification_uri_complete 交给 [`crate::proot::qrcode_term`] 渲染成终端二维码
//!   ③ 手机扫码打开 /activate，用 GitHub / Gitee 登录并确认绑定
//!   ④ [`poll_token`] 轮询：pending 返回 `None`，authorized 返回 device_token 并落盘
//!
//! HTTP 走系统 `curl`（CLI 工具本就依赖 curl，零 TLS 依赖；引擎 cdylib 不编入本模块）。
//! JSON 解析为手写最小提取器（响应为扁平字段，无需 serde），且**与网络调用分离**便于单测。

#![cfg(feature = "tui")]

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

/// 后端基址默认常量（部署后替换为真实 mox-id 域名）。可用环境变量 `MOX_ID_BASE` 覆盖。
pub const DEFAULT_BASE: &str = "https://mox-id.moxsh.app";

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

fn base_url() -> String {
    std::env::var("MOX_ID_BASE")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_BASE.to_string())
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
    let _ = set_readonly(&p);
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

fn curl_post_json(url: &str, body: &str) -> Result<String, String> {
    let out = Command::new("curl")
        .args(["-fsS", "-X", "POST", "-H", "content-type: application/json", "--max-time", "20", "-d", body, url])
        .output()
        .map_err(|e| format!("调用 curl 失败（请确认系统已安装 curl）: {e}"))?;
    if !out.status.success() {
        return Err(format!("请求 {} 失败 (HTTP {})", url, out.status.code().unwrap_or(-1)));
    }
    String::from_utf8(out.stdout).map_err(|_| "响应不是合法 UTF-8".to_string())
}

/// 请求设备码（联网）。
pub fn request_code(base: &str) -> Result<DeviceCodeResp, String> {
    let url = format!("{}/device/code", base.trim_end_matches('/'));
    let text = curl_post_json(&url, "{}")?;
    parse_device_code(&text)
}

/// 轮询换取 device_token（联网）。
pub fn poll_token(base: &str, device_code: &str) -> Result<PollOutcome, String> {
    let url = format!("{}/device/token", base.trim_end_matches('/'));
    let body = format!("{{\"device_code\":\"{}\"}}", escape_json(device_code));
    let text = curl_post_json(&url, &body)?;
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

/// 完整登录流程（供 `moxsh-proot login` 子命令调用）：发码 → 终端二维码 → 轮询 → 落盘。
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
    // 渲染二维码（verification_uri_complete 编码进图，手机扫码即跳到 /activate 确认页）
    match crate::proot::qrcode_term::render_uri(&resp.verification_uri_complete) {
        Ok(qr) => {
            println!("{qr}");
        }
        Err(e) => {
            eprintln!("⚠ 二维码渲染失败（{e}），可手动打开下方链接完成绑定");
        }
    }
    println!();
    println!("请用手机浏览器/相机扫描上方二维码，或在手机打开：");
    println!("  {}", resp.verification_uri_complete);
    println!("验证码（若需手动输入）：{}", resp.user_code);
    println!("（若扫不出，请把终端行距设为 1.0）");
    println!();

    // 轮询（尊重 interval，超时按 expires_in 截止）
    eprintln!("⠸ 等待手机确认…（按 R 重新生成 / Ctrl-C 取消）");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(resp.expires_in);
    let interval = std::time::Duration::from_secs(resp.interval.max(1));
    loop {
        if std::time::Instant::now() >= deadline {
            eprintln!("✗ 等待超时（{}s），请重新运行 moxsh-proot login", resp.expires_in);
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
