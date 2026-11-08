#!/usr/bin/env bash
# mox-proot 一键安装脚本（原版 Termux / Linux 用户专用）。
#
# 把 proot 容器引擎（Rust ptrace 路径翻译，非上游 C 版 proot 的封装）
# 装成 `mox-proot` 命令，之后即可：
#
#   mox-proot -R ~/ubuntu -0 -- bash
#
# 安装方式（原版 Termux，二选一，脚本自动探测国内外线路）：
#   curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-mox-proot.sh | bash
#   curl -fsSL https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-mox-proot.sh | bash
#
# 本脚本是安装体系的 **bash 探测层**（零依赖、可 curl|bash）；
# richer 安装/更新体验走 Go 安装器（仓库 go/ 目录，Release 资产
# `mox-proot-installer-<goos>-<arch>`，子命令 install/update/probe）：
#   mox-proot-installer install
# 两层默认下载源一致，均为本仓库 Release。
#
# 设计约束（Termux 现实）：
#   - 只依赖 Termux 自带工具：curl / coreutils（sha256sum、install、mktemp）；
#   - 二进制从 GitHub Release 下载并做 sha256 校验（放不下就不装，绝不静默）；
#   - 非交互：`curl | bash` 下 stdin 是脚本本身，全程无需任何输入；
#   - PREFIX 未设置时按 Termux 常见路径探测，探测不到则明确报错（不猜 system 路径，
#     Android 10+ 的 W^X 会让 system 分区上的可执行文件根本跑不起来）；
#   - 钩子种子（py/runtime + lua/）随仓库 tarball 一并落盘到 $MOX_ROOT/hooks，
#     下载失败不阻断主安装（引擎无钩子照常工作）。

set -euo pipefail

GITHUB_REPO="${MOX_PROOT_REPO:-codecloud-dev/moxsh-proot}"
GITEE_REPO="${MOX_GITEE_REPO:-thousand-code-crafts/moxsh-proot}"
RELEASE="${MOX_PROOT_RELEASE:-proot-latest}"

# NO_COLOR 约定（no-color.org）：设了任意值即关色；非 TTY（curl|bash 重定向）也关色。
if [ -n "${NO_COLOR:-}" ] || [ ! -t 2 ]; then
  C_CYAN=''; C_YELLOW=''; C_RED=''; C_BOLD=''; C_RST=''
else
  C_CYAN='\033[1;36m'; C_YELLOW='\033[1;33m'; C_RED='\033[1;31m'; C_BOLD='\033[1m'; C_RST='\033[0m'
fi
log()  { printf "${C_CYAN}==>${C_RST} %s\n" "$*"; }
warn() { printf "${C_YELLOW}警告:${C_RST} %s\n" "$*" >&2; }
die()  { printf "${C_RED}错误:${C_RST} %s\n" "$*" >&2; exit 1; }
ok()   { printf "${C_CYAN}✔${C_RST} %s\n" "$*"; }

# 紧凑品牌字标（与 src/proot/brand.rs 的 BANNER 一致，4 行 / 手机端不折行）。
printf '\n'
printf "${C_BOLD}"
cat <<'BANNER'
_  _ ____ _  _
|\/| |  |  \/
|  | |__| _/\_
  mox-proot
BANNER
printf "${C_RST}"
printf '  %s\n\n' "多架构容器引擎 · 一键安装"

