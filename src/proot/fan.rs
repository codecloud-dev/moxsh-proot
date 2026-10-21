//! 粉丝专属客户端（0.6.0）——「脚本 ↔ 官网」握手，**在线校验 + localhost 回环自动收码**。
//!
//! 流程（手机 / Termux，灰度首发于 moxsh-proot 脚本，旗舰 moxsh 后上）：
//!   1. 粉丝在终端敲 `mox fan login`（入口命令由公众号自动回复告知，**手动触发，
//!      不自动跳**）→ 脚本在 `127.0.0.1` 起一个一次性回环监听 → 唤起手机浏览器打开
//!      **隐藏粉丝页**（官网导航点不到那个），URL 带上 `redirect=<回环地址>&state=<随机>`；
//!   2. 粉丝页内 **GitHub 登录**（`/auth/github/start?redirect=<粉丝页>`，登录后回到粉丝页）；
//!   3. 点「确认领取」= 后端 `POST /fan/issue` 用**服务端密钥**签发一个激活码，
//!      页面带 `code` 跳回 `redirect`（回环）→ 脚本**自动收到**码；
//!   4. 脚本 `POST /fan/verify {code}` **联网经官方真实校验**（服务端持密钥、可吊销）
//!      → 通过则把档位/到期落盘 `$MOXSH_ROOT/fan.toml` → 解锁 fan_only 配方。
//!
//! 桌面 Linux（没浏览器可调）：官网手动取激活码 → `mox fan redeem <code>`（同样联网验）。
//!
//! 设计约束（与全仓库一致）：
//! - **零外部依赖**：不做任何客户端加解密——激活码由服务端签发、服务端校验，手机只
//!   `curl` 转发（网络层用 curl，Rust 不引 reqwest）。这比"客户端 HMAC 自带密钥"更硬：
//!   密钥只在服务端，手机二进制里没有可伪造材料。
//! - **灰度开关**：`MOXSH_FAN_ENABLE=1` 才启用（默认关）；后端 `/fan/issue` 另有
//!   GitHub 白名单，双保险（见 cli.rs `cmd_fan`）。
//! - **不做设备绑定/自动恢复**：删了本地 `fan.toml` 就重新登录领取（项目主人拍板）。
//!
//! 与后端（mox-id）的验签契约（实现后端时按此对齐）：
//!   `POST <MOXSH_REDEEM_API>`  body `{"code":"<激活码>"}`
//!     成功 → HTTP 200，body 为一行：`ok <tier> <exp_unix_秒>`
//!     失败 → 非 200 或 body 非 `ok` 开头（如 `err not_in_canary` / `err bad_code` / `err expired`）
//!   激活码本身由后端 HS256（服务端 `JWT_SECRET`/`FAN_SECRET`）签发，手机不解析、只转发。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::proot::{ProotError, ProotResult};

/// 令牌文件相对 `$MOXSH_ROOT` 的位置（账号级，删容器不删它；删了需重新领取）。
pub const FAN_FILE_REL: &str = "fan.toml";

/// 灰度总开关：`MOXSH_FAN_ENABLE=1` 才启用登录/兑换（默认关）。
pub const FAN_ENABLE_ENV: &str = "MOXSH_FAN_ENABLE";
/// 隐藏粉丝页 URL（`MOXSH_FAN_PAGE` 覆盖；默认占位，部署后改或设环境变量）。
pub const FAN_PAGE_ENV: &str = "MOXSH_FAN_PAGE";
/// 后端验签端点（`MOXSH_REDEEM_API` 覆盖；默认占位）。
pub const REDEEM_API_ENV: &str = "MOXSH_REDEEM_API";

/// 回环等待浏览器回跳的超时（粉丝要登录+点领取，给宽点）。
const LOOPBACK_TIMEOUT: Duration = Duration::from_secs(240);

/// 已落盘的粉丝状态。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FanState {
    pub code: String,
    pub tier: String,
    /// 过期 unix 秒；0 = 永久。
    pub exp: u64,
}

/// 唤起浏览器的结果。
#[derive(Debug, Clone, PartialEq)]
pub enum Opened {
    Via(String),
    Failed,
}

/// 灰度是否开启（`MOXSH_FAN_ENABLE=1`）。
pub fn fan_enabled() -> bool {
    std::env::var(FAN_ENABLE_ENV).as_deref() == Ok("1")
}

/// 当前验签模式（诊断用）——在线为唯一模式。
pub fn verify_mode() -> &'static str {
    "online"
}

