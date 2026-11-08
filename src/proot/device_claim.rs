//! 设备声明（Device Claim）——申请登录码时必须随请求提交的「设备证明」。
//!
//! 用户口径（2026-11-07）：登录码这类请求需要带上能证明「这是什么设备」的专属信息，
//! 但**绝不能触及用户隐私**。因此本模块只收集粗粒度、非身份标识的环境事实：
//!
//! | 字段 | 含义 | 为什么不算隐私 |
//! |---|---|---|
//! | `device_id` | 本机随机生成的 32 位 hex，落盘可删可重置 | 与硬件无关，不是 MAC/IMEI/序列号 |
//! | `platform` | 编译期常量 `linux` / `android` / … | 粗粒度 OS 类别 |
//! | `arch` | 编译期常量 `x86_64` / `aarch64` / … | CPU 架构 |
//! | `client` | 发起请求的客户端名 | 软件标识 |
//! | `client_version` | 客户端语义版本 | 软件版本 |
//!
//! **明确不收集**：主机名、用户名、MAC/IMEI/序列号、精确 OS 版本串、磁盘路径、
//! IP、地理位置、浏览器 UA 原文、任何文件内容。
//!
//! 载体：固定字段顺序的 canonical JSON **原文** → `X-Mox-Device-Claim` 请求头
//! （同时放进 JSON body 作为反代剥离 header 时的兜底）。后端强制校验：
//! 缺失、字段非法、含未知字段/隐私字段，一律拒绝签发登录码。
//! 轮询换 token 时带同一声明，服务端比对哈希，防止设备码被别的设备截获冒领。
//!
//! 2026-11-08 真机实测：旧版 header_value() 发 base64url，而后端只做 JSON.parse，
//! `mox-proot login` 恒 400 invalid_device_claim。现在头值统一为原始 JSON；
//! 后端仍兼容 base64url（新旧客户端都能用），双端都不再互相锁死。

use std::fs;
use std::io::Read;
use std::path::PathBuf;

/// 数据根目录（与 device_token 同处，便于用户统一清理/重置身份）。
pub fn data_root() -> PathBuf {
    std::env::var("MOX_ROOT")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".mox")
        })
}

/// 本机设备声明（全部字段由本模块生成，不接受外部输入）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceClaim {
    pub device_id: String,
    pub platform: &'static str,
    pub arch: &'static str,
    pub client: &'static str,
    pub client_version: &'static str,
}

const CLIENT: &str = "mox-proot";
const VERSION: &str = env!("CARGO_PKG_VERSION");

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    }
}

fn arch() -> &'static str {
    if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "arm") {
        "armv7"
    } else if cfg!(target_arch = "x86") {
        "i686"
    } else if cfg!(target_arch = "riscv64") {
        "riscv64"
    } else {
        "unknown"
    }
}

impl DeviceClaim {
/// 字段顺序固定，服务端按同一顺序算哈希；任何多余字段都不得进入。
pub fn canonical_json(&self) -> String {
    format!(
        "{{\"device_id\":\"{}\",\"platform\":\"{}\",\"arch\":\"{}\",\"client\":\"{}\",\"client_version\":\"{}\"}}",
        esc(&self.device_id),
        esc(self.platform),
        esc(self.arch),
        esc(self.client),
        esc(self.client_version),
    )
}

/// 人眼可读摘要（终端提示 / 手机确认页展示用）。
pub fn label(&self) -> String {
    format!("{} · {} · {} {}", self.platform, self.arch, self.client, self.client_version)
}

/// `X-Mox-Device-Claim` 头值：canonical JSON **原文**（不再 base64url）。
/// 后端直接 JSON.parse 该头；同时兼容旧版 base64url，但新客户端一律发原文。
pub fn header_value(&self) -> String {
    self.canonical_json()
}
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(H[(b >> 4) as usize] as char);
        s.push(H[(b & 15) as usize] as char);
    }
    s
}

