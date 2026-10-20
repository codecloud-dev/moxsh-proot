#!/bin/sh
# 配方：KDE Plasma 桌面（clean-room）
. /tmp/moxsh-lib.sh
moxsh_info "安装 KDE Plasma 桌面环境…"
case "$(detect_family)" in
    debian) pkginstall kde-plasma-desktop ;;
    alpine) pkginstall plasma-desktop ;;
    arch)   pkginstall plasma plasma-wayland-session ;;
    redhat) pkginstall @kde-desktop ;;
    suse)   pkginstall patterns-kde-kde_plasma ;;
    *)      pkginstall kde-plasma-desktop ;;
esac
moxsh_ok "KDE Plasma 桌面就绪"
