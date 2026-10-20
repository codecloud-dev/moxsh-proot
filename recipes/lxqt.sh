#!/bin/sh
# 配方：LXQt 桌面（clean-room）
. /tmp/moxsh-lib.sh
moxsh_info "安装 LXQt 桌面环境…"
case "$(detect_family)" in
    debian) pkginstall lxqt sddm ;;
    alpine) pkginstall lxqt ;;
    arch)   pkginstall lxqt ;;
    redhat) pkginstall @lxqt ;;
    suse)   pkginstall patterns-lxqt-lxqt ;;
    *)      pkginstall lxqt ;;
esac
moxsh_ok "LXQt 桌面就绪"
