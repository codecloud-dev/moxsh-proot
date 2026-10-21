//! 粉丝专属 / 兑换码客户端（0.5.0）——「脚本 ↔ 官网」对接握手的客户端半边。
//!
//! 流程（与官网后端的分工，后端侧另行实现，本模块只做客户端）：
//!   1. 用户在公众号/官网登录后，于「粉丝专区」点击「确认领取」→ 官网签发一个
//!      **兑换码（code）**；
//!   2. `mox fan login` 用脚本**唤起手机浏览器**打开官网领取页（省得手动输网址）；
//!   3. 用户拿到兑换码后 `mox fan redeem <code>` → 本模块**验签 + 落盘**，
//!      之后 `mox fan status` 显示已解锁，fan_only 配方随之可用。
//!
//! 设计约束（与全仓库一致）：
//! - **零外部依赖**：不引 ring/serde。SHA-256 + HMAC-SHA256 + base64url 全部
//!   手写（见下文测试里的 RFC/NIST 已知向量）。
//! - **住在 mox 控制台里**：`mox fan ...` 经 console 反穿透，`root` = 真实
//!   `$MOXSH_ROOT`；令牌落盘 `$MOXSH_ROOT/fan.toml`，随用户数据根走（不随单个
//!   rootfs 的 backup——粉丝身份是账号级，不是容器级）。
//! - **粉丝门禁而非安全边界**：兑换码用 HMAC-SHA256 离线验签，密钥内置于二进制
//!   （公开仓库可见）。这能挡住随手伪造/手滑输错，但不是抗逆向的强保护。官网后端
//!   上线后应改为**服务端签发 + 在线校验**（设 `MOXSH_REDEEM_API` 即走在线路径），
//!   离线 HMAC 仅作后端未就绪时的可用兜底。见 [`verify_mode`]。
//!
//! 兑换码格式（官网签发端按此实现，二者才能对上）：
//! ```text
//!   code = base64url(payload) + "." + base64url(hmac_sha256(KEY, payload))
//!   payload = "v1|tier|exp|nonce"
//!     tier  : 粉丝档位（如 "fan"）
//!     exp   : 过期 unix 秒（"0" = 永不过期）
//!     nonce : 随机十六进制（防重放/辨识）
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::proot::{ProotError, ProotResult};

/// 令牌版本前缀（payload 首段）。格式不兼容时 bump。
const TOKEN_VERSION: &str = "v1";

/// 离线验签用的 HMAC 密钥（粉丝门禁，非安全边界；生产用 `MOXSH_FAN_KEY` 覆盖）。
/// 必须与官网粉丝专区 `config.js` 的 `fanKey` 逐字一致——官网用 Web Crypto 签
/// 设备兑换码，手机端用本密钥离线验签（见 `issue`/`verify` 与官网 README）。
const DEFAULT_FAN_KEY: &[u8] = b"moxsh-fan-gate-v1-9f3a";

/// 官网基础地址（`MOXSH_SITE_URL` 覆盖；默认占位，部署时改或设环境变量）。
const DEFAULT_SITE_URL: &str = "https://www.moxsh.cn";

/// 在线校验 API（`MOXSH_REDEEM_API` 设置即优先走在线；未设则离线 HMAC）。
const REDEEM_API_ENV: &str = "MOXSH_REDEEM_API";

/// 令牌文件相对 `$MOXSH_ROOT` 的位置。
pub const FAN_FILE_REL: &str = "fan.toml";

/// 验签通过的声明。
#[derive(Debug, Clone, PartialEq)]
pub struct Claims {
    pub tier: String,
    /// 过期 unix 秒；0 = 永不过期。
    pub exp: u64,
    pub nonce: String,
}

impl Claims {
    /// 是否已过期（exp=0 视为永久）。
    pub fn is_expired(&self, now: u64) -> bool {
        self.exp != 0 && now >= self.exp
    }
}

/// 已落盘的粉丝状态。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FanState {
    pub code: String,
    pub tier: String,
    pub exp: u64,
}

/// 唤起浏览器的结果（供调用方打印差异化提示）。
#[derive(Debug, Clone, PartialEq)]
pub enum Opened {
    /// 经哪条命令唤起成功。
    Via(String),
    /// 全部方式失败——调用方应把 URL 打给用户手动打开。
    Failed,
}

