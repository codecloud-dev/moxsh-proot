#!/usr/bin/env python3
"""把付费配方脚本混淆（XOR + base64）后内嵌进二进制，磁盘只留无害占位桩。

目的（渗透测试报告·路径③）：付费脚本本体此前是 0644 明文文件，攻击者可
直接 `sh recipes/pro-gpu.sh` 绕过门禁。混淆后仅门禁通过时在内存解码使用，
磁盘上的同名文件改为无害占位，从"文本即可运行"抬到"需逆向+改二进制"。

这不是密码学强度（密钥编译期内嵌），只是抬高绕过成本；真正的不可破解
需服务端下发（见 redeem.rs 模块文档）。
"""
import base64
import os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
RECIPES = os.path.join(REPO, "recipes")

PAID = {
    "pro-gpu": "pro-gpu.sh",
    "pro-nested": "pro-nested.sh",
    "pro-systemd": "pro-systemd.sh",
    "pro-snapshot": "pro-snapshot.sh",
}

# 32 字节混淆密钥（编译期内嵌；仅作混淆，非密钥保密用途）
KEY = bytes([0x4D, 0x6F, 0x78, 0x50, 0x72, 0x6F, 0x6F, 0x74,
             0x9A, 0x3C, 0x17, 0xE2, 0x55, 0xC1, 0x88, 0x0B,
             0x2F, 0x7D, 0x44, 0x91, 0xAE, 0x60, 0x12, 0xD3,
             0x7B, 0x05, 0xC4, 0xEF, 0x9D, 0x23, 0x6A, 0xF8])


def obfuscate(text: str) -> str:
    data = text.encode("utf-8")
    out = bytes(data[i] ^ KEY[i % len(KEY)] for i in range(len(data)))
    return base64.b64encode(out).decode("ascii")


def main() -> None:
    blobs = {}
    for rid, fname in PAID.items():
        path = os.path.join(RECIPES, fname)
        with open(path, "r", encoding="utf-8") as f:
            src = f.read()
        # 防呆：源文件已被占位桩替换时，绝不能把桩内容混淆内嵌（会让
        # decode_paid 解出占位桩）。需先从 git 恢复原始脚本再跑本脚本。
        if "占位桩" in src:
            raise SystemExit(
                f"错误：{fname} 已是占位桩，无法内嵌真实脚本。\n"
                f"请先 `git checkout HEAD -- recipes/{fname}` 恢复原始脚本，再运行本脚本。"
            )
        blobs[rid] = obfuscate(src)
        # 替换为无害占位桩：门禁未过时不执行任何真实逻辑
        with open(path, "w", encoding="utf-8") as f:
            f.write(
                "#!/bin/sh\n"
                "# 占位桩：真实脚本已内嵌于二进制并混淆，仅在付费门禁通过后由\n"
                "# `mox-proot-distro recipe apply` 在内存解码执行。本文件不执行任何\n"
                "# 真实逻辑，直接运行无害（渗透测试路径③修复）。\n"
                "echo 'mox-proot: 该配方为付费内容，请通过 `mox redeem login` 授权后使用 recipe apply'\n"
                "exit 0\n"
            )
        print(f"obfuscated + stubbed: {fname}")

    key_arr = ", ".join(f"0x{b:02X}" for b in KEY)
    entries = ",\n".join(
        f'        ("{rid}", "{blob}")' for rid, blob in blobs.items()
    )
    module = f"""//! 付费配方脚本内嵌（混淆）存储——不在磁盘落明文，防直接 `sh`（渗透测试路径③）。
//!
//! 运行期仅在门禁（requires 判定）通过后在内存 XOR 解码使用。解码密钥
//! 编译期内嵌，属"抬高逆向成本"的混淆（非密码学强度）；真正不可破解需
//! 服务端下发（见 redeem.rs 模块文档）。磁盘上的 `recipes/pro-*.sh` 为无害占位桩。
//!
//! 不引 base64 crate：下方内联一个极简 base64 标准字母解码器（仅本文件用）。

/// 混淆密钥（仅作混淆用途，非保密）。
const KEY: [u8; 32] = [{key_arr}];

/// (配方 id, base64(XOR(明文))) 查表。
const PAID: &[(&str, &str)] = &[
{entries}
];

/// 极简 base64 标准字母解码（内联，避免新增依赖）。无效字符→None。
fn b64_decode(s: &str) -> Option<Vec<u8>> {{
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {{
        let v = match c {{
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => continue,
            _ => return None,
        }};
        buf = (buf << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {{
            bits -= 8;
            out.push((buf >> bits) as u8);
        }}
    }}
    Some(out)
}}

/// 解码付费脚本明文（门禁通过后调用）。未知 id 返回 None。
pub fn decode_paid(id: &str) -> Option<String> {{
    let blob = PAID.iter().find(|(rid, _)| *rid == id).map(|(_, b)| *b)?;
    let raw = b64_decode(blob)?;
    let text: Vec<u8> = raw.iter().enumerate().map(|(i, b)| b ^ KEY[i % KEY.len()]).collect();
    String::from_utf8(text).ok()
}}
"""
    out = os.path.join(REPO, "src", "proot", "recipe_paid.rs")
    with open(out, "w", encoding="utf-8") as f:
        f.write(module)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
