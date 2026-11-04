#!/bin/sh
# 配方：noVNC 网页访问 —— websockify 把 VNC TCP 桥接成浏览器 WebSocket
#        （依赖 vnc 配方先起好 VNC；端口模型参考 tmoe：noVNC 默认 36080）
. /tmp/mox-proot-lib.sh
mox-proot_info "配置 noVNC 网页访问…"

MOX_VNC_DISPLAY="${MOX_VNC_DISPLAY:-2}"
MOX_VNC_PORT=$((5900 + MOX_VNC_DISPLAY))
MOX_NOVNC_PORT="${MOX_NOVNC_PORT:-36080}"
pkginstall novnc websockify

mox-proot_ok "noVNC 就绪"
mox-proot_info "在容器内启动（需先用 vnc 配方起 VNC）："
echo "  websockify --web /usr/share/novnc ${MOX_NOVNC_PORT} localhost:${MOX_VNC_PORT}"
mox-proot_info "浏览器打开： http://<设备IP>:${MOX_NOVNC_PORT}/vnc.html"
mox-proot_info "管理全部容器：容器内固定命令 mox（反穿透到宿主，list/install/env…）"