/// 隐藏粉丝页 URL（`MOXSH_FAN_PAGE` 覆盖）。
/// 默认指向官网（GitHub Pages）的隐藏页 fan.html——导航里没有入口，
/// 只能由本命令带 redirect/state 唤起（项目主人拍板：不给密钥 URL）。
pub fn fan_page_url() -> String {
    std::env::var(FAN_PAGE_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://codecloud-dev.github.io/mox-site/fan.html".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 后端验签端点（`MOXSH_REDEEM_API` 覆盖）。
fn redeem_api() -> String {
    std::env::var(REDEEM_API_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://mox-id.3042980037.workers.dev/fan/verify".to_string())
}

/// 当前 unix 秒（时钟异常回落 0）。
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 随机 state（pid ^ 纳秒，十六进制；防回调伪造/串话）。
fn make_state() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), t)
}

// ============================ 令牌落盘（$MOXSH_ROOT/fan.toml） ============================

/// 令牌文件绝对路径。
pub fn fan_file_path(root: &Path) -> PathBuf {
    root.join(FAN_FILE_REL)
}

/// 读已落盘粉丝状态（无文件/过期 = None）。
pub fn load_state(root: &Path) -> Option<FanState> {
    let text = std::fs::read_to_string(fan_file_path(root)).ok()?;
    let mut st = FanState::default();
    let mut in_fan = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line == "[fan]" {
            in_fan = true;
            continue;
        }
        if line.starts_with('[') {
            in_fan = false;
            continue;
        }
        if !in_fan {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "code" => st.code = v.to_string(),
                "tier" => st.tier = v.to_string(),
                "exp" => st.exp = v.parse().unwrap_or(0),
                _ => {}
            }
        }
    }
    if st.code.is_empty() {
        return None;
    }
    if st.exp != 0 && now_unix() >= st.exp {
        return None; // 过期文件等同未解锁
    }
    Some(st)
}

/// 写粉丝状态（先写临时再改名，尽量原子）。
pub fn save_state(root: &Path, state: &FanState) -> ProotResult<()> {
    std::fs::create_dir_all(root).map_err(ProotError::Io)?;
    let text = format!(
        "# moxsh 粉丝令牌（由 `mox fan` 写入，请勿手改）。删除本文件 = 解除，需重新登录领取。\n\
         [fan]\n\
         code = \"{}\"\n\
         tier = \"{}\"\n\
         exp = {}\n",
        state.code, state.tier, state.exp
    );
    let path = fan_file_path(root);
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(ProotError::Io)?;
    std::fs::rename(&tmp, &path).map_err(ProotError::Io)?;
    Ok(())
}

/// 是否已解锁（有未过期令牌）。
pub fn is_fan(root: &Path) -> bool {
    load_state(root).is_some()
}

// ============================ 在线校验（curl 转发，零 Rust 加密） ============================

/// 联网校验激活码：`POST <API> {"code":..}` → 解析 `ok <tier> <exp>`。
/// 契约见模块文档；失败返回可读原因。
pub fn verify_online(code: &str) -> ProotResult<(String, u64)> {
    let api = redeem_api();
    // 极简 JSON：code 只含 base64url + '.'，剥掉引号防注入即可。
    let payload = format!("{{\"code\":\"{}\"}}", code.trim().replace('"', ""));
    let out = Command::new("curl")
        .args(["-sS", "--connect-timeout", "10", "--max-time", "20", "-X", "POST"])
        .arg(&api)
        .args(["-H", "Content-Type: application/json", "--data", &payload])
        .arg("-w")
        .arg("\n%{http_code}")
        .output()
        .map_err(ProotError::Io)?;
    if !out.status.success() {
        return Err(ProotError::InvalidArg(
            "联网校验失败：无法执行 curl（网络或环境问题）".to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, http) = text.rsplit_once('\n').unwrap_or((text.as_ref(), ""));
    if http.trim() != "200" {
        return Err(ProotError::InvalidArg(format!(
            "官方校验未通过（HTTP {}）：{}",
            http.trim(),
            body.trim()
        )));
    }
    let mut it = body.split_whitespace();
    if it.next() != Some("ok") {
        return Err(ProotError::InvalidArg(format!(
            "官方校验未通过：{}",
            body.trim()
        )));
    }
    let tier = it.next().unwrap_or("fan").to_string();
    let exp = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok((tier, exp))
}

/// 兑换（手动路径，桌面 Linux 也用这个）：联网验 → 落盘。
pub fn redeem(root: &Path, code: &str) -> ProotResult<String> {
    let (tier, exp) = verify_online(code)?;
    save_state(
        root,
        &FanState {
            code: code.trim().to_string(),
            tier: tier.clone(),
            exp,
        },
    )?;
    Ok(tier)
}

// ============================ localhost 回环（自动收码） ============================

/// 由粉丝页 URL 拼出带回环 redirect + state 的 URL。
/// `page` 形如 `https://site/fan`；拼成 `https://site/fan?redirect=..&state=..`
/// （page 已带 query 则用 `&` 续）。
pub fn build_fan_url(page: &str, loopback: &str, state: &str) -> String {
    let sep = if page.contains('?') { '&' } else { '?' };
    format!("{}{}redirect={}&state={}", page, sep, url_encode(loopback), url_encode(state))
}

/// 极简 URL 百分号编码（只编码回环/state 用到的少量字符；激活码走 query 亦安全）。
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// 从回环收到的 HTTP 请求里解析 `code` 与 `state`（取请求行的 query）。
/// 请求行形如：`GET /done?code=<码>&state=<态> HTTP/1.1`。
pub fn parse_callback(request: &str) -> Option<(String, String)> {
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?; // 跳过 method
    let (_, query) = target.split_once('?')?;
    let mut code = None;
    let mut state = None;
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "code" => code = Some(v.to_string()),
            "state" => state = Some(v.to_string()),
            _ => {}
        }
    }
    match (code, state) {
        (Some(c), Some(s)) if !c.is_empty() => Some((c, s)),
        _ => None,
    }
}

