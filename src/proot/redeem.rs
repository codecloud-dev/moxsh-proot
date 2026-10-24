//! 通用商品授权客户端（与 fan 灰度系统**并列互不影响**）——「脚本 ↔ 官网」握手，
//! **设备绑定（回环）+ 授权校验（在线）**。
//!
//! 定位：付费商品（proot 引擎增值版等，后台可加）的设备级授权。fan 是**账号级**
//! 粉丝福利（`mox fan`，`$MOXSH_ROOT/fan.toml`）；本模块是**设备级**商品授权
//! （`mox redeem`，`$MOXSH_ROOT/device.toml` + `entitlements.toml`），两套文件、
//! 两条命令、互不覆盖（项目主人拍板：粉丝系统不许被吃掉）。
//!
//! 流程（手机 / Termux）：
//!   1. `mox redeem login`（手动触发，不自动跳）→ 引擎在 `127.0.0.1` 起一次性回环
//!      → 唤起浏览器打开官网绑定页 `bind.html`，URL 带
//!      `redirect=<回环>&state=<随机>&device_token=<本机令牌>&name=&platform=`；
//!   2. 页内 **GitHub 登录** → 点「绑定此设备」= 后端 `POST /devices/register`
//!      把令牌挂到账号 → 页面带 `ok=1&state` 跳回回环 → 脚本**自动收到确认**；
//!   3. 买家在官网兑换页（redeem.html）用**兑换码或爱发电订单号**把授权赋予
//!      这台设备（后端 `/products/redeem`，设备归属已校验）；
//!   4. `mox redeem status` → `GET /entitlement/check?device_token=..` **联网拉取**
//!      全部授权（纯文本，一行一个 `ok <product_id> <tier> <exp>`）→ 落盘
//!      `entitlements.toml` 缓存 → 增值配方（`requires = "<product_id>"`）按缓存门控。
//!
//! 设计约束（与全仓库一致）：
//! - **零外部依赖**：不做任何客户端加解密——授权判定完全在服务端，手机只 `curl`
//!   转发（Rust 不引 reqwest）。设备令牌是本地随机串（URL 安全），泄漏=换个设备，
//!   不含任何可伪造材料。
//! - **缓存 + TTL**：`entitlements.toml` 带 `fetched_at`，超过 [`ENTS_TTL_DAYS`] 天
//!   视为陈旧（门控显示 🔒 并提示 `mox redeem status` 刷新）；吊销在 TTL 内生效。
//! - **无灰度开关**：商品是正式商业功能（不像 fan 有 canary），随时可用。
//!
//! 与后端（mox-id）的契约（实现/改动任一侧必须同步另一侧）：
//!   `POST /devices/register`（登录态）body `{"device_token":"..","name":"..","platform":".."}`
//!     成功 → 200 `{ok:true, device_id, device_token}`；他人已占用 → 403
//!   `GET  /entitlement/check?device_token=<hex>`（无登录，token 即凭据）
//!     成功 → 200 纯文本，一行一个 `ok <product_id> <tier> <exp_unix秒>`（exp 0=永久）
//!     无授权 → 404 `err not_found`；缺 token → 400 `err missing_device_token`
//!   绑定页回跳脚本回环时带 `ok=1&state=<原样回传>`（state 须一致，防伪造）。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::proot::{ProotError, ProotResult};

/// 设备身份文件相对 `$MOXSH_ROOT` 的位置（删容器不删它；删了=换设备，需重新绑定）。
pub const DEVICE_FILE_REL: &str = "device.toml";
/// 授权缓存文件（`mox redeem status` 拉取后落盘；门禁只读缓存，不联网）。
pub const ENTS_FILE_REL: &str = "entitlements.toml";
/// 绑定页 URL（`MOXSH_BIND_PAGE` 覆盖）。
pub const BIND_PAGE_ENV: &str = "MOXSH_BIND_PAGE";
/// 授权校验端点（`MOXSH_ENTITLEMENT_API` 覆盖）。
pub const ENTITLEMENT_API_ENV: &str = "MOXSH_ENTITLEMENT_API";
/// 设备显示名（`MOXSH_DEVICE_NAME` 覆盖；默认取 hostname）。
pub const DEVICE_NAME_ENV: &str = "MOXSH_DEVICE_NAME";