# ---------------------------------------------------------------- 国内外分流
# 自动选源（0.2.0）：探测 GitHub 与 Gitee 的可达性 + 握手延迟，国内走
# Gitee、国外走 GitHub。覆盖方式（优先级从高到低）：
#   1. MOX_PROOT_BASE_URL 显式指定（镜像自建场景，需保持
#      <BASE_URL>/<ASSET> 与 <BASE_URL>/<ASSET>.sha256 布局）；
#   2. MOX_PROOT_SOURCE=github|gitee 强制选端（不猜 URL）；
#   3. 自动探测（默认）。
#
# 探测端点选轻量资产文件：本架构主二进制的 .sha256 校验文件（Release 资产，
# 几十字节，不计速只计可达与握手耗时）。语义从「仓库可达」升级为「资产可达」：
# 2026-10-25 实跑发现 Gitee release 只有源码包、无二进制资产（raw 探测会误判
# 「可达」），国内用户被选中后白等一轮 404 才切源。探资产则无资产的端自动落选。
# 两端资产都不可达时默认 GitHub——错误信息指向标准源，且瞬时抖动下
# GitHub 的全球可达性通常更好。
probe_seconds() {
  # $1 = URL；**仅 HTTP 200** 才输出耗时（秒），其余一律输出空。
  #
  # 【渗透复测 P2 修复】旧实现 `curl -fsS ... -w '%{time_total}' ... || true`
  # 有致命缺陷：curl 即便因 `-f` 判失败（404 / 域名不存在 / 证书错 / 302 未跟随），
  # `-w` 依然会打印 time_total；`|| true` 又吞掉非零退出码，于是「打不开的源」
  # 被当成「可达且快」，选源直接选错，国内用户白等一轮 404。
  # 现在同时取 http_code，跟随重定向，只有最终 200 才算可达；连接失败时
  # http_code 为 000，同样落选。探测的是 .sha256 小文件，全量下载无压力。
  local out code t
  out="$(curl -sSL -o /dev/null --connect-timeout 4 --max-time 10 \
        -w '%{http_code} %{time_total}' "$1" 2>/dev/null || true)"
  code="${out%% *}"
  t="${out#* }"
  if [ "$code" = "200" ] && [ -n "$t" ]; then
    printf '%s' "$t"
  fi
}

pick_source() {
  if [ -n "${MOX_PROOT_BASE_URL:-}" ]; then
    SOURCE="custom"
    BASE_URL="$MOX_PROOT_BASE_URL"
    ARCHIVE_URL=""
    log "下载源：自定义镜像 ${BASE_URL}"
    return
  fi
  if [ "${MOX_PROOT_SOURCE:-auto}" = "github" ]; then
    set_source_github; return
  fi
  if [ "${MOX_PROOT_SOURCE:-auto}" = "gitee" ]; then
    set_source_gitee; return
  fi
  local gh_s gitee_s
  gh_s="$(probe_seconds "https://github.com/${GITHUB_REPO}/releases/download/${RELEASE}/mox-proot-${ASSET_SUFFIX}.sha256")"
  gitee_s="$(probe_seconds "https://gitee.com/${GITEE_REPO}/releases/download/${RELEASE}/mox-proot-${ASSET_SUFFIX}.sha256")"
  if [ -n "$gitee_s" ] && { [ -z "$gh_s" ] || awk -v g="$gitee_s" -v h="$gh_s" 'BEGIN{exit !(g<h)}'; }; then
    set_source_gitee
  else
    set_source_github
  fi
}

set_source_github() {
  SOURCE="github"
  BASE_URL="https://github.com/${GITHUB_REPO}/releases/download/${RELEASE}"
  ARCHIVE_URL="https://github.com/${GITHUB_REPO}/archive/refs/tags/${RELEASE}.tar.gz"
  log "下载源：GitHub（${BASE_URL}）"
}

set_source_gitee() {
  SOURCE="gitee"
  BASE_URL="https://gitee.com/${GITEE_REPO}/releases/download/${RELEASE}"
  ARCHIVE_URL="https://gitee.com/${GITEE_REPO}/repository/archive/${RELEASE}.tar.gz"
  log "下载源：Gitee 国内镜像（${BASE_URL}）"
}

# 下载源回退切换（github ↔ gitee；custom 不可切——用户显式镜像不擅自换）。
# 用于「选错源 / 该端 Release 缺资产」时的自救：切一次，防乒乓。
switch_source() {
  case "$SOURCE" in
    github) set_source_gitee; warn "下载受阻，已回退切换到 Gitee 源重试" ;;
    gitee)  set_source_github; warn "下载受阻，已回退切换到 GitHub 源重试" ;;
    *)      return 1 ;;
  esac
}


# ---------------------------------------------------------------- 环境探测
# 平台判定：存在 Termux 数据目录即视为 Android(Termux)，否则按桌面 Linux 处理
# （Ubuntu/Debian/Arch 等均可装，不再硬绑 Termux）。
IS_ANDROID=0
if [ -d /data/data/com.termux/files/usr ]; then
  IS_ANDROID=1; PREFIX="${PREFIX:-/data/data/com.termux/files/usr}"