/// 回给浏览器的成功页（提示可关闭）。
fn success_html() -> String {
    "<!DOCTYPE html><meta charset=utf-8><title>moxsh</title>\
     <body style=\"font-family:sans-serif;text-align:center;padding:60px\">\
     <h2>✅ 已领取，返回终端查看</h2><p>可以关闭这个页面了。</p>"
        .to_string()
}

/// 在已绑定的 listener 上等待一次回环回调，校验 state，返回激活码。
/// `listener` 须已 `bind`（调用方先用它拿 port 拼 redirect）。
pub fn await_code(listener: &TcpListener, expect_state: &str) -> ProotResult<String> {
    listener.set_nonblocking(true).map_err(ProotError::Io)?;
    let deadline = Instant::now() + LOOPBACK_TIMEOUT;
    let mut stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(ProotError::InvalidArg(
                        "等待浏览器领取超时（请重试 mox fan login）".to_string(),
                    ));
                }
                std::thread::sleep(Duration::from_millis(150));
            }
            Err(e) => return Err(ProotError::Io(e)),
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap_or(0);
    let req = String::from_utf8_lossy(&buf[..n]);
    let mut stream = stream;
    let _ = stream.write_all(success_html().as_bytes());
    let _ = stream.flush();

    match parse_callback(&req) {
        Some((code, state)) if state == expect_state => Ok(code),
        Some(_) => Err(ProotError::InvalidArg("回环 state 不匹配，已取消".to_string())),
        None => Err(ProotError::InvalidArg("回环请求里没有有效 code".to_string())),
    }
}

/// [`login`] 的结果：档位 + 实际打开的粉丝页 URL（浏览器唤起失败时 CLI 需
/// 提示用户手动打开该 URL）。
#[derive(Debug, Clone, PartialEq)]
pub struct LoginOutcome {
    pub tier: String,
    pub url: String,
    pub opened: Opened,
}

/// 完整登录流程：起回环 → 开浏览器 → 收码 → 联网验 → 落盘。返回档位与 URL。
pub fn login(root: &Path) -> ProotResult<LoginOutcome> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(ProotError::Io)?;
    let port = listener.local_addr().map_err(ProotError::Io)?.port();
    let state = make_state();
    let loopback = format!("http://127.0.0.1:{}/done", port);
    let url = build_fan_url(&fan_page_url(), &loopback, &state);
    let opened = open_browser(&url); // 开浏览器（失败也继续等，用户可手动开 URL）
    let code = await_code(&listener, &state)?;
    let tier = redeem(root, &code)?;
    Ok(LoginOutcome { tier, url, opened })
}

// ============================ 唤起手机浏览器 ============================

