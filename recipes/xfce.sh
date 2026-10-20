#!/bin/sh
# 配方：XFCE 桌面（clean-room，参考 tmoe 桌面安装思路，重写优化）
. /tmp/moxsh-lib.sh
moxsh_info "安装 XFCE 桌面环境…"
pkginstall xfce4 xfce4-goodies xfce4-terminal
# 中文与 CJK 字体按发行族分流（tmoe 在数十个 DE 函数里各写一遍，这里一处搞定）
case "$(detect_family)" in
    debian) pkginstall fonts-noto-cjk language-pack-zh-hans ;;
    alpine) pkginstall font-noto-cjk ;;
    arch)   pkginstall noto-fonts-cjk adobe-source-han-sans-otc-fonts ;;
    redhat) pkginstall google-noto-cjk-fonts ;;
    suse)   pkginstall noto-sans-cjk-fonts ;;
esac
moxsh_ok "XFCE 桌面就绪；接配方 vnc 起图形会话，或配方 novnc 网页访问"