elif [ -d /data/data/com.termux.dev/files/usr ]; then
  IS_ANDROID=1; PREFIX="${PREFIX:-/data/data/com.termux.dev/files/usr}"
fi
if [ -z "${PREFIX:-}" ]; then
  if [ "$IS_ANDROID" = 1 ]; then
    die "未检测到 Termux 环境（PREFIX 未设置）。请显式指定安装目录：
  PREFIX=/your/prefix bash install-mox-proot.sh"
  else
    # 桌面 Linux：默认装到 ~/.local（无需 root，符合 FHS 用户目录惯例）。
    PREFIX="${HOME:-$HOME}/.local"
  fi
fi

# 架构：mox-proot 提供 aarch64 与 x86_64 两版（Android + 桌面 Linux 均覆盖）。
case "$(uname -m)" in
  aarch64|arm64)  TRIPLE_ARCH="aarch64"; ARCH_ALIAS="arm64" ;;
  x86_64|amd64)   TRIPLE_ARCH="x86_64";  ARCH_ALIAS="amd64" ;;
  armv7l|armv8l|arm)
    die "检测到 32 位 ARM（$(uname -m)），暂不提供预编译二进制。
请从源码构建：https://github.com/${GITHUB_REPO}#从源码构建" ;;
  *) die "不支持的架构: $(uname -m)" ;;
esac

# 资产后缀：Android 用 -linux-android；桌面 Linux 用 -linux-<arch>（glibc），
# 检测到 musl 时切 -linux-<arch>-musl 静态版（零依赖，适合 Alpine 等）。
if [ "$IS_ANDROID" = 1 ]; then
  ASSET_SUFFIX="${TRIPLE_ARCH}-linux-android"
else
  ASSET_SUFFIX="linux-${ARCH_ALIAS}"
  # 优先静态 musl 版：零 glibc 依赖，可在任意（含老）glibc 系统运行
  # （doc §A#1：gnu 版硬绑 glibc 2.39，老系统跑不起 → 把依赖版本降下来）。
  # 判定：显式 musl，或 glibc 版本低于发布构建地板（2.35）。
  use_musl=0
  if command -v ldd >/dev/null 2>&1; then
    ldd_out="$(ldd --version 2>&1)"
    if printf '%s' "$ldd_out" | grep -qi musl; then
      use_musl=1
    elif printf '%s' "$ldd_out" | grep -qi glibc; then
      glibc_ver="$(printf '%s' "$ldd_out" | grep -oE '[0-9]+\.[0-9]+' | head -1)"
      if [ -n "$glibc_ver" ] && awk "BEGIN{exit !($glibc_ver + 0 < 2.35)}"; then
        use_musl=1
      fi
    fi
  fi
  # 【P0 修复】自救开关必须真生效：此前报错文案让用户设
  # MOX_PROOT_FORCE_MUSL=1，但全文只有那一处提示、判定块根本没读它 —— 用户照做
  # 毫无变化，等于给了条死路。现在显式覆盖探测结果（musl 静态版零 glibc 依赖）。
  if [ "${MOX_PROOT_FORCE_MUSL:-0}" = "1" ]; then
    use_musl=1
    log "已启用 MOX_PROOT_FORCE_MUSL=1：强制使用静态 musl 版"
  fi
  if [ "$use_musl" = 1 ]; then
    ASSET_SUFFIX="linux-${ARCH_ALIAS}-musl"
  fi
fi

BINDIR="${PREFIX}/bin"
[ -d "$BINDIR" ] || mkdir -p "$BINDIR"
[ -w "$BINDIR" ] || die "${BINDIR} 不可写（PREFIX 是否正确？）"

command -v curl >/dev/null 2>&1 || die "未找到 curl（pkg install curl）"
command -v sha256sum >/dev/null 2>&1 || die "未找到 sha256sum（pkg install coreutils）"

# 选源（探测 GitHub/Gitee 延迟；需 curl 就位后执行）。
pick_source

