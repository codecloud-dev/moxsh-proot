#!/bin/sh
# 配方：粉丝专属锦囊（fan_only）——一键全家桶，感谢支持。
#
# 融合我们的独家能力（不是通用脚本）：
#   - 跑在 exec_into → bind_console 的容器环境里，$MOXSH_ROOT 已 bind 到 /moxsh-root；
#   - 收尾把 DISPLAY/PULSE_SERVER 写进 /etc/moxsh/env——我们的持久 env 文件
#     （引擎每次登录自动注入，且随 backup 走），下次 launch 即生效；
#   - 提示容器内固定命令 mox 接管后续管理。
. /tmp/moxsh-lib.sh
moxsh_info "装配粉丝专属锦囊（XFCE + 浏览器 + 中文 + VNC）…"

FAM="$(detect_family)"
pkginstall xfce4 xfce4-terminal tigervnc || true

# 浏览器按发行族分流（名字不一，取到即用）。
case "$FAM" in
    debian) pkginstall chromium || pkginstall firefox-esr || true ;;
    alpine) pkginstall chromium || pkginstall firefox || true ;;
    arch)   pkginstall chromium || pkginstall firefox || true ;;
    redhat) pkginstall chromium || pkginstall firefox || true ;;
esac

# 中文与 CJK 字体。
case "$FAM" in
    debian) pkginstall fonts-noto-cjk language-pack-zh-hans ;;
    alpine) pkginstall font-noto-cjk ;;
    arch)   pkginstall noto-fonts-cjk ;;
    redhat) pkginstall google-noto-cjk-fonts ;;
    suse)   pkginstall noto-sans-cjk-fonts ;;
esac

# 融合点：把图形会话变量写进我们的持久 env 文件（引擎登录时注入 + 随 backup 走）。
ENVF=/etc/moxsh/env
mkdir -p "$(dirname "$ENVF")" 2>/dev/null || true
if [ -w "$(dirname "$ENVF")" ] || [ -w "$ENVF" ] 2>/dev/null; then
    # 幂等 upsert：先删同名旧行再追加（避免重复累积）。
    for key in DISPLAY PULSE_SERVER; do
        sed -i "/^${key}=/d" "$ENVF" 2>/dev/null || true
    done
    {
        echo "DISPLAY=:2"
        echo "PULSE_SERVER=127.0.0.1:4713"
    } >> "$ENVF" 2>/dev/null || true
    moxsh_ok "已把 DISPLAY/PULSE_SERVER 写入 ${ENVF}（登录即注入，随备份走）"
else
    moxsh_info "env 文件不可写，跳过持久化（不影响本次，下次登录用默认值）"
fi

moxsh_ok "粉丝专属锦囊就绪。"
moxsh_info "起图形会话：配方 vnc（VNC 服务）→ 容器内输入 mox 管理全部容器"