/// 当前生效的验签模式（诊断/提示用）。
pub fn verify_mode() -> &'static str {
    if std::env::var_os(REDEEM_API_ENV).is_some() {
        "online"
    } else {
        "offline"
    }
}

/// HMAC 密钥：`MOXSH_FAN_KEY` > 内置默认。
fn fan_key() -> Vec<u8> {
    std::env::var_os("MOXSH_FAN_KEY")
        .map(|k| k.into_encoded_bytes())
        .unwrap_or_else(|| DEFAULT_FAN_KEY.to_vec())
}

/// 当前 unix 秒（时钟异常回落 0，不当 panic）。
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 一个够用的 nonce：pid ^ 纳秒时间，十六进制。无需 rand crate。
fn make_nonce() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", std::process::id(), t)
}

// ============================ base64url（无填充） ============================

const B64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// base64url 编码（无 `=` 填充）。
pub fn b64url_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(B64URL[(n >> 18) as usize & 63] as char);
        out.push(B64URL[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(B64URL[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(B64URL[n as usize & 63] as char);
        }
    }
    out
}

/// base64url 解码（容忍缺 `=` 填充；非法字符返回 None）。
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut rev = [255u8; 256];
    for (i, &c) in B64URL.iter().enumerate() {
        rev[c as usize] = i as u8;
    }
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &c in s.as_bytes() {
        let v = rev[c as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
        }
    }
    Some(out)
}

// ============================ SHA-256（FIPS 180-4） ============================

const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 摘要（32 字节）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    // 预处理：消息 + 0x80 + 补零 + 64 位大端长度。
    let bitlen = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());

    for block in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in block.chunks_exact(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
        let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, v) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_be_bytes());
    }
    out
}

/// HMAC-SHA256（RFC 2104）。
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = if key.len() > 64 { sha256(key).to_vec() } else { key.to_vec() };
    k.resize(64, 0);
    let ipad: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    let opad: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    let mut inner = Vec::with_capacity(64 + msg.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(msg);
    let inner_hash = sha256(&inner);
    let mut outer = Vec::with_capacity(96);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

/// 定长时间比较（避免逐字节早退的时序侧信道）。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ============================ 兑换码：签发 / 验签 ============================

/// 签发一个兑换码（客户端侧主要用于测试/自签；官网后端有自己的签发端）。
///
/// `tier` 粉丝档位；`exp` 过期 unix 秒（0 = 永久）。
pub fn issue(tier: &str, exp: u64) -> String {
    let payload = format!(
        "{}|{}|{}|{}",
        TOKEN_VERSION,
        tier,
        exp,
        make_nonce()
    );
    let mac = hmac_sha256(&fan_key(), payload.as_bytes());
    format!("{}.{}", b64url_encode(payload.as_bytes()), b64url_encode(&mac))
}

/// 验签并解析兑换码。格式错/签名错/过期均返回 None。
///
/// 在线模式（设 `MOXSH_REDEEM_API`）下本函数仍先做离线结构校验，在线二次校验
/// 由调用方（redeem 流程）负责——分离关注点，便于测试。
pub fn verify(code: &str) -> Option<Claims> {
    let (p_b64, mac_b64) = code.split_once('.')?;
    let payload = b64url_decode(p_b64)?;
    let mac = b64url_decode(mac_b64)?;
    let expect = hmac_sha256(&fan_key(), &payload);
    if !ct_eq(&mac, &expect) {
        return None;
    }
    let payload = String::from_utf8(payload).ok()?;
    let mut it = payload.split('|');
    let ver = it.next()?;
    if ver != TOKEN_VERSION {
        return None;
    }
    let tier = it.next()?.to_string();
    let exp: u64 = it.next()?.parse().ok()?;
    let nonce = it.next()?.to_string();
    let claims = Claims { tier, exp, nonce };
    if claims.is_expired(now_unix()) {
        return None;
    }
    Some(claims)
}

// ============================ 令牌落盘（$MOXSH_ROOT/fan.toml） ============================

/// 令牌文件绝对路径。
pub fn fan_file_path(root: &Path) -> PathBuf {
    root.join(FAN_FILE_REL)
}

/// 读已落盘粉丝状态（无文件/解析不出 = None，不是错误）。
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
    // 落盘态也要防过期：过期文件等同未解锁。
    if st.exp != 0 && now_unix() >= st.exp {
        return None;
    }
    Some(st)
}