# MOX_ROOT：钩子与容器数据根目录（对齐引擎 DistroManager 约定：
# $MOX_ROOT > $HOME/.mox-proot）。
MOX_ROOT="${MOX_ROOT:-}"
if [ -z "$MOX_ROOT" ]; then
  HOME_DIR="${HOME:-}"
  [ -n "$HOME_DIR" ] || die "HOME 未设置，无法推断 MOX_ROOT（请显式设置）"
  MOX_ROOT="${HOME_DIR}/.mox-proot"
fi

# ---------------------------------------------------------------- 下载 + 校验
TMP="$(mktemp -d "${TMPDIR:-/tmp}/mox-proot.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# 多级镜像兜底通道（国内外分流，2026-11 起）：
#   - ghproxy 类：把 GitHub Release 完整 URL 整体前缀，国内直连 GitHub 超时时的兜底；
#     实测 ghproxy.net 对 Release 资产返回 200。
# 决策（2026-11-07）：官网静态 /dl 通道**不做**——站点从未部署 functions/dl，
# 长期 404，留着只会让每个资产白等一次超时。相关占位代码与注释已整体移除，
# 分发链路固定为 GitHub ↔ Gitee 双端 + 用户可选的 ghproxy 镜像 + sha256 强校验。
# 注：jsDelivr 仅服务仓库内文件、不服务 Release 二进制资产（实测 404），
# 脚本 raw 的 jsDelivr 备援见 README。
#
# 【渗透复测 P2 修复】ghproxy.net 这类公共代理存活率不稳定，且被墙/停服时
# 对每个资产都要白等一次 12s 超时（4 个二进制 × 2 校验文件 ≈ 8 次死等），
# 更糟的是部分代理对不存在的资产返回 200 错误页而非 404，会把探测带偏。
# 改为**默认关闭**，仅当用户显式设置 MOX_PROOT_MIRROR_PROXIES
# （确认代理当前可用）时才加入兜底链。
# 真正兜底靠「另一端 GitHub↔Gitee」+ sha256 强校验，不依赖第三方代理。
MIRROR_PROXIES="${MOX_PROOT_MIRROR_PROXIES:-}"

# 由 GitHub Release 基址派生 ghproxy 镜像候选 URL 列表。
mirror_urls() {
  local base="$1" asset="$2"
  local url="${base}/${asset}"
  for m in $MIRROR_PROXIES; do
    printf '%s%s\n' "$m" "$url"
  done
}

# 带切源重试 + 多级镜像兜底的资产下载（$1 = 资产名，$2 = 输出路径）。
# 兜底顺序：当前选源 → 另一端（github↔gitee）→ ghproxy 镜像（用户显式启用时）。
fetch_asset_file() {
  local asset="$1" out="$2" cand
  # 1) 当前选源（GitHub / Gitee 二选一；Gitee 现已含二进制资产，国内直连可用）
  if curl -fSL --retry 3 --connect-timeout 15 -o "$out" "${BASE_URL}/${asset}"; then
    return 0
  fi
  # 2) 回退另一端（github ↔ gitee）
  if switch_source; then
    if curl -fSL --retry 3 --connect-timeout 15 -o "$out" "${BASE_URL}/${asset}"; then
      return 0
    fi
  fi
  # 3) ghproxy 镜像（GitHub Release 整体前缀，国内直连 GitHub 超时兜底）
  for cand in $(mirror_urls "https://github.com/${GITHUB_REPO}/releases/download/${RELEASE}" "$asset"); do
    if curl -fSL --retry 2 --connect-timeout 12 -o "$out" "$cand"; then
      return 0
    fi
  done
  return 1
}

# 下载并校验单个资产（$1 = 资产名）；失败即 die（"放不下就不装"）。
fetch_asset() {
  local asset="$1"
  log "下载 ${asset}（${RELEASE}）…"
  fetch_asset_file "$asset" "${TMP}/${asset}" \
    || die "下载失败：${asset}（已尝试 ${SOURCE} 及回退源）
可能原因：网络不通 / Release 尚未发布（滚动发布 proot-latest 由 CI 在 main 更新时自动重建）。
可尝试：MOX_PROOT_SOURCE=github|gitee 强制选端，或 MOX_PROOT_BASE_URL 指向自建镜像。"

  fetch_asset_file "${asset}.sha256" "${TMP}/${asset}.sha256" \
    || die "下载校验文件失败：${asset}.sha256"

  log "校验 sha256…"
  # 校验文件内为 "<hash>  <文件名>"（无路径），cd 到 TMP 后按文件名核对。
  ( cd "$TMP" && sha256sum -c "${asset}.sha256" ) \
    || die "sha256 校验失败，二进制可能被截断或篡改，已中止安装。"
}

