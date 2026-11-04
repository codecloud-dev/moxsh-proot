#!/bin/sh
# 配方：VNC 服务（tigervnc）—— 端口=5900+display，支持中文与音频转发
#        （端口模型参考 tmoe：display 默认 2 避开 :0/:1；PULSE_SERVER 默认本机 4713）
. /tmp/mox-proot-lib.sh
mox-proot_info "配置 VNC 服务（tigervnc）…"

MOX_VNC_DISPLAY="${MOX_VNC_DISPLAY:-2}"
MOX_VNC_PORT=$((5900 + MOX_VNC_DISPLAY))
MOX_VNC_RES="${MOX_VNC_RES:-1440x720}"
pkginstall tigervnc

mkdir -p "$HOME/.vnc"
VNC_PASS="${MOX_VNC_PASS:-mox-proot}"
# tigervnc 的 vncpasswd 交互式，用 printf 喂两遍（回车确认）
if command -v vncpasswd >/dev/null 2>&1; then
    printf '%s\n%s\n' "$VNC_PASS" "$VNC_PASS" | vncpasswd "$HOME/.vnc/passwd" >/dev/null 2>&1 || true
    chmod 600 "$HOME/.vnc/passwd" 2>/dev/null || true
fi

# 容器内固定启动脚本（DE 默认走 Xsession，可用 MOX_DE 覆盖）
cat > "$HOME/.vnc/start" <<'EOF'
#!/bin/sh
export DISPLAY=":${MOX_VNC_DISPLAY:-2}"
export PULSE_SERVER="${PULSE_SERVER:-127.0.0.1:4713}"
if [ -r /etc/X11/xinit/Xsession ]; then
    exec /etc/X11/xinit/Xsession
fi
exec "${MOX_DE:-startxfce4}" || exec /bin/sh
EOF
chmod +x "$HOME/.vnc/start"

mox-proot_ok "VNC 已配置：display=:${MOX_VNC_DISPLAY} 端口=${MOX_VNC_PORT} 分辨率=${MOX_VNC_RES} 密码=${VNC_PASS}"

# 融合点：把图形会话变量写进我们的持久 env 文件（引擎登录时自动注入，
# 且随 backup 走）——下次 launch 无需再手动 export。幂等 upsert。
ENVF=/etc/mox-proot/env
if [ -w "$(dirname "$ENVF")" ] 2>/dev/null || [ -w "$ENVF" ] 2>/dev/null; then
    mkdir -p "$(dirname "$ENVF")" 2>/dev/null || true
    for key in DISPLAY PULSE_SERVER; do
        sed -i "/^${key}=/d" "$ENVF" 2>/dev/null || true
    done
    {
        echo "DISPLAY=:${MOX_VNC_DISPLAY}"
        echo "PULSE_SERVER=127.0.0.1:4713"
    } >> "$ENVF" 2>/dev/null || true
    mox-proot_ok "已写入 ${ENVF}（登录即注入，随备份走）"
fi

mox-proot_info "在容器内启动："
echo "  vncserver :${MOX_VNC_DISPLAY} -geometry ${MOX_VNC_RES} -localhost no -rfbauth \$HOME/.vnc/passwd && \$HOME/.vnc/start"
mox-proot_info "管理全部容器：容器内固定命令 mox（反穿透到宿主，list/install/env…）"
