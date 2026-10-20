#!/bin/sh
# 配方：MATE 桌面（clean-room）
. /tmp/moxsh-lib.sh
moxsh_info "安装 MATE 桌面环境…"
case "$(detect_family)" in
    debian) pkginstall mate-desktop-environment mate-terminal ;;
    alpine) pkginstall mate-desktop-environment ;;
    arch)   pkginstall mate mate-terminal ;;
    redhat) pkginstall @mate-desktop ;;
    suse)   pkginstall patterns-mate-mate ;;
    *)      pkginstall mate-desktop-environment ;;
esac
moxsh_ok "MATE 桌面就绪"