# 安装单个二进制到 $BINDIR（$1 = 资产名，$2 = 目标命令名）。
install_bin() {
  local asset="$1" name="$2"
  # install -m 0755：原子落盘 + 可执行位，避免 cp 后忘记 chmod。
  install -m 0755 "${TMP}/${asset}" "${BINDIR}/${name}"
  # 自检 1：是 ELF 可执行（防下载到 HTML 错误页之类）。
  head -c 4 "${BINDIR}/${name}" | grep -q $'\x7fELF' \
    || die "装好的 ${name} 不是 ELF 可执行文件，安装异常。"
  # 自检 2：--help 必须**真的执行成功**，且输出像一份用法文本。
  #
  # 这段历史上有两个方向的 bug，都被真实安装测试撞到过：
  #   · 假阴性：旧版用 `grep -q "moxsh-proot"`，而 distro 的帮助首行是
  #     「proot-distro（moxsh 兼容实现）」——压根不含品牌全名，0 命中 → die，
  #     二进制其实是好的，却报安装失败。拿品牌名做内容断言本来就靠不住。
  #   · 假阳性：旧版 `$(... --help 2>&1 || true)` 把退出码吞了；二进制因
  #     GLIBC 版本跑不起来时，加载器报错行恰好以 "mox-proot:" 开头，grep
  #     又把报错当成 help 输出认，于是「装成功」但一跑就崩。
  #
  # 现在：① 先判退出码（非 0 直接失败，能抓住动态链接失败）；
  #       ② 内容只断言「非空 + 含用法/Usage/proot 这类稳定词」，与品牌名解耦；
  #       ③ 失败时把真实输出回显，方便一眼定位。
  local help_out help_rc
  if help_out="$("${BINDIR}/${name}" --help 2>&1)"; then
    help_rc=0
  else
    help_rc=$?
  fi
  if [ "$help_rc" -ne 0 ] || [ -z "$help_out" ]; then
    die "自检失败：${name} --help 执行失败（退出码 ${help_rc}）。
  实际输出：${help_out:-（空）}
  常见原因：系统 glibc 低于该二进制的构建基线，或下载到损坏/错架构文件。
  可尝试强制走零依赖 musl 静态版（整行复制执行）：
  curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-mox-proot.sh | MOX_PROOT_FORCE_MUSL=1 sh"
  fi
  if ! printf '%s\n' "$help_out" | grep -Eq '用法[:：]|Usage|proot-distro|proot '; then
    die "自检失败：${name} --help 输出不像用法文本（前 3 行）：
$(printf '%s\n' "$help_out" | head -3)"
  fi
}

# ---------------------------------------------------------------- 主体安装
fetch_asset "mox-proot-${ASSET_SUFFIX}"
install_bin "mox-proot-${ASSET_SUFFIX}" "mox-proot"
log "安装完成：${BINDIR}/mox-proot"

# distro 管理二进制（rootfs 安装/登录/备份；与引擎同一套 ptrace 核心）。
fetch_asset "mox-proot-distro-${ASSET_SUFFIX}"
install_bin "mox-proot-distro-${ASSET_SUFFIX}" "mox-proot-distro"
log "安装完成：${BINDIR}/mox-proot-distro"

