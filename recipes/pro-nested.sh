#!/bin/sh
# 配方：嵌套容器 Docker / Podman（ProoT 增值服务 proot-pro 专属）
# 在 ProoT 容器内再跑一个容器运行时，支持镜像构建与容器编排。
# 注意：嵌套容器运行在 ProoT 的用户态隔离内，不碰宿主系统分区。
. /tmp/moxsh-lib.sh
moxsh_info "安装嵌套容器运行时（ProoT 增值能力）…"

case "$(detect_family)" in
  debian)
    pkginstall docker.io 2>/dev/null || pkginstall podman 2>/dev/null
    ;;
  alpine)
    pkginstall docker 2>/dev/null || pkginstall podman 2>/dev/null
    ;;
  arch)
    pkginstall docker 2>/dev/null || pkginstall podman 2>/dev/null
    ;;
  redhat)
    pkginstall podman 2>/dev/null || pkginstall docker-ce 2>/dev/null
    ;;
  suse)
    pkginstall podman 2>/dev/null || pkginstall docker 2>/dev/null
    ;;
esac

# ProoT 用户态环境下无法跑真正的 dockerd（需特权）；优先用 rootless podman + fuse-overlayfs
if command -v podman >/dev/null 2>&1; then
  pkginstall fuse-overlayfs slirp4netns 2>/dev/null
  moxsh_ok "已安装 podman（rootless）。启动示例：podman run --rm hello-world"
elif command -v docker >/dev/null 2>&1; then
  moxsh_ok "已安装 docker。注意：ProoT 用户态内 dockerd 需宿主特权，建议改用 podman rootless"
else
  moxsh_ok "未找到可用运行时；请确认软件源可用后重试"
fi
