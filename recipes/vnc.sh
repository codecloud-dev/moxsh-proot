#!/bin/sh
# 配方：VNC 服务（tigervnc）—— 端口=5900+display，支持中文与音频转发
#        （端口模型参考 tmoe：display 默认 2 避开 :0/:1；PULSE_SERVER 默认本机 4713）
. /tmp/moxsh-lib.sh
moxsh_info "配置 VNC 服务（tigervnc）…"

MOXSH_VNC_DISPLAY="${MOXSH_VNC_DISPLAY:-2}"
MOXSH_VNC_PORT=$((5900 + MOXSH_VNC_DISPLAY))
MOXSH_VNC_RES="${MOXSH_VNC_RES:-1440x720}"
pkginstall tigervnc

mkdir -p "$HOME/.vnc"
VNC_PASS="${MOXSH_VNC_PASS:-moxsh}"
# tigervnc 的 vncpasswd 交互式，用 printf 喂两遍（回车确认）
if command -v vncpasswd >/dev/null 2>&1; then
    printf '%s\n%s\n' "$VNC_PASS" "$VNC_PASS" | vncpasswd "$HOME/.vnc/passwd" >/dev/null 2>&1 || true
    chmod 600 "$HOME/.vnc/passwd" 2>/dev/null || true
fi

# 容器内固定启动脚本（DE 默认走 Xsession，可用 MOXSH_DE 覆盖）
cat > "$HOME/.vnc/start" <<'EOF'
#!/bin/sh
export DISPLAY=":${MOXSH_VNC_DISPLAY:-2}"
export PULSE_SERVER="${PULSE_SERVER:-127.0.0.1:4713}"
if [ -r /etc/X11/xinit/Xsession ]; then
    exec /etc/X11/xinit/Xsession
fi
exec "${MOXSH_DE:-startxfce4}" || exec /bin/sh
EOF
chmod +x "$HOME/.vnc/start"

moxsh_ok "VNC 已配置：display=:${MOXSH_VNC_DISPLAY} 端口=${MOXSH_VNC_PORT} 分辨率=${MOXSH_VNC_RES} 密码=${VNC_PASS}"
moxsh_info "在容器内启动："
echo "  vncserver :${MOXSH_VNC_DISPLAY} -geometry ${MOXSH_VNC_RES} -localhost no -rfbauth \$HOME/.vnc/passwd && \$HOME/.vnc/start"