# ---------------------------------------------------------------- TUI 控制台（可选资产）
# 2026-11-08 真机实测：Release 目前只为 linux-amd64 / linux-amd64-musl 发布 TUI 资产，
# Android / arm64 尚无。绝不能因为缺 TUI 让整个安装 die（那比没有 TUI 更糟）。
# 先用 HEAD 快速探活，存在才下载 + 校验 sha256；任何失败仅警告，命令版照常可用。
TUI_ASSET="mox-proot-tui-${ASSET_SUFFIX}"
TUI_READY=0
if curl -fsSIL --connect-timeout 6 --max-time 12 "${BASE_URL}/${TUI_ASSET}.sha256" >/dev/null 2>&1; then
  if fetch_asset_file "$TUI_ASSET" "${TMP}/${TUI_ASSET}" && \
     fetch_asset_file "${TUI_ASSET}.sha256" "${TMP}/${TUI_ASSET}.sha256" && \
     ( cd "$TMP" && sha256sum -c "${TUI_ASSET}.sha256" ) >/dev/null 2>&1; then
    if install -m 0755 "${TMP}/${TUI_ASSET}" "${BINDIR}/mox-proot-tui"; then
      TUI_READY=1
    fi
  fi
fi
if [ "$TUI_READY" = 1 ]; then
  log "TUI 控制台就绪：${BINDIR}/mox-proot-tui（无参 mox / mox-proot tui 可进）"
else
  warn "本平台暂无 TUI 控制台资产（${TUI_ASSET}），已跳过；命令版不受影响"
fi

# ---------------------------------------------------------------- 控制台入口
# 容器内固定命令 mox（roadmap §3：不开放自定义；与引擎 console::CONSOLE_SCRIPT
# 同源——bind-mount 反穿透到宿主 MOX_ROOT 执行管理子命令）。
if cat > "${BINDIR}/mox" <<'MOXEOF'
#!/bin/sh
# mox-proot 容器内控制台（固定命令 mox，不开放自定义——roadmap §3）。
# 由引擎/安装器自动生成；删除本文件不影响宿主侧其他命令。
unset LD_PRELOAD

# ── 惊喜体验三件套（时间上下文感知 / 古诗 / 彩蛋，#450）──
# 零依赖 shell 自包含实现：即使 distro 二进制异常，控制台也能照常问候。
__mox_phase() {
  h=$(date +%H 2>/dev/null | sed 's/^0//')
  case "$h" in
    5|6|7|8|9|10) echo morning ;;
    11|12|13|14|15|16) echo noon ;;
    17|18|19|20|21|22) echo evening ;;
    *) echo night ;;
  esac
}
__mox_poem() {
  case "$1" in
    morning) echo "晨起开门雪满山，雪晴云淡日光寒。 ——郑燮《山中雪后》" ;;
    noon) echo "锄禾日当午，汗滴禾下土。 ——李绅《悯农》" ;;
    evening) echo "夕阳无限好，只是近黄昏。 ——李商隐《登乐游原》" ;;
    night) echo "春眠不觉晓，处处闻啼鸟。 ——孟浩然《春晓》" ;;
  esac
}
__mox_greet() {
  case "$(__mox_phase)" in
    morning) echo "早安，新的一天从容器里开始 ☀" ;;
    noon) echo "午安，喝口水再战 ☕" ;;
    evening) echo "晚安，夜色正好适合折腾 🌙" ;;
    night) echo "夜深了，注意身体，别熬太晚 🌃" ;;
  esac
  echo "  $(__mox_poem "$(__mox_phase)")"
}
__mox_egg() {
  echo "🥚 彩蛋：你找到了隐藏菜单！"
  echo "   mox-proot 的每一行代码都是手写对齐上游 PRoot 行为——没有抄。"
  echo "   试试无参 mox 进入，看时间送你的那句诗。"
}
# 彩蛋暗号（先于一切，命中即退出；不进入管理控制台）。
case "${1:-}" in
  egg|彩蛋|konami|蛋)
    __mox_egg
    exit 0
    ;;
esac

if [ -n "${MOX_CONTAINER:-}" ]; then
  # 容器内：反穿透到宿主真实 MOX_ROOT（bind-mount 挂载点）。
  case "${1:-}" in
    login|shell|launch)
      echo "mox: 已在容器内，不能嵌套进入其他容器（ptrace 不支持套娃）。" >&2
      echo "mox: 请先退出当前容器，在宿主侧执行：mox $*" >&2
      exit 1
      ;;
  esac
  ROOT=/mox-proot-root
else
  # 宿主侧直接跑：普通 distro CLI 包装。
  ROOT="${MOX_ROOT:-$HOME/.mox-proot}"
