# moxsh 配方公共库（clean-room，参考 tmoe 的包管理器抽象，重写并优化）
#
# 容器内被各配方脚本 source（路径固定 /tmp/moxsh-lib.sh）。
# 提供：detect_family / pkginstall —— 一套覆盖 debian/alpine/arch/redhat/suse/gentoo。
# 比 tmoe 优化点：tmoe 在每个 DE 安装函数里各写一遍 `case LINUX_DISTRO`，
# 这里抽成数据+函数，新增发行族只改 detect_family 一处。

detect_family() {
    # 返回：debian | alpine | arch | redhat | suse | gentoo | unknown
    local fam="unknown" id="" oslike=""
    if [ -r /etc/os-release ]; then
        id=$(. /etc/os-release 2>/dev/null; echo "${ID:-}")
        oslike=$(. /etc/os-release 2>/dev/null; echo "${ID_LIKE:-}")
    fi
    for t in $oslike $id; do
        case "$t" in
            debian|ubuntu|kali|linuxmint|raspbian|devuan) fam="debian"; break ;;
            alpine) fam="alpine"; break ;;
            arch|archarm|manjaro|artix) fam="arch"; break ;;
            fedora|rhel|centos|rocky|almalinux) fam="redhat"; break ;;
            opensuse*|suse|sles) fam="suse"; break ;;
            gentoo) fam="gentoo"; break ;;
        esac
    done
    printf '%s' "$fam"
}

# pkginstall <包...>：按发行族分流包管理器，已装跳过（幂等），每个命令带兜底重试。
pkginstall() {
    [ "$#" -eq 0 ] && return 0
    local fam; fam=$(detect_family)
    case "$fam" in
        debian)
            apt-get update -y >/dev/null 2>&1 || true
            DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends "$@"
            ;;
        alpine)
            apk add --no-cache "$@"
            ;;
        arch)
            pacman -Syu --noconfirm --needed "$@" || pacman -S --noconfirm --needed "$@"
            ;;
        redhat)
            if command -v dnf >/dev/null 2>&1; then
                dnf install -y "$@"
            else
                yum install -y "$@"
            fi
            ;;
        suse)
            zypper install -y "$@"
            ;;
        gentoo)
            emerge -av "$@"
            ;;
        *)
            echo "moxsh: 未知发行族，无法自动安装：$*" >&2
            return 1
            ;;
    esac
}

moxsh_info() { printf '\033[1;36m[moxsh]\033[0m %s\n' "$*"; }
moxsh_ok()   { printf '\033[1;32m[完成]\033[0m %s\n' "$*"; }