fn try_launch(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// 唤起浏览器：`termux-open-url` → `am start -a android.intent.action.VIEW`（含
/// `/system/bin/am` 兜底）→ `xdg-open`。全失败返回 [`Opened::Failed`]。
pub fn open_browser(url: &str) -> Opened {
    if try_launch("termux-open-url", &[url]) {
        return Opened::Via("termux-open-url".into());
    }
    if try_launch("am", &["start", "-a", "android.intent.action.VIEW", "-d", url]) {
        return Opened::Via("am".into());
    }
    for am in ["/system/bin/am", "/system/xbin/am"] {
        if std::path::Path::new(am).exists()
            && try_launch(am, &["start", "-a", "android.intent.action.VIEW", "-d", url])
        {
            return Opened::Via(am.into());
        }
    }
    if try_launch("xdg-open", &[url]) {
        return Opened::Via("xdg-open".into());
    }
    Opened::Failed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moxsh-fan2-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    // ---- URL 拼装 ----
    #[test]
    fn build_fan_url_appends_query() {
        let u = build_fan_url("https://site/fan", "http://127.0.0.1:5000/done", "abc123");
        assert!(u.starts_with("https://site/fan?redirect="));
        assert!(u.contains("&state=abc123"));
        assert!(u.contains("127.0.0.1%3A5000")); // ':' 被编码
    }

    #[test]
    fn build_fan_url_amp_when_page_has_query() {
        let u = build_fan_url("https://site/fan?x=1", "http://127.0.0.1:1/done", "s");
        assert!(u.contains("fan?x=1&redirect="));
    }

    #[test]
    fn url_encode_spec() {
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(url_encode("http://x:1/"), "http%3A%2F%2Fx%3A1%2F");
    }

    // ---- 回环请求解析 ----
    #[test]
    fn parse_callback_ok() {
        let req = "GET /done?code=dm9vMQ.abc&state=deadbeef HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let (code, state) = parse_callback(req).unwrap();
        assert_eq!(code, "dm9vMQ.abc");
        assert_eq!(state, "deadbeef");
    }

    #[test]
    fn parse_callback_rejects_bad() {
        assert!(parse_callback("").is_none());
        assert!(parse_callback("GET /done HTTP/1.1").is_none()); // 无 query
        assert!(parse_callback("GET /done?state=x HTTP/1.1").is_none()); // 无 code
        assert!(parse_callback("GET /done?code= HTTP/1.1").is_none()); // 空 code
    }

    #[test]
    fn parse_callback_ignores_extra_params() {
        let req = "GET /done?foo=1&code=C.S&state=ST&bar=2 HTTP/1.1";
        let (c, s) = parse_callback(req).unwrap();
        assert_eq!(c, "C.S");
        assert_eq!(s, "ST");
    }

    // ---- 落盘 / 状态 ----
    #[test]
    fn save_load_roundtrip_and_is_fan() {
        let root = tmp("store");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!is_fan(&root));
        save_state(&root, &FanState { code: "c.s".into(), tier: "fan".into(), exp: 0 }).unwrap();
        assert!(is_fan(&root));
        let st = load_state(&root).unwrap();
        assert_eq!(st.code, "c.s");
        assert_eq!(st.tier, "fan");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_state_ignores_expired_file() {
        let root = tmp("expfile");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(fan_file_path(&root), "[fan]\ncode=\"x\"\ntier=\"fan\"\nexp=1\n").unwrap();
        assert!(!is_fan(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 在线校验 / 灰度：只验纯逻辑，不打真实网络 ----
    #[test]
    fn online_defaults_shape() {
        assert!(fan_page_url().starts_with("http"));
        assert_eq!(verify_mode(), "online");
    }

    #[test]
    fn fan_enabled_matches_env() {
        assert_eq!(fan_enabled(), std::env::var(FAN_ENABLE_ENV).as_deref() == Ok("1"));
    }

    // ---- 回环：真实 localhost 收一次包，端到端验证 await_code ----
    #[test]
    fn await_code_end_to_end_on_localhost() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            let req = "GET /done?code=THECODE.SIG&state=st-xyz HTTP/1.1\r\n\r\n";
            s.write_all(req.as_bytes()).unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf); // 读成功页
        });
        let code = await_code(&listener, "st-xyz").expect("应收到 code");
        assert_eq!(code, "THECODE.SIG");
        handle.join().unwrap();
    }

    #[test]
    fn await_code_rejects_state_mismatch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            s.write_all(b"GET /done?code=X.S&state=WRONG HTTP/1.1\r\n\r\n").unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf);
        });
        let err = await_code(&listener, "RIGHT").unwrap_err();
        assert!(matches!(err, ProotError::InvalidArg(_)));
        handle.join().unwrap();
    }
}