fi
# 进入即画品牌字标 + 时间上下文问候 + 古诗（无参数交互入口时）。
if [ "$#" -eq 0 ]; then
cat <<'MOX_BANNER'
_  _ ____ _  _
|\/| |  |  \/
|  | |__| _/\_
  mox-proot
MOX_BANNER
  __mox_greet
  # 装了 TUI 且当前是交互终端 → 无参 mox 直接进图形控制台。
  # 无 TTY（脚本 / CI / ssh -c）时回退命令版，绝不硬崩、绝不 os error 11。
  if [ -x "$(dirname "$0")/mox-proot-tui" ] && [ -t 0 ] && [ -t 1 ]; then
    exec "$(dirname "$0")/mox-proot-tui" "$@"
  fi
fi
exec env MOX_ROOT="$ROOT" "$(dirname "$0")/mox-proot-distro" "$@"
MOXEOF
then
  chmod 0755 "${BINDIR}/mox"
  log "控制台入口就绪：${BINDIR}/mox（进容器后固定命令，管理全部容器）"
else
  warn "mox 脚本写入失败——容器内控制台不可用，其余功能不受影响"
fi

# ---------------------------------------------------------------- 钩子种子
# 从仓库 tarball 提取 py/runtime + lua/ 到 $MOX_ROOT/hooks（引擎
# post-install / pre-login 钩子的默认内容）。失败仅警告——钩子是增强项。
# custom 源无 ARCHIVE_URL（用户镜像通常只放二进制），跳过种子。
if [ -n "$ARCHIVE_URL" ]; then
  log "种子钩子目录 ${MOX_ROOT}/hooks …"
  if curl -fSL --retry 2 --connect-timeout 15 -o "${TMP}/repo.tar.gz" "${ARCHIVE_URL}" 2>/dev/null; then
    mkdir -p "${MOX_ROOT}/hooks"
    # 顶层目录名两端不同（GitHub: <repo>-<release>/；Gitee: <repo>/），
    # 用 --wildcards '*/...' 通配兼容；--strip-components=1 去掉顶层。
    if tar -xzf "${TMP}/repo.tar.gz" -C "${TMP}" --strip-components=1 --wildcards \
        '*/py/runtime/provision.py' \
        '*/py/runtime/login_hook.py' \
        '*/lua/proot.lua' \
        '*/lua/hooks/post-install.lua' \
        '*/lua/hooks/pre-login.lua' 2>/dev/null; then
    # 钩子必须直接落在 hooks/ 根下：引擎 script.rs::discover_hooks 用
    # fs::read_dir 只扫一层、不递归，放进 hooks/lua/ 反而永远加载不到。
    # 旧版这里 `mkdir -p hooks/lua` 又立刻 `rmdir`（建了空目录再删），
    # 属于无意义操作且注释与行为不符，已移除。
    # 【P0 修复】升级安装不得静默覆盖用户手改过的钩子：旧实现直接 cp，
    # 用户在 ~/.mox-proot/hooks/ 里改过的 post-install / pre-login 会被种子版悄悄换掉。
    # 改为「已存在则保留」；不用 cp -n（busybox/Termux 不保证支持），自己判 -f 更可移植。
    seed_hook() {
      for s in "$@"; do
        [ -f "$s" ] || continue
        b="$(basename "$s")"
        d="${MOX_ROOT}/hooks/${b}"
        if [ -f "$d" ]; then
          log "保留已有钩子 ${b}（不覆盖用户修改）"
          continue
        fi
        cp "$s" "$d" 2>/dev/null || warn "钩子种子写入失败：${b}"
      done
    }
    seed_hook "${TMP}/py/runtime/provision.py" "${TMP}/py/runtime/login_hook.py" \
              "${TMP}/lua/proot.lua" \
              "${TMP}/lua/hooks/post-install.lua" "${TMP}/lua/hooks/pre-login.lua"
    log "钩子种子完成：${MOX_ROOT}/hooks（post-install / pre-login 生命周期）"
  else
    warn "tarball 内未找到钩子文件，跳过种子——引擎无钩子照常工作。"
  fi
else
  warn "钩子 tarball 下载失败（${ARCHIVE_URL}），跳过种子——引擎无钩子照常工作。"
fi
else
  log "自定义镜像源，跳过钩子种子（镜像通常只放二进制）。"