/// 回环等待浏览器「登录 + 点绑定」的超时（买家要登录，给宽点）。
const LOOPBACK_TIMEOUT: Duration = Duration::from_secs(240);
/// 授权缓存陈旧阈值（天）：超过则门控显示未授权并提示刷新。
pub const ENTS_TTL_DAYS: u64 = 7;
/// 设备令牌长度（hex 字符数；32 hex = 128-bit 随机）。
const TOKEN_HEX_LEN: usize = 32;

/// 本机设备身份（`device.toml`）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceInfo {
    pub token: String,
    pub name: String,
    pub platform: String,
}

/// 一条商品授权（来自 `/entitlement/check` 的一行 `ok`）。
#[derive(Debug, Clone, PartialEq)]
pub struct Entitlement {
    pub product_id: String,
    pub tier: String,
    /// 过期 unix 秒；0 = 永久。
    pub exp: u64,
}

/// 授权缓存（`entitlements.toml`）：列表 + 拉取时间。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntCache {
    pub ents: Vec<Entitlement>,
    /// 拉取时间（unix 秒）；0 = 从未成功拉取。
    pub fetched_at: u64,
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

// ============================ 设备身份（device.toml） ============================

/// 设备文件绝对路径。
pub fn device_file_path(root: &Path) -> PathBuf {
    root.join(DEVICE_FILE_REL)
}

/// 读本机设备身份（无文件/无 token = None）。
pub fn load_device(root: &Path) -> Option<DeviceInfo> {
    let text = std::fs::read_to_string(device_file_path(root)).ok()?;
    let mut d = DeviceInfo::default();
    let mut in_dev = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line == "[device]" {
            in_dev = true;
            continue;
        }
        if line.starts_with('[') {
            in_dev = false;
            continue;
        }
        if !in_dev {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "token" => d.token = v.to_string(),
                "name" => d.name = v.to_string(),
                "platform" => d.platform = v.to_string(),
                _ => {}
            }
        }
    }
    if d.token.is_empty() {
        return None;
    }
    Some(d)
}

/// 写设备身份（先写临时再改名，尽量原子）。
pub fn save_device(root: &Path, dev: &DeviceInfo) -> ProotResult<()> {
    std::fs::create_dir_all(root).map_err(ProotError::Io)?;
    let text = format!(
        "# moxsh 设备身份（由 `mox redeem` 写入，请勿手改）。删除本文件 = 放弃本设备，需重新绑定。\n\
         [device]\n\
         token = \"{}\"\n\
         name = \"{}\"\n\
         platform = \"{}\"\n",
        dev.token, dev.name, dev.platform
    );
    let path = device_file_path(root);
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(ProotError::Io)?;
    std::fs::rename(&tmp, &path).map_err(ProotError::Io)?;
    Ok(())
}

