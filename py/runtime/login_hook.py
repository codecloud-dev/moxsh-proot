#!/usr/bin/env python3
"""登录前体检钩子：在登录 shell 启动前输出容器网络环境快照，失败仅警告不阻断。"""

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
    cid = os.environ.get("MOX_CONTAINER_ID", "?")
    print(f"[login-hook] 容器 {cid} 环境快照")
    print(f"[login-hook] hostname={socket.gethostname()} python={sys.version.split()[0]}")
    v6 = ipv6_reachable()
    print(f"[login-hook] IPv6 出网: {'可达' if v6 else '不可达（v4 不受影响）'}")
    # 体检钩子永不失败——它的产物是信息，不是门槛。
    return 0


if __name__ == "__main__":
    sys.exit(main())