fi

# 反馈链接按源切换（Gitee 源的用户去 Gitee 提 issue）。
if [ "$SOURCE" = "gitee" ]; then
  ISSUES_URL="https://gitee.com/${GITEE_REPO}/issues"
else
  ISSUES_URL="https://github.com/${GITHUB_REPO}/issues"
fi

# ---------------------------------------------------------------- 收尾自检
# 逐项确认三个命令都在且可执行 + 打印版本，装完当场自证（T5 精修）。
log "收尾自检：核对已安装命令…"
SELF_OK=1
for chk in mox-proot mox-proot-distro mox; do
  if [ -x "${BINDIR}/${chk}" ]; then
    ok "${BINDIR}/${chk} 就绪"
  else
    warn "${BINDIR}/${chk} 缺失或不可执行"
    SELF_OK=0
  fi
done
# TUI 可选：装了就必须能跑 --version（旧的动态 musl 资产启动即 SIGSEGV，
# 这条自检会在安装当场拦住坏二进制，而不是等用户敲 mox 才发现）。
if [ -x "${BINDIR}/mox-proot-tui" ]; then
  if TUI_VER="$("${BINDIR}/mox-proot-tui" --version 2>&1)" && [ -n "$TUI_VER" ]; then
    ok "mox-proot-tui 就绪（${TUI_VER}）"
  else
    warn "mox-proot-tui --version 自检失败，TUI 资产可能损坏"
    SELF_OK=0
  fi
fi
if [ "$SELF_OK" = 1 ]; then
  # 【P1 修复】引擎主程序现已实现 --version/-V 且 rc=0；旧实现用 `|| true`
  # 吞掉退出码，rc=2 也会报「自检通过」，安装收尾一直挂假绿警告。
  if VER_OUT="$("${BINDIR}/mox-proot" --version 2>&1)" && [ -n "$VER_OUT" ]; then
    ok "引擎自检通过：${VER_OUT}"
  else
    warn "引擎 --version 自检失败（退出码非 0 或无输出），建议执行 mox-proot --help 排查"
  fi
else
  warn "自检发现缺项，其余功能可能不受影响；建议重跑安装脚本"
fi

cat <<EOF

────────── 先跑这三步，确认装好了 ──────────
  mox-proot-distro install alpine                      # 最小镜像，几十秒就好
  mox-proot-distro login alpine -- cat /etc/os-release
  mox                                                  # 进容器后敲它，出管理台

上面两行能跑通就说明引擎、rootfs、路径翻译全部就绪。
（想装大发行版把 alpine 换成 ubuntu/kali 即可；首次下载会久一点，不是卡死。）

────────── 更进一步的命令（用到再查）──────────
  mox-proot -R ~/ubuntu -0 -- bash       # 原生引擎：推荐绑定集 + fake root
  mox-proot --help                       # 完整用法 / 默认绑定集说明
  mox-proot-distro backup <id>           # 备份（打印归档绝对路径与大小）
  mox-proot-distro restore <id> <归档>   # 恢复（打印恢复到哪一天的备份）
  mox-proot-distro versions ubuntu       # 列出可选版本
  mox-proot-distro time / mox egg        # 时相问候 + 古诗 / 彩蛋
  mox-proot-installer update             # Go 安装器：richer 更新体验
                                         # 下载：https://github.com/codecloud-dev/moxsh-proot/releases/tag/proot-latest

────────── 付费配方显示 🔒 时 ──────────
  访问 https://mox-site.pages.dev/fan 兑换，或容器内执行 mox redeem login 授权。

钩子（安装/登录时自动执行，可编辑；升级安装不会覆盖你改过的文件）：
  ${MOX_ROOT}/hooks/provision.py        # 装完后供给检查（DNS/hosts）
  ${MOX_ROOT}/hooks/login_hook.py       # 登录时环境快照
  ${MOX_ROOT}/hooks/proot.lua           # 声明式配置（motd / DNS 偏好）
  ${MOX_ROOT}/hooks/post-install.lua    # 安装后生命周期钩子
  ${MOX_ROOT}/hooks/pre-login.lua       # 登录前生命周期钩子

有问题反馈：${ISSUES_URL}
EOF