/// 生成本机设备令牌：优先 `/dev/urandom` 读 16 字节转 32 hex；
/// 读不到时用 pid^纳秒哈希兜底（进程内随机性足够防猜测）。
pub fn generate_token() -> String {
    let mut buf = [0u8; 16];
    let ok = std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_ok();
    if !ok {
        // 兜底：pid + 纳秒 + 一个栈地址混合（不引 rand  crate，零依赖）
        let t = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0) as u64;
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (std::process::id() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for b in t.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x1000_0000_01b3);
        }
        buf.copy_from_slice(&h.to_le_bytes());
        let t2 = t.rotate_left(17) ^ (&buf as *const _ as u64);
        buf[8..].copy_from_slice(&t2.to_le_bytes());
    }
    let mut s = String::with_capacity(TOKEN_HEX_LEN);
    for b in buf.iter().take(TOKEN_HEX_LEN / 2) {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 探测平台标识（android / linux / macos / windows）。
pub fn detect_platform() -> String {
    // Termux：$PREFIX 指向 /data/user/0/com.termux/files/usr
    if let Ok(p) = std::env::var("PREFIX") {
        if p.contains("com.termux") {
            return "android".into();
        }
    }
    if std::env::var("TERMUX_VERSION").is_ok() {
        return "android".into();
    }
    if cfg!(target_os = "macos") {
        return "macos".into();
    }
    if cfg!(target_os = "windows") {
        return "windows".into();
    }
    "linux".into()
}

/// 探测设备显示名（`MOXSH_DEVICE_NAME` > hostname > 通用名）。
pub fn detect_name() -> String {
    if let Ok(n) = std::env::var(DEVICE_NAME_ENV) {
        let n = n.trim();
        if !n.is_empty() {
            return n.chars().take(64).collect();
        }
    }
    if let Ok(o) = Command::new("hostname").output() {
        if o.status.success() {
            let h = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !h.is_empty() {
                return h.chars().take(64).collect();
            }
        }
    }
    "moxsh 设备".into()
}

/// 取本机设备身份：有文件直接用；没有则生成令牌并落盘（**未绑定**，仅 `login` 后会生效）。
pub fn ensure_device(root: &Path) -> ProotResult<DeviceInfo> {
    if let Some(d) = load_device(root) {
        return Ok(d);
    }
    let d = DeviceInfo {
        token: generate_token(),
        name: detect_name(),
        platform: detect_platform(),
    };
    save_device(root, &d)?;
    Ok(d)
}

// ============================ 在线校验（curl 转发，零 Rust 加密） ============================

/// 绑定页 URL（`MOXSH_BIND_PAGE` 覆盖）。
/// 默认指向官网 pages.dev 的隐藏页 bind.html——导航里没有入口，
/// 只能由本命令带 redirect/state/device_token 唤起（与 fan 页同思路）。
pub fn bind_page_url() -> String {
    std::env::var(BIND_PAGE_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://mox-site.pages.dev/bind.html".to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 授权校验端点（`MOXSH_ENTITLEMENT_API` 覆盖）。
/// 默认走**官网同域反代**（pages.dev/entitlement/check）——Worker 标准域
/// workers.dev 国内不可达（爱发电 webhook 443 同款坑），pages.dev 实测可达。
fn entitlement_api() -> String {
    std::env::var(ENTITLEMENT_API_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://mox-site.pages.dev/entitlement/check".to_string())
}

/// 联网拉取本机全部授权：`GET <api>?device_token=<hex>` → 逐行解析 `ok <pid> <tier> <exp>`。
/// 404（无授权）是正常空态，返回空列表而非错误。
pub fn fetch_entitlements(token: &str) -> ProotResult<Vec<Entitlement>> {
    let api = entitlement_api();
    let url = format!("{}?device_token={}", api, token.trim());
    let out = Command::new("curl")
        .args(["-sS", "--connect-timeout", "10", "--max-time", "20"])
        .arg(&url)
        .arg("-w")
        .arg("\n%{http_code}")
        .output()
        .map_err(ProotError::Io)?;
    if !out.status.success() {
        return Err(ProotError::InvalidArg(
            "联网拉取授权失败：无法执行 curl（网络或环境问题）".to_string(),
        ));
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (body, http) = text.rsplit_once('\n').unwrap_or((text.as_ref(), ""));
    match http.trim() {
        "200" => {}
        "404" => return Ok(Vec::new()), // 无授权：正常空态
        other => {
            return Err(ProotError::InvalidArg(format!(
                "授权查询未通过（HTTP {}）：{}",
                other,
                body.trim()
            )));
        }
    }
    let mut ents = Vec::new();
    for line in body.lines() {
        let mut it = line.split_whitespace();
        if it.next() != Some("ok") {
            continue; // 容忍空行/杂讯行
        }
        let product_id = it.next().unwrap_or("").to_string();
        if product_id.is_empty() {
            continue;
        }
        let tier = it.next().unwrap_or("").to_string();
        let exp = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
        ents.push(Entitlement {
            product_id,
            tier,
            exp,
        });
    }
    Ok(ents)
}

// ============================ 授权缓存（entitlements.toml） ============================

/// 授权缓存文件绝对路径。
pub fn ents_file_path(root: &Path) -> PathBuf {
    root.join(ENTS_FILE_REL)
}

/// 写授权缓存（原子替换）。
pub fn save_ents(root: &Path, ents: &[Entitlement]) -> ProotResult<()> {
    std::fs::create_dir_all(root).map_err(ProotError::Io)?;
    let mut text = format!(
        "# moxsh 商品授权缓存（由 `mox redeem status` 拉取后写入，请勿手改）。\n\
         [cache]\n\
         fetched_at = {}\n",
        now_unix()
    );
    for e in ents {
        text.push_str(&format!(
            "[[ent]]\nproduct_id = \"{}\"\ntier = \"{}\"\nexp = {}\n",
            e.product_id, e.tier, e.exp
        ));
    }
    let path = ents_file_path(root);
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(ProotError::Io)?;
    std::fs::rename(&tmp, &path).map_err(ProotError::Io)?;
    Ok(())
}

/// 读授权缓存（无文件 = None）。
pub fn load_ents(root: &Path) -> Option<EntCache> {
    let text = std::fs::read_to_string(ents_file_path(root)).ok()?;
    let mut c = EntCache::default();
    let mut cur: Option<Entitlement> = None;
    let mut in_cache = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if line == "[cache]" {
            in_cache = true;
            continue;
        }
        if line == "[[ent]]" {
            if let Some(e) = cur.take() {
                c.ents.push(e);
            }
            cur = Some(Entitlement {
                product_id: String::new(),
                tier: String::new(),
                exp: 0,
            });
            in_cache = false;
            continue;
        }
        if line.starts_with('[') {
            if let Some(e) = cur.take() {
                c.ents.push(e);
            }
            in_cache = false;
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim().trim_matches('"');
            match k.trim() {
                "fetched_at" if in_cache => c.fetched_at = v.parse().unwrap_or(0),
                "product_id" => {
                    if let Some(e) = cur.as_mut() {
                        e.product_id = v.to_string()
                    }
                }
                "tier" => {
                    if let Some(e) = cur.as_mut() {
                        e.tier = v.to_string()
                    }
                }
                "exp" => {
                    if let Some(e) = cur.as_mut() {
                        e.exp = v.parse().unwrap_or(0)
                    }
                }
                _ => {}
            }
        }
    }
    if let Some(e) = cur.take() {
        if !e.product_id.is_empty() {
            c.ents.push(e);
        }
    }
    Some(c)
}

/// 缓存是否足够新（TTL 内且有成功拉取记录）。
pub fn ents_fresh(root: &Path) -> bool {
    match load_ents(root) {
        Some(c) if c.fetched_at > 0 => {
            now_unix().saturating_sub(c.fetched_at) < ENTS_TTL_DAYS * 86400
        }
        _ => false,
    }
}

/// 门控查询：新鲜缓存里是否有指定商品的有效授权（exp 0=永久，过期视同无）。
/// 陈旧/无缓存 → false（调用方应提示 `mox redeem status` 刷新）。
pub fn has_entitlement(root: &Path, product_id: &str) -> bool {
    if !ents_fresh(root) {
        return false;
    }
    let now = now_unix();
    load_ents(root)
        .map(|c| {
            c.ents
                .iter()
                .any(|e| e.product_id == product_id && (e.exp == 0 || e.exp > now))
        })
        .unwrap_or(false)
}

/// `status`：联网拉取 → 落盘缓存 → 返回授权列表（空 = 未购买/未兑换）。
pub fn refresh_ents(root: &Path) -> ProotResult<Vec<Entitlement>> {
    let dev = ensure_device(root)?;
    let ents = fetch_entitlements(&dev.token)?;
    save_ents(root, &ents)?;
    Ok(ents)
}

// ============================ localhost 回环（绑定设备） ============================

/// 由绑定页 URL 拼出带回环 redirect + state + 设备参数的 URL。
/// `page` 形如 `https://site/bind.html`；拼成
/// `https://site/bind.html?redirect=..&state=..&device_token=..&name=..&platform=..`
/// （page 已带 query 则用 `&` 续）。
pub fn build_bind_url(
    page: &str,
    loopback: &str,
    state: &str,
    token: &str,
    name: &str,
    platform: &str,
) -> String {
    let sep = if page.contains('?') { '&' } else { '?' };
    format!(
        "{}{}redirect={}&state={}&device_token={}&name={}&platform={}",
        page,
        sep,
        url_encode(loopback),
        url_encode(state),
        url_encode(token),
        url_encode(name),
        url_encode(platform)
    )
}

/// 极简 URL 百分号编码（设备名可能含中文/空格，必须编码）。
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
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

/// 从回环收到的 HTTP 请求里解析 `ok` 与 `state`（取请求行的 query）。
/// 请求行形如：`GET /done?ok=1&state=<态> HTTP/1.1`。
pub fn parse_ok_callback(request: &str) -> Option<(String, String)> {
    let line = request.lines().next()?;
    let target = line.split_whitespace().nth(1)?; // 跳过 method
    let (_, query) = target.split_once('?')?;
    let mut okk = None;
    let mut state = None;
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        match k {
            "ok" => okk = Some(v.to_string()),
            "state" => state = Some(v.to_string()),
            _ => {}
        }
    }
    match (okk, state) {
        (Some(o), Some(s)) if o == "1" => Some((o, s)),
        _ => None,
    }
}

/// 回给浏览器的成功页（提示可关闭）。
fn success_html() -> String {
    "<!DOCTYPE html><meta charset=utf-8><title>moxsh</title>\
     <body style=\"font-family:sans-serif;text-align:center;padding:60px\">\
     <h2>✅ 设备已绑定，返回终端查看</h2><p>可以关闭这个页面了。</p>"
        .to_string()
}

/// 在已绑定的 listener 上等待一次回环回调，校验 state，确认绑定成功。
/// `listener` 须已 `bind`（调用方先用它拿 port 拼 redirect）。
pub fn await_bind(listener: &TcpListener, expect_state: &str) -> ProotResult<()> {
    listener.set_nonblocking(true).map_err(ProotError::Io)?;
    let deadline = Instant::now() + LOOPBACK_TIMEOUT;
    let mut stream = loop {
        match listener.accept() {
            Ok((s, _)) => break s,
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(ProotError::InvalidArg(
                        "等待浏览器绑定超时（请重试 mox redeem login）".to_string(),
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

    match parse_ok_callback(&req) {
        Some((_, state)) if state == expect_state => Ok(()),
        Some(_) => Err(ProotError::InvalidArg("回环 state 不匹配，已取消".to_string())),
        None => Err(ProotError::InvalidArg("回环请求里没有有效绑定确认".to_string())),
    }
}

/// [`login`] 的结果：设备信息 + 实际打开的绑定页 URL（浏览器唤起失败时 CLI 需
/// 提示用户手动打开该 URL）。
#[derive(Debug, Clone, PartialEq)]
pub struct BindOutcome {
    pub device: DeviceInfo,
    pub url: String,
    pub opened: Opened,
}

/// 完整绑定流程：确保设备身份 → 起回环 → 开浏览器 → 收确认。返回设备与 URL。
/// 只负责「设备 ↔ 账号」关联；商品授权由买家之后在官网兑换页完成。
pub fn login(root: &Path) -> ProotResult<BindOutcome> {
    let dev = ensure_device(root)?;
    let listener = TcpListener::bind("127.0.0.1:0").map_err(ProotError::Io)?;
    let port = listener.local_addr().map_err(ProotError::Io)?.port();
    let state = make_state();
    let loopback = format!("http://127.0.0.1:{}/done", port);
    let url = build_bind_url(
        &bind_page_url(),
        &loopback,
        &state,
        &dev.token,
        &dev.name,
        &dev.platform,
    );
    let opened = open_browser(&url); // 开浏览器（失败也继续等，用户可手动开 URL）
    await_bind(&listener, &state)?;
    Ok(BindOutcome { device: dev, url, opened })
}

// ============================ 唤起手机浏览器 ============================

#[derive(Debug, Clone, PartialEq)]
pub enum Opened {
    Via(String),
    Failed,
}

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
        let d = std::env::temp_dir().join(format!("moxsh-redeem-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    // ---- 设备身份 ----
    #[test]
    fn generate_token_is_32_lower_hex() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
        assert_ne!(t, generate_token(), "两次生成不应相同");
    }

    #[test]
    fn device_roundtrip_and_ensure() {
        let root = tmp("dev");
        assert!(load_device(&root).is_none());
        let d = ensure_device(&root).unwrap();
        assert_eq!(d.token.len(), 32);
        assert!(!d.name.is_empty());
        assert!(!d.platform.is_empty());
        let d2 = ensure_device(&root).unwrap();
        assert_eq!(d, d2, "二次 ensure 不重新生成");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_device_ignores_garbage_file() {
        let root = tmp("devbad");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(device_file_path(&root), "not a toml at all\n").unwrap();
        assert!(load_device(&root).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- URL 拼装 ----
    #[test]
    fn build_bind_url_encodes_all_params() {
        let u = build_bind_url(
            "https://mox-site.pages.dev/bind.html",
            "http://127.0.0.1:5000/done",
            "st1",
            "abc123",
            "我的 手机",
            "android",
        );
        assert!(u.starts_with("https://mox-site.pages.dev/bind.html?redirect="));
        assert!(u.contains("&state=st1"));
        assert!(u.contains("&device_token=abc123"));
        assert!(u.contains("name=%E6%88%91%E7%9A%84%20%E6%89%8B%E6%9C%BA"));
        assert!(u.contains("platform=android"));
        assert!(u.contains("127.0.0.1%3A5000"));
    }

    #[test]
    fn build_bind_url_amp_when_page_has_query() {
        let u = build_bind_url("https://s/bind.html?x=1", "http://127.0.0.1:1/done", "s", "t", "n", "linux");
        assert!(u.contains("bind.html?x=1&redirect="));
    }

    #[test]
    fn url_encode_spec() {
        assert_eq!(url_encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(url_encode("http://x:1/"), "http%3A%2F%2Fx%3A1%2F");
        assert_eq!(url_encode("中"), "%E4%B8%AD");
    }

    // ---- 回环请求解析 ----
    #[test]
    fn parse_ok_callback_ok() {
        let req = "GET /done?ok=1&state=deadbeef HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        let (o, s) = parse_ok_callback(req).unwrap();
        assert_eq!(o, "1");
        assert_eq!(s, "deadbeef");
    }

    #[test]
    fn parse_ok_callback_rejects_bad() {
        assert!(parse_ok_callback("").is_none());
        assert!(parse_ok_callback("GET /done HTTP/1.1").is_none()); // 无 query
        assert!(parse_ok_callback("GET /done?state=x HTTP/1.1").is_none()); // 无 ok
        assert!(parse_ok_callback("GET /done?ok=0&state=x HTTP/1.1").is_none()); // ok!=1
        assert!(parse_ok_callback("GET /done?ok=1 HTTP/1.1").is_none()); // 无 state
    }

    // ---- 授权缓存 ----
    #[test]
    fn ents_cache_roundtrip_fresh_and_gate() {
        let root = tmp("ents");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!ents_fresh(&root));
        assert!(!has_entitlement(&root, "proot-pro"));
        let ents = vec![
            Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: 0 },
            Entitlement { product_id: "moxsh-plus".into(), tier: "plus".into(), exp: now_unix() + 3600 },
        ];
        save_ents(&root, &ents).unwrap();
        assert!(ents_fresh(&root));
        assert!(has_entitlement(&root, "proot-pro"));
        assert!(has_entitlement(&root, "moxsh-plus"));
        assert!(!has_entitlement(&root, "no-such"));
        let c = load_ents(&root).unwrap();
        assert_eq!(c.ents.len(), 2);
        assert_eq!(c.ents[0].tier, "pro");
        assert!(c.fetched_at > 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn ents_cache_stale_and_expired_gate_off() {
        let root = tmp("entsstale");
        std::fs::create_dir_all(&root).unwrap();
        // 手工写一个 8 天前的缓存（超 TTL）+ 一个昨天过期的授权
        let old = now_unix() - 8 * 86400;
        std::fs::write(
            ents_file_path(&root),
            format!("[cache]\nfetched_at = {}\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = {}\n", old, now_unix() - 86400),
        )
        .unwrap();
        assert!(!ents_fresh(&root), "8 天前的缓存应判陈旧");
        assert!(!has_entitlement(&root, "proot-pro"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_ents_cache_is_fresh_but_no_gate() {
        let root = tmp("entsempty");
        std::fs::create_dir_all(&root).unwrap();
        save_ents(&root, &[]).unwrap();
        assert!(ents_fresh(&root), "刚拉取过的空列表仍是新鲜缓存");
        assert!(!has_entitlement(&root, "proot-pro"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 端点默认值形状（不打网络）----
    #[test]
    fn defaults_shape() {
        assert!(bind_page_url().starts_with("http"));
        assert!(bind_page_url().contains("bind.html"));
    }

    // ---- 回环：真实 localhost 收一次包，端到端验证 await_bind ----
    #[test]
    fn await_bind_end_to_end_on_localhost() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            let req = "GET /done?ok=1&state=st-xyz HTTP/1.1\r\n\r\n";
            s.write_all(req.as_bytes()).unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf); // 读成功页
        });
        await_bind(&listener, "st-xyz").expect("应收到绑定确认");
        handle.join().unwrap();
    }

    #[test]
    fn await_bind_rejects_state_mismatch() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut s = std::net::TcpStream::connect(addr).unwrap();
            s.write_all(b"GET /done?ok=1&state=WRONG HTTP/1.1\r\n\r\n").unwrap();
            let mut buf = [0u8; 256];
            let _ = s.read(&mut buf);
        });
        assert!(await_bind(&listener, "st-xyz").is_err());
        handle.join().unwrap();
    }
}
