//! 通用商品授权客户端（与 fan 灰度系统**并列互不影响**）——「脚本 ↔ 官网」握手，
//! **设备绑定（回环）+ 授权校验（在线）**。
//!
//! 定位：付费商品（proot 引擎增值版等，后台可加）的设备级授权。fan 是**账号级**
//! 粉丝福利（`mox fan`，`$MOX_ROOT/fan.toml`）；本模块是**设备级**商品授权
//! （`mox redeem`，`$MOX_ROOT/device.toml` + `entitlements.toml`），两套文件、
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
//!   视为陈旧（永久授权门控显示 🔒 并提示 `mox redeem status` 刷新；吊销在 TTL 内生效）。
//!   **有限期订阅**（`exp>0`，如 `proot-pro` 月度）例外：即便缓存陈旧也按 `exp` 判定，
//!   离线 30 天仍能用到到期当天，过后立即失效（服务端已承诺该到期时间）。
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

use crate::proot::backend;
use crate::proot::{ProotError, ProotResult};

/// 授权缓存签名验证公钥（Ed25519，32 字节）。
///
/// 对应服务端私钥仅在 mox-id 后端持有（环境变量 `ED25519_ENTITLEMENT_SECRET`），
/// 客户端只嵌公钥、只验签——私钥永不落客户端，攻击者无法伪造授权缓存
/// （渗透测试报告·路径①彻底关闭）。公钥公开，无保密需求。
const ENTITLEMENT_VERIFY_KEY: [u8; 32] = [
    0x6c, 0x82, 0xf5, 0xd1, 0x75, 0x3c, 0x2c, 0x7e, 0x12, 0x06, 0x89, 0x60, 0x67, 0x9e, 0xcc, 0xe9,
    0x50, 0xbf, 0x60, 0xd0, 0x4d, 0x71, 0xf9, 0xb8, 0x9a, 0x50, 0x38, 0xd9, 0x06, 0xfe, 0x72, 0x29,
];

/// 极简 base64 标准字母解码（内联，避免新增依赖；仅签名校验用）。
fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        };
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

/// 重建服务端签名所用的规范消息：每条授权一行 `ok <pid> <tier> <exp>\n`。
/// 必须与 mox-id `/entitlement/check` 的签名输入逐字节一致。
fn canonical_entitlement_blob(ents: &[Entitlement]) -> String {
    let mut s = String::new();
    for e in ents {
        s.push_str(&format!("ok {} {} {}\n", e.product_id, e.tier, e.exp));
    }
    s
}

/// 校验授权缓存签名（服务端 Ed25519）。`sig_b64` 为服务端追加的 `sig <b64>` 行。
/// 仅当签名存在且有效时返回 true；客户端无私钥，无法伪造。
pub fn verify_entitlement_signature(ents: &[Entitlement], sig_b64: &str) -> bool {
    use ed25519_dalek::Verifier;
    let raw = match b64_decode(sig_b64) {
        Some(r) if r.len() == 64 => r,
        _ => return false,
    };
    let sig_arr: [u8; 64] = match raw.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let vk = match ed25519_dalek::VerifyingKey::from_bytes(&ENTITLEMENT_VERIFY_KEY) {
        Ok(k) => k,
        Err(_) => return false,
    };
    let sig = ed25519_dalek::Signature::from_bytes(&sig_arr);
    vk.verify(canonical_entitlement_blob(ents).as_bytes(), &sig).is_ok()
}

/// 设备身份文件相对 `$MOX_ROOT` 的位置（删容器不删它；删了=换设备，需重新绑定）。
pub const DEVICE_FILE_REL: &str = "device.toml";
/// 授权缓存文件（`mox redeem status` 拉取后落盘；门禁只读缓存，不联网）。
pub const ENTS_FILE_REL: &str = "entitlements.toml";
/// 绑定页 URL（`MOX_BIND_PAGE` 覆盖）。
pub const BIND_PAGE_ENV: &str = "MOX_BIND_PAGE";
/// 授权校验端点（`MOX_ENTITLEMENT_API` 覆盖）。
pub const ENTITLEMENT_API_ENV: &str = "MOX_ENTITLEMENT_API";
/// 设备显示名（`MOX_DEVICE_NAME` 覆盖；默认取 hostname）。
pub const DEVICE_NAME_ENV: &str = "MOX_DEVICE_NAME";

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