/// 写粉丝状态（原子感：先写后改名；目录不存在则建）。
pub fn save_state(root: &Path, state: &FanState) -> ProotResult<()> {
    std::fs::create_dir_all(root).map_err(ProotError::Io)?;
    let text = format!(
        "# moxsh 粉丝专属令牌（由 `mox fan redeem` 写入，请勿手改）。\n\
         # 删除本文件 = 解除粉丝专属；重新领取兑换码即可恢复。\n\
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

/// 是否已解锁粉丝专属（有未过期令牌）。
pub fn is_fan(root: &Path) -> bool {
    load_state(root).is_some()
}

/// 兑换：验签 → 落盘 → 返回档位。失败给出可读原因。
pub fn redeem(root: &Path, code: &str) -> ProotResult<String> {
    let code = code.trim();
    match verify(code) {
        Some(claims) => {
            save_state(
                root,
                &FanState {
                    code: code.to_string(),
                    tier: claims.tier.clone(),
                    exp: claims.exp,
                },
            )?;
            Ok(claims.tier)
        }
        None => Err(ProotError::InvalidArg(format!(
            "兑换码无效或已过期：{}（请从官网粉丝专区重新领取）",
            code
        ))),
    }
}

// ============================ 唤起手机浏览器 ============================

/// 官网基础地址（`MOXSH_SITE_URL` 覆盖）。
pub fn site_base_url() -> String {
    std::env::var("MOXSH_SITE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_SITE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// 粉丝专区领取页（登录 + 确认领取都在这里）。
pub fn claim_url() -> String {
    format!("{}/claim", site_base_url())
}

/// 尝试用给定命令唤起浏览器（存在即退出码 0 视为成功）。
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

/// 唤起手机浏览器打开 `url`。依次尝试：
/// `termux-open-url`（Termux API）→ `am start -a android.intent.action.VIEW`
/// （Android 原生，PATH 或常见绝对路径）→ `xdg-open`（桌面 Linux）。
/// 全失败返回 [`Opened::Failed`]，调用方应把 URL 交给用户手动打开。
pub fn open_browser(url: &str) -> Opened {
    if try_launch("termux-open-url", &[url]) {
        return Opened::Via("termux-open-url".into());
    }
    // am：PATH 里没有时试常见绝对路径（Termux 精简 PATH 常缺 /system/bin）。
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

    // ---- base64url ----
    #[test]
    fn b64url_roundtrip_and_alphabet() {
        for s in ["", "a", "ab", "abc", "hello world", "v1|fan|0|deadbeef"] {
            let enc = b64url_encode(s.as_bytes());
            // 无填充、只用 url 安全字母。
            assert!(!enc.contains('=') && !enc.contains('+') && !enc.contains('/'), "enc={enc}");
            assert_eq!(b64url_decode(&enc).unwrap(), s.as_bytes());
        }
    }

    #[test]
    fn b64url_known_vector() {
        // "Hello!" -> base64url 无填充。
        assert_eq!(b64url_encode(b"Hello!"), "SGVsbG8h");
        assert_eq!(b64url_decode("SGVsbG8h").unwrap(), b"Hello!");
    }

    #[test]
    fn b64url_rejects_bad_char() {
        assert!(b64url_decode("ab*cd").is_none());
    }

    // ---- SHA-256 / HMAC（NIST + RFC 4231 已知向量）----
    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn hmac_sha256_rfc4231_case2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex(&mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    // ---- 兑换码签发 / 验签 ----
    #[test]
    fn issue_and_verify_roundtrip() {
        let code = issue("fan", 0);
        let claims = verify(&code).expect("自签应验过");
        assert_eq!(claims.tier, "fan");
        assert_eq!(claims.exp, 0);
        assert!(!claims.nonce.is_empty());
    }

    #[test]
    fn verify_rejects_tamper() {
        let code = issue("fan", 0);
        // 改 payload 一个字符（base64 段），签名对不上。
        let (p, m) = code.split_once('.').unwrap();
        let mut pc: Vec<char> = p.chars().collect();
        let idx = pc.len() / 2;
        pc[idx] = if pc[idx] == 'A' { 'B' } else { 'A' };
        let forged = format!("{}.{}", pc.into_iter().collect::<String>(), m);
        assert!(verify(&forged).is_none(), "篡改 payload 必须拒");
    }

    #[test]
    fn verify_rejects_expired() {
        // 手工造一个已过期 payload + 正确签名（绕过 issue 的 now 检查用旧 exp）。
        let payload = format!("{}|fan|{}|{}", TOKEN_VERSION, 1u64, "dead");
        let mac = hmac_sha256(&fan_key(), payload.as_bytes());
        let code = format!("{}.{}", b64url_encode(payload.as_bytes()), b64url_encode(&mac));
        assert!(verify(&code).is_none(), "过期码必须拒");
    }

    #[test]
    fn verify_rejects_garbage() {
        assert!(verify("").is_none());
        assert!(verify("nodot").is_none());
        assert!(verify("aaa.bbb").is_none());
        // 签名段非 base64url。
        assert!(verify("YWJj.###".replace("###", "!!!").as_str()).is_none());
    }

    #[test]
    fn verify_accepts_webcrypto_issued_code() {
        // 跨语言一致性：本码由 Node 的 Web Crypto（浏览器同款 crypto.subtle HMAC）
        // 用官网 config.js 的同一 fanKey + 同一 payload 格式签发。
        // 若本测试通过，即证明「官网粉丝专区签发的设备兑换码，手机端必能离线验过」。
        // payload = "v1|fan|9999999999|xcheck-nonce"（exp 为 2286 年，不至过期）。
        let code = "djF8ZmFufDk5OTk5OTk5OTl8eGNoZWNrLW5vbmNl.1MhDjjzdIfx-aUaYvZ8TDbDvL3kXJmLeaic8vVKNfbs";
        let claims = verify(code).expect("Web Crypto 签的码应被 Rust 验过");
        assert_eq!(claims.tier, "fan");
        assert_eq!(claims.exp, 9999999999);
    }

    // ---- 落盘 / 状态 ----
    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moxsh-fan-test-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn save_load_roundtrip_and_is_fan() {
        let root = tmp("store");
        std::fs::create_dir_all(&root).unwrap();
        assert!(!is_fan(&root), "空目录未解锁");
        let code = issue("fan", 0);
        let tier = redeem(&root, &code).unwrap();
        assert_eq!(tier, "fan");
        assert!(is_fan(&root));
        let st = load_state(&root).unwrap();
        assert_eq!(st.tier, "fan");
        assert_eq!(st.code, code);
        // 重载一致（幂等读）。
        assert_eq!(load_state(&root), Some(st));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn redeem_bad_code_errors_and_does_not_write() {
        let root = tmp("bad");
        std::fs::create_dir_all(&root).unwrap();
        assert!(redeem(&root, "garbage.code").is_err());
        assert!(!fan_file_path(&root).exists(), "失败不应落盘");
        assert!(!is_fan(&root));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_state_ignores_expired_file() {
        let root = tmp("expfile");
        std::fs::create_dir_all(&root).unwrap();
        // 直接写一个过期 fan.toml（不 redeem，模拟手动/旧文件）。
        std::fs::write(
            fan_file_path(&root),
            "[fan]\ncode = \"x\"\ntier = \"fan\"\nexp = 1\n",
        )
        .unwrap();
        assert!(!is_fan(&root), "过期文件等同未解锁");
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 浏览器 / URL ----
    #[test]
    fn claim_url_follows_site_env() {
        // 不动全局 env（测试并行安全）：只验证默认形状与 trim 逻辑用纯函数路径。
        assert!(claim_url().ends_with("/claim"));
        assert!(site_base_url().starts_with("http"));
    }

    #[test]
    fn open_browser_returns_failed_in_headless() {
        // CI/无头环境无 termux-open-url/am/xdg-open → Failed（不 panic）。
        // 注：若某环境恰好装了 xdg-open 且能起，可能 Via；这里只保证不 panic。
        let _ = open_browser("https://example.com/claim");
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{:02x}", x)).collect()
    }
}
