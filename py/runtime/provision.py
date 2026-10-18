#!/usr/bin/env python3
"""post-install 供给钩子（ProOT 引擎内嵌 Python 运行时，M8）。

引擎在发行版安装完成、DNS/hosts 初始化之后，于容器内执行本脚本
（`moxsh-proot-distro install <id>` 触发；rootfs 无 python3 时优雅跳过）。

Python 特长点落点：标准库丰富——DNS 解析、hosts 核对、结构化报告
这类数据型任务无需第三方依赖；且本脚本**在宿主侧同样可直接运行**
（开发期自证，见 py/verify/）。

约定（引擎注入的环境变量）：
    MOXSH_CONTAINER_ID   容器 id（如 ubuntu）
    MOXSH_HOOK_KIND      钩子类型（post-install）
    MOXSH_HOOK_DIR       钩子目录 guest 路径（/moxsh/hooks）

退出码：0 = 通过；1 = 供给未完全就绪（不阻断安装，仅报告）。
"""

from __future__ import annotations

import os
import socket
import sys
import time

REQUIRED_ENV = ("MOXSH_CONTAINER_ID", "MOXSH_HOOK_KIND", "MOXSH_HOOK_DIR")
# 解析超时与上游 resolv.conf 的 options 对齐（timeout:2 attempts:2）。
PROBE_TIMEOUT = 2.0


def check_contract() -> list[str]:
    """环境契约检查：引擎约定的变量必须齐全。"""
    missing = [k for k in REQUIRED_ENV if not os.environ.get(k)]
    return missing


def check_dns(host: str = "archive.ubuntu.com") -> tuple[bool, str]:
    """DNS 解析探针：guest glibc 读 rootfs 内 resolv.conf（引擎已维护），
    这里验证「resolv.conf 里的服务器真的能解析」——网络栈第一现场。"""
    try:
        ip = socket.gethostbyname(host)
        return True, f"{host} -> {ip}"
    except OSError as e:
        return False, f"{host} 解析失败: {e}"


def check_resolv_conf() -> tuple[bool, str]:
    """resolv.conf 形态检查：至少一个 nameserver。"""
    try:
        with open("/etc/resolv.conf", "r", encoding="utf-8") as f:
            servers = [
                ln.split()[1]
                for ln in f
                if ln.strip().startswith("nameserver") and len(ln.split()) > 1
            ]
    except OSError as e:
        return False, f"读取 /etc/resolv.conf 失败: {e}"
    if not servers:
        return False, "/etc/resolv.conf 无 nameserver（引擎应已写入，异常）"
    return True, f"nameserver: {', '.join(servers)}"


def main() -> int:
    cid = os.environ.get("MOXSH_CONTAINER_ID", "?")
    print(f"[provision] 容器 {cid} post-install 供给检查（Python {sys.version.split()[0]}）")

    missing = check_contract()
    if missing:
        print(f"[provision] 环境契约缺失: {', '.join(missing)}（是否绕过了引擎直接运行？）")
        return 1

    ok = True
    for name, (passed, detail) in (
        ("resolv.conf", check_resolv_conf()),
        ("DNS 解析", check_dns()),
    ):
        mark = "✓" if passed else "✗"
        print(f"[provision] {mark} {name}: {detail}")
        ok = ok and passed

    if ok:
        print("[provision] 网络供给就绪")
        return 0
    print("[provision] 网络供给未就绪（不影响安装；登录后用 moxsh-proot-distro login 复查）")
    return 1


if __name__ == "__main__":
    sys.exit(main())