/// 授权缓存（`entitlements.toml`）：列表 + 拉取时间 + 绑定设备 + 服务端签名。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EntCache {
    pub ents: Vec<Entitlement>,
    /// 拉取时间（unix 秒）；0 = 从未成功拉取。
    pub fetched_at: u64,
    /// 写入缓存时的本机设备令牌：**防跨设备拷贝**——把别人机器的
    /// `entitlements.toml` 拷过来会因 `device_token` 不匹配而失效
    /// （渗透测试报告·次要问题#1）。无设备身份时为空。
    pub device_token: String,
    /// 服务端 Ed25519 签名（base64）。存在且有效时门禁完全信任缓存；
    /// 缺失则回退旧行为（设备绑定 + TTL）；无效则视为篡改直接拒绝。
    pub sig: Option<String>,
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
        "# mox-proot 设备身份（由 `mox redeem` 写入，请勿手改）。删除本文件 = 放弃本设备，需重新绑定。\n\
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

/// 探测设备显示名（`MOX_DEVICE_NAME` > hostname > 通用名）。
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
    "mox-proot 设备".into()
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

/// 绑定页 URL（`MOX_BIND_PAGE` 覆盖）。
/// 默认指向官网 pages.dev 的隐藏页 bind.html——导航里没有入口，
/// 只能由本命令带 redirect/state/device_token 唤起（与 fan 页同思路）。
pub fn bind_page_url() -> String {
    std::env::var(BIND_PAGE_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| backend::site_page("/bind.html"))
        .trim_end_matches('/')
        .to_string()
}

/// 授权校验端点（`MOX_ENTITLEMENT_API` 覆盖）。
/// 默认走**官网同域反代**（pages.dev/entitlement/check）——Worker 标准域
/// workers.dev 国内不可达（爱发电 webhook 443 同款坑），pages.dev 实测可达。
/// 域名由 [`backend`] 单一来源派生，避免与设备码 / 粉丝客户端各自漂移。
///
/// 安全：环境覆盖值仅接受 `https://` 或**本机** `http://`（测试 / 本地自托管，
/// 无网络 MITM）；拒绝 `http://` 公网主机，防中间人把授权查询劫持到攻击者
/// 服务器投毒（渗透测试报告·MOXSH_REDEEM_API 劫持）。不合法覆盖值回退官方反代。
fn entitlement_api() -> String {
    safe_override_url(
        &std::env::var(ENTITLEMENT_API_ENV).unwrap_or_default(),
        backend::endpoint("/entitlement/check"),
    )
}

/// 授权/兑换 API 覆盖值校验：https 任意放行；`http://127.0.0.1`/`localhost`
/// 视为本机放行；其余 http 视为可能被中间人篡改，回退 `default`。
fn safe_override_url(env_val: &str, default: String) -> String {
    if env_val.is_empty() {
        return default;
    }
    if env_val.starts_with("https://") {
        return env_val.to_string();
    }
    if let Some(rest) = env_val.strip_prefix("http://") {
        let host = rest.split('/').next().unwrap_or("");
        let is_local =
            host == "localhost" || host == "127.0.0.1" || host.starts_with("127.0.0.1:");
        if is_local {
            return env_val.to_string();
        }
    }
    default
}

/// 联网拉取本机全部授权：`GET <api>?device_token=<hex>` → 逐行解析 `ok <pid> <tier> <exp>`，
/// 末尾可选 `sig <base64>` 为服务端 Ed25519 签名。
/// 404（无授权）是正常空态，返回空列表而非错误。返回 `(授权列表, 签名)`。
pub fn fetch_entitlements(token: &str) -> ProotResult<(Vec<Entitlement>, Option<String>)> {
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
        "404" => return Ok((Vec::new(), None)), // 无授权：正常空态
        other => {
            return Err(ProotError::InvalidArg(format!(
                "授权查询未通过（HTTP {}）：{}",
                other,
                body.trim()
            )));
        }
    }
    let mut ents = Vec::new();
    let mut sig: Option<String> = None;
    for line in body.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("ok") => {
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
            Some("sig") => {
                // 服务端 Ed25519 签名（base64）；后续落盘缓存并在门控时验签。
                let s = it.next().unwrap_or("").to_string();
                if !s.is_empty() {
                    sig = Some(s);
                }
            }
            _ => {} // 容忍空行/杂讯行
        }
    }
    Ok((ents, sig))
}