fn valid_device_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 随机 16 字节（优先 /dev/urandom；不可用时退化为时间+pid 混合，仍非硬件标识）。
fn random_16() -> [u8; 16] {
    let mut b = [0u8; 16];
    if let Ok(mut f) = fs::File::open("/dev/urandom") {
        if f.read_exact(&mut b).is_ok() {
            return b;
        }
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let mut x = nanos ^ ((std::process::id() as u64) << 32) ^ (std::process::id() as u64);
    for chunk in b.chunks_mut(8) {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        for (i, v) in chunk.iter_mut().enumerate() {
            *v = (x >> (i * 8)) as u8;
        }
    }
    b
}

/// 读取或生成本机 device_id。它只是**本地随机标识**，用户删掉
/// `$MOX_ROOT/device_id` 即完成重置，不绑定任何硬件信息。
pub fn load_or_create_device_id() -> Result<String, String> {
    let dir = data_root();
    let path = dir.join("device_id");
    if let Ok(s) = fs::read_to_string(&path) {
        let t = s.trim();
        if valid_device_id(t) {
            return Ok(t.to_string());
        }
    }
    let id = hex(&random_16());
    fs::create_dir_all(&dir).map_err(|e| format!("创建 {dir:?} 失败: {e}"))?;
    fs::write(&path, format!("{id}\n")).map_err(|e| format!("写 device_id 失败: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(id)
}

/// 构造当前设备的声明（唯一入口）。
pub fn build() -> Result<DeviceClaim, String> {
    Ok(DeviceClaim {
        device_id: load_or_create_device_id()?,
        platform: platform(),
        arch: arch(),
        client: CLIENT,
        client_version: VERSION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim() -> DeviceClaim {
        DeviceClaim {
            device_id: "0123456789abcdef0123456789abcdef".into(),
            platform: "linux",
            arch: "x86_64",
            client: "mox-proot",
            client_version: "0.9.0",
        }
    }

    #[test]
    fn canonical_json_is_fixed_order_and_has_no_private_fields() {
        let j = claim().canonical_json();
        assert_eq!(
            j,
            r#"{"device_id":"0123456789abcdef0123456789abcdef","platform":"linux","arch":"x86_64","client":"mox-proot","client_version":"0.9.0"}"#
        );
        for forbidden in ["host", "user", "mac", "imei", "serial", "ip", "path", "ua", "email"] {
            assert!(!j.to_lowercase().contains(&format!("\"{forbidden}\"")), "泄漏字段 {forbidden}");
        }
    }

    #[test]
    fn header_value_is_raw_canonical_json() {
        let c = claim();
        let h = c.header_value();
        // 必须是原始 JSON 头，不是 base64url（旧版与后端 JSON.parse 不匹配）
        assert_eq!(h, c.canonical_json());
        assert!(h.starts_with('{') && h.ends_with('}'), "头值应为 JSON 对象");
        assert!(!h.contains('=') || h.contains("\""), "不得退化为裸 base64");
        assert!(h.contains("\"device_id\"") && h.contains("\"client_version\""));
    }

    #[test]
    fn device_id_is_local_random_hex_not_hardware() {
        let c = build().unwrap();
        assert!(valid_device_id(&c.device_id), "device_id 必须是 32 位 hex：{}", c.device_id);
        // 两次构造拿到同一个值（持久化），但不是主机名/用户名
        assert_eq!(c.device_id, build().unwrap().device_id);
        assert!(!c.label().is_empty());
    }

    #[test]
    fn escaping_cannot_inject_extra_fields() {
        let mut c = claim();
        c.device_id = "a\",\"admin\":true".into();
        let j = c.canonical_json();
        // 值被转义，不会变成新的 JSON 键
        assert!(j.contains("a\\\",\\\"admin\\\":true"));
        assert!(!j.contains("\"admin\":true"));
    }
}
