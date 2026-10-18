#!/usr/bin/env python3
"""pre-login 体检钩子（ProOT 引擎内嵌 Python 运行时，M8）。

引擎在登录 shell 启动前于容器内执行（`moxsh-proot-distro login <id>`；
`moxsh-proot-distro shell <id>`）。失败仅警告、不阻断登录。

与 post-install/provision.py 的分工：那边管「装完时供给是否就绪」，
这边管「每次登录时的环境快照」——网络边界报告（IPv6 可达性/DNS 状态）
即引擎 net 模块语义的脚本侧呈现，供用户一眼判断容器网络健康度。
"""

from __future__ import annotations

import os
import socket
import sys


def ipv6_reachable(host: str = "2400:3200::1", port: int = 53) -> bool:
    """IPv6 连通性探测（UDP connect 只建路由不发包，与引擎探测同语义）。"""
    try:
        infos = socket.getaddrinfo(
            host, port, socket.AF_INET6, socket.SOCK_DGRAM, socket.IPPROTO_UDP
        )
        family, socktype, proto, _, addr = infos[0]
        with socket.socket(family, socktype, proto) as s:
            s.settimeout(2.0)
            s.connect(addr)
        return True
    except OSError:
        return False


def main() -> int:
    cid = os.environ.get("MOXSH_CONTAINER_ID", "?")
    print(f"[login-hook] 容器 {cid} 环境快照")
    print(f"[login-hook] hostname={socket.gethostname()} python={sys.version.split()[0]}")
    v6 = ipv6_reachable()
    print(f"[login-hook] IPv6 出网: {'可达' if v6 else '不可达（v4 不受影响）'}")
    # 体检钩子永不失败——它的产物是信息，不是门槛。
    return 0


if __name__ == "__main__":
    sys.exit(main())