// ============================ 授权缓存（entitlements.toml） ============================

/// 授权缓存文件绝对路径。
pub fn ents_file_path(root: &Path) -> PathBuf {
    root.join(ENTS_FILE_REL)
}

/// 写授权缓存（原子替换）。`sig` 为服务端 Ed25519 签名（base64），无则省略。
pub fn save_ents(root: &Path, ents: &[Entitlement], sig: Option<&str>) -> ProotResult<()> {
    std::fs::create_dir_all(root).map_err(ProotError::Io)?;
    let dev_tok = load_device(root).map(|d| d.token).unwrap_or_default();
    let mut text = format!(
        "# mox-proot 商品授权缓存（由 `mox redeem status` 拉取后写入，请勿手改）。\n\
         [cache]\n\
         fetched_at = {}\n\
         device_token = \"{}\"\n",
        now_unix(),
        dev_tok
    );
    if let Some(s) = sig {
        text.push_str(&format!("sig = \"{}\"\n", s));
    }
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
    // 授权缓存含设备令牌与商品授权，仅拥有者可读写（0o600），降低本地泄露。
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
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
                "device_token" if in_cache => c.device_token = v.to_string(),
                "sig" if in_cache => c.sig = Some(v.to_string()),
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

/// 门控查询：是否有指定商品的有效授权。
///
/// 两套判定逻辑（对应两类商品）：
/// - **有限期订阅**（如 `proot-pro` 月度，`exp>0`）：即便授权缓存已陈旧也**尊重到期日**，
///   即离线 30 天仍能用到到期当天，过后立即失效（用户 2026-10-31 要求）。服务端已承诺
///   该到期时间，客户端无需联网即可判定。
/// - **永久授权**（`exp==0`）：依赖 7 天缓存 TTL 维持可吊销性——缓存陈旧则判无，
///   提示用户执行 `mox redeem status` 刷新（与旧行为一致）。
pub fn has_entitlement(root: &Path, product_id: &str) -> bool {
    let now = now_unix();
    let cache = match load_ents(root) {
        Some(c) => c,
        None => return false,
    };
    // 【渗透测试复测 P0-1】字段缺失一律按"拒绝"处理。
    //
    // 旧实现的两个致命漏洞（真机实测可 5 行 toml 解锁全部付费配方）：
    //   · 验签写在 `if let Some(sig)` 里 → 不写 `sig` 行即整段跳过；
    //   · 设备绑定在 `device_token` 为空 / 本机无 device.toml 时跳过。
    // 攻击者只要伪造 entitlements.toml 时同时省略这两个可选字段，就回落到
    // "无签名降级"分支，`exp > now` 直接放行。故此处改为**强制三连校验**：
    //   1) 本机设备身份必须存在（合法缓存必经 refresh_ents → ensure_device）；
    //   2) 缓存 device_token 必须存在且等于本机 token；
    //   3) 缓存 sig 必须存在且 Ed25519 验签通过（无私钥无法伪造）。
    // 任一不满足立即拒绝，绝不回落。代价：升级后旧缓存需联网刷新一次
    //（`mox redeem status`），换取付费门禁不可伪造。
    let dev = match load_device(root) {
        Some(d) => d,
        None => return false,
    };
    if cache.device_token.is_empty() || cache.device_token != dev.token {
        return false;
    }
    let sig = match cache.sig.as_deref() {
        Some(s) => s,
        None => return false,
    };
    if !verify_entitlement_signature(&cache.ents, sig) {
        return false;
    }
    // 签名有效：完全信任服务端声明——离线也按到期日生效，无需 TTL。
    for e in &cache.ents {
        if e.product_id == product_id && (e.exp == 0 || e.exp > now) {
            return true;
        }
    }
    false
}

/// `status`：联网拉取 → 落盘缓存 → 返回授权列表（空 = 未购买/未兑换）。
pub fn refresh_ents(root: &Path) -> ProotResult<Vec<Entitlement>> {
    let dev = ensure_device(root)?;
    let (ents, sig) = fetch_entitlements(&dev.token)?;
    save_ents(root, &ents, sig.as_deref())?;
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
    "<!DOCTYPE html><meta charset=utf-8><title>mox-proot</title>\
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

/// 测试夹具（仅 cfg(test) 编译）：供 cli/recipe 等跨模块门禁测试构造
/// 「服务端已签名 + 绑定本机设备」的合法授权缓存。P0-1 之后门禁为
/// fail-closed（缺 sig 或缺 device.toml 一律拒绝），各测试不能再自行
/// 用 `save_ents(root, ents, None)` 伪造缓存，必须经此夹具落盘。
///
/// 签名样本是公钥验签数据（对应内嵌 ENTITLEMENT_VERIFY_KEY），非机密；
/// 私钥仅在 mox-id 后端，客户端代码里绝不出现。
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;

    /// `ok proot-pro pro 0\n` 的 Ed25519 签名（base64）。
    pub const SIG_PROOT_PRO: &str = "OE3p5GJx2hi82OjOlRxN2bC2shpOPjYlohl8SZXCSiR2soCYa8AZbRAXMoKSMOpyn/1+M13a/D3LihZaF8QCCg==";
    /// `ok proot-pro pro 0\nok mox-proot-plus plus 4102444800\n` 的签名。
    pub const SIG_TWO: &str = "hl5hEmw7LkvzK8q3xTkFnqL9krYwvmKQYj4S7EQf58CqeCZGm4HjI5sFfqGMLIYA0DYvhi6ABJzpdIwKqKpXAw==";

    /// 落一份本机合法缓存：先写 device.toml（设备身份），再写带服务端签名的
    /// entitlements.toml，使 `has_entitlement` 的三连校验（设备 + token + 签名）全过。
    pub fn seed_legit_cache(root: &Path, ents: &[Entitlement], sig: &str) {
        save_device(
            root,
            &DeviceInfo { token: "aaa".into(), name: "n".into(), platform: "linux".into() },
        )
        .unwrap();
        save_ents(root, ents, Some(sig)).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mox-proot-redeem-test-{}-{}", tag, std::process::id()));
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
    // 预生成的有效签名（对应真实服务端私钥派生公钥；私钥只在 mox-id 后端，
    // 客户端代码里绝不出现）。用于离线构造「服务端亲笔签发」的缓存做测试。
    const SIG_SINGLE: &str = "OE3p5GJx2hi82OjOlRxN2bC2shpOPjYlohl8SZXCSiR2soCYa8AZbRAXMoKSMOpyn/1+M13a/D3LihZaF8QCCg=="; // ok proot-pro pro 0
    const SIG_TWO: &str = "hl5hEmw7LkvzK8q3xTkFnqL9krYwvmKQYj4S7EQf58CqeCZGm4HjI5sFfqGMLIYA0DYvhi6ABJzpdIwKqKpXAw=="; // 上面 + ok mox-proot-plus plus 4102444800
    const SIG_EMPTY: &str = "efGAaXXpFmzW5T8OMWKGwTsh1saDaHRkO7zL27QBlhWuu6diaae/+Pkf/sdiXrGYeIXVMCMV9WDR2QKEmn2oCQ=="; // 空清单
    const SIG_PAST: &str = "FcKVmi5M263tUk9Lng9iSVCgag0xGQ62LddaLIwrif24PZeV4gm7NKzzjTyMeG5R3XCrlO60gl5WYCaQ29AnCA=="; // ok proot-pro pro 1000000000
    const FUTURE_EXP: u64 = 4102444800; // 2100-01-01
    const PAST_EXP: u64 = 1000000000;  // 2001-09-09

    /// 构造一份「本机合法」缓存：先落设备身份，再写带服务端签名的缓存。
    fn seed_legit(root: &Path, ents: &[Entitlement], sig: &str) {
        save_device(
            root,
            &DeviceInfo { token: "aaa".into(), name: "n".into(), platform: "linux".into() },
        )
        .unwrap();
        save_ents(root, ents, Some(sig)).unwrap();
    }

    #[test]
    fn ents_cache_roundtrip_fresh_and_gate() {
        let root = tmp("ents");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!ents_fresh(&root));
        assert!(!has_entitlement(&root, "proot-pro"));
        let ents = vec![
            Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: 0 },
            Entitlement { product_id: "mox-proot-plus".into(), tier: "plus".into(), exp: FUTURE_EXP },
        ];
        seed_legit(&root, &ents, SIG_TWO);
        assert!(ents_fresh(&root));
        assert!(has_entitlement(&root, "proot-pro"));
        assert!(has_entitlement(&root, "mox-proot-plus"));
        assert!(!has_entitlement(&root, "no-such"));
        let c = load_ents(&root).unwrap();
        assert_eq!(c.ents.len(), 2);
        assert_eq!(c.ents[0].tier, "pro");
        assert!(c.fetched_at > 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn signed_but_expired_entitlement_rejected() {
        // 签名有效（服务端确实签过这个 exp）但已过期 → 按声明的到期日拒绝。
        let root = tmp("entspast");
        std::fs::create_dir_all(&root).unwrap();
        seed_legit(
            &root,
            &[Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: PAST_EXP }],
            SIG_PAST,
        );
        assert!(!has_entitlement(&root, "proot-pro"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn entitlement_cache_bound_to_device_blocks_foreign_copy() {
        // 防跨设备拷贝：本机设备身份与缓存 device_token 不一致时，整份授权失效。
        let root = tmp("entsdev");
        std::fs::create_dir_all(&root).unwrap();
        seed_legit(&root, &[Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: 0 }], SIG_SINGLE);
        assert!(has_entitlement(&root, "proot-pro"), "本机缓存应放行");

        // 攻击者把别人机器的 entitlements.toml 拷过来（device_token=bbb）
        let p = ents_file_path(&root);
        let mut s = std::fs::read_to_string(&p).unwrap();
        s = s.replace("device_token = \"aaa\"", "device_token = \"bbb\"");
        std::fs::write(&p, s).unwrap();
        assert!(!has_entitlement(&root, "proot-pro"), "他人授权文件应失效");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 【P0-1 回归】渗透测试真机复现的攻击：伪造 toml 同时省略 `sig` 与
    /// `device_token` 两个可选字段，旧实现会跳过验签 + 跳过设备绑定，回落到
    /// 无签名分支把 `exp > now` 直接放行。现在必须一律拒绝。
    #[test]
    fn forged_cache_omitting_sig_and_device_token_is_rejected() {
        let root = tmp("entsforge");
        std::fs::create_dir_all(&root).unwrap();
        // 攻击载荷：5 行，未来 exp，无 sig 行、无 device_token 行
        std::fs::write(
            ents_file_path(&root),
            format!(
                "[cache]\nfetched_at = {}\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = {}\n",
                now_unix(), FUTURE_EXP
            ),
        )
        .unwrap();
        assert!(!has_entitlement(&root, "proot-pro"), "省略 sig/device_token 的伪造必须拒绝");

        // 变体 A：本机有 device.toml，但缓存仍省略两个字段
        seed_legit(&root, &[], SIG_EMPTY); // 先建合法设备身份（空授权）
        std::fs::write(
            ents_file_path(&root),
            format!(
                "[cache]\nfetched_at = {}\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = {}\n",
                now_unix(), FUTURE_EXP
            ),
        )
        .unwrap();
        assert!(!has_entitlement(&root, "proot-pro"), "有设备身份但缓存无 sig 也必须拒绝");

        // 变体 B：有 device_token、无 sig
        std::fs::write(
            ents_file_path(&root),
            format!(
                "[cache]\nfetched_at = {}\ndevice_token = \"aaa\"\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = {}\n",
                now_unix(), FUTURE_EXP
            ),
        )
        .unwrap();
        assert!(!has_entitlement(&root, "proot-pro"), "缺 sig 必须拒绝");

        // 变体 C：有 sig、无 device_token
        std::fs::write(
            ents_file_path(&root),
            format!(
                "[cache]\nfetched_at = {}\nsig = \"{}\"\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = 0\n",
                now_unix(), SIG_SINGLE
            ),
        )
        .unwrap();
        assert!(!has_entitlement(&root, "proot-pro"), "缺 device_token 必须拒绝");

        // 变体 D：无 device.toml（攻击者删掉设备身份文件）
        let root2 = tmp("entsnodev");
        std::fs::create_dir_all(&root2).unwrap();
        std::fs::write(
            ents_file_path(&root2),
            format!(
                "[cache]\nfetched_at = {}\nsig = \"{}\"\ndevice_token = \"aaa\"\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = 0\n",
                now_unix(), SIG_SINGLE
            ),
        )
        .unwrap();
        assert!(!has_entitlement(&root2, "proot-pro"), "无设备身份文件必须拒绝");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn signed_cache_trusted_offline_even_when_stale() {
        // 签名有效 → 完全信任服务端声明：缓存陈旧（超 TTL）也离线放行。
        let root = tmp("entssig");
        std::fs::create_dir_all(&root).unwrap();
        seed_legit(&root, &[Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: 0 }], SIG_SINGLE);
        let p = ents_file_path(&root);
        let mut s = std::fs::read_to_string(&p).unwrap();
        let old = load_ents(&root).unwrap().fetched_at;
        s = s.replace(
            &format!("fetched_at = {}", old),
            &format!("fetched_at = {}", now_unix() - 30 * 86400),
        );
        std::fs::write(&p, s).unwrap();
        assert!(!ents_fresh(&root), "缓存确实陈旧");
        assert!(has_entitlement(&root, "proot-pro"), "带有效签名的陈旧缓存应离线放行");
        assert!(!has_entitlement(&root, "no-such"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn tampered_signature_rejected_even_when_fresh() {
        // 篡改检测：签名存在但无效（伪造/损坏）→ 直接拒绝，即使缓存新鲜。
        let root = tmp("entstamper");
        std::fs::create_dir_all(&root).unwrap();
        seed_legit(&root, &[], SIG_EMPTY);
        std::fs::write(
            ents_file_path(&root),
            format!(
                "[cache]\nfetched_at = {}\ndevice_token = \"aaa\"\nsig = \"AAAA-invalid-signature\"\n[[ent]]\nproduct_id = \"proot-pro\"\ntier = \"pro\"\nexp = 0\n",
                now_unix()
            ),
        )
        .unwrap();
        assert!(ents_fresh(&root), "缓存新鲜");
        assert!(!has_entitlement(&root, "proot-pro"), "签名无效必须拒绝");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_entitlement_signature_rejects_garbage() {
        let ents = vec![Entitlement {
            product_id: "proot-pro".into(),
            tier: "pro".into(),
            exp: 0,
        }];
        assert!(!verify_entitlement_signature(&ents, "not-valid-base64!!!"));
        assert!(!verify_entitlement_signature(&ents, ""));
        assert!(verify_entitlement_signature(&ents, SIG_SINGLE));
        // 签名是对整个清单的：单条签名不能用于双条清单
        let two = vec![
            Entitlement { product_id: "proot-pro".into(), tier: "pro".into(), exp: 0 },
            Entitlement { product_id: "mox-proot-plus".into(), tier: "plus".into(), exp: FUTURE_EXP },
        ];
        assert!(!verify_entitlement_signature(&two, SIG_SINGLE));
        assert!(verify_entitlement_signature(&two, SIG_TWO));
    }

    #[test]
    fn empty_ents_cache_is_fresh_but_no_gate() {
        let root = tmp("entsempty");
        std::fs::create_dir_all(&root).unwrap();
        seed_legit(&root, &[], SIG_EMPTY);
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
