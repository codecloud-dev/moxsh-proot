#!/bin/sh
# 配方：systemd 常驻（完整 init，ProoT 增值服务 proot-pro 专属）
# 让容器以 systemd 作为 PID 1 运行，支持服务自启、套接字激活与日志归集。
. /tmp/mox-proot-lib.sh
mox-proot_info "配置 systemd 常驻（ProoT 增值能力）…"

case "$(detect_family)" in
  debian)
    pkginstall systemd systemd-sysv 2>/dev/null
    ;;
  alpine)
    pkginstall systemd 2>/dev/null
    ;;
  arch)
    pkginstall systemd systemd-sysvcompat 2>/dev/null
    ;;
  redhat)
    pkginstall systemd 2>/dev/null
    ;;
  suse)
    pkginstall systemd 2>/dev/null
    ;;
esac

# ProoT 用户态无真正 PID 1 接管；提供一键启动脚本，在容器内以 systemd 托管服务
cat > /usr/local/bin/mox-proot-systemd <<'EOF'
#!/bin/sh
# 以 systemd 作为容器内 init 运行（需 ProoT 启用 --as-pid1 类支持）
export SYSTEMD_LOG_LEVEL=info
exec /lib/systemd/systemd --system --unit=multi-user.target "$@"
EOF
chmod +x /usr/local/bin/mox-proot-systemd

mox-proot_ok "systemd 已就绪。容器内运行 /usr/local/bin/mox-proot-systemd 即以 systemd 托管服务"
