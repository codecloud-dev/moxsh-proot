#!/usr/bin/env bash
# moxsh-proot 一键安装脚本（原版 Termux / Linux 用户专用）。
#
# 把 ProOT 容器引擎（Rust ptrace 路径翻译，非上游 C 版 proot 的封装）
# 装成 `moxsh-proot` 命令，之后即可：
#
#   moxsh-proot -R ~/ubuntu -0 -- bash
#
# 安装方式（原版 Termux，二选一，脚本自动探测国内外线路）：
#   curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-moxsh-proot.sh | bash
#   curl -fsSL https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-moxsh-proot.sh | bash
#
# 本脚本是安装体系的 **bash 探测层**（零依赖、可 curl|bash）；
# richer 安装/更新体验走 Go 安装器（仓库 go/ 目录，Release 资产
# `moxsh-installer-<goos>-<arch>`，子命令 install/update/probe）：
#   moxsh-installer install
# 两层默认下载源一致，均为本仓库 Release。
#
# 设计约束（Termux 现实）：
#   - 只依赖 Termux 自带工具：curl / coreutils（sha256sum、install、mktemp）；
#   - 二进制从 GitHub Release 下载并做 sha256 校验（放不下就不装，绝不静默）；
#   - 非交互：`curl | bash` 下 stdin 是脚本本身，全程无需任何输入；
#   - PREFIX 未设置时按 Termux 常见路径探测，探测不到则明确报错（不猜 system 路径，
#     Android 10+ 的 W^X 会让 system 分区上的可执行文件根本跑不起来）；
#   - 钩子种子（py/runtime + lua/）随仓库 tarball 一并落盘到 $MOXSH_ROOT/hooks，
#     下载失败不阻断主安装（引擎无钩子照常工作）。

set -euo pipefail

GITHUB_REPO="${MOXSH_PROOT_REPO:-codecloud-dev/moxsh-proot}"
GITEE_REPO="${MOXSH_GITEE_REPO:-thousand-code-crafts/moxsh-proot}"
RELEASE="${MOXSH_PROOT_RELEASE:-proot-latest}"

log()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m警告:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------- 国内外分流
# 自动选源（0.2.0）：探测 GitHub 与 Gitee 的可达性 + 握手延迟，国内走
# Gitee、国外走 GitHub。覆盖方式（优先级从高到低）：
#   1. MOXSH_PROOT_BASE_URL 显式指定（镜像自建场景，需保持
#      <BASE_URL>/<ASSET> 与 <BASE_URL>/<ASSET>.sha256 布局）；
#   2. MOXSH_PROOT_SOURCE=github|gitee 强制选端（不猜 URL）；
#   3. 自动探测（默认）。
#
# 探测端点选轻量 raw README（几十 KB，不计速只计可达与握手耗时）；
# 两端都不可达时默认 GitHub——错误信息指向标准源，且瞬时抖动下
# GitHub 的全球可达性通常更好。
probe_seconds() {
  # $1 = URL；成功输出耗时（秒），失败输出空。
  curl -fsS -o /dev/null --connect-timeout 4 --max-time 8 -w '%{time_total}' "$1" 2>/dev/null || true
}

pick_source() {
  if [ -n "${MOXSH_PROOT_BASE_URL:-}" ]; then
    SOURCE="custom"
    BASE_URL="$MOXSH_PROOT_BASE_URL"
    ARCHIVE_URL=""
    log "下载源：自定义镜像 ${BASE_URL}"
    return
  fi
  if [ "${MOXSH_PROOT_SOURCE:-auto}" = "github" ]; then
    set_source_github; return
  fi
  if [ "${MOXSH_PROOT_SOURCE:-auto}" = "gitee" ]; then
    set_source_gitee; return
  fi
  local gh_s gitee_s
  gh_s="$(probe_seconds "https://raw.githubusercontent.com/${GITHUB_REPO}/main/README.md")"
  gitee_s="$(probe_seconds "https://gitee.com/${GITEE_REPO}/raw/main/README.md")"
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
# PREFIX：Termux 会话必然设置；裸跑（如 ssh 进 Termux）时按已知路径兜底。
PREFIX="${PREFIX:-}"
if [ -z "$PREFIX" ]; then
  for c in \
    /data/data/com.termux/files/usr \
    /data/data/com.termux.dev/files/usr
  do
    if [ -d "$c" ]; then PREFIX="$c"; break; fi
  done
fi
[ -n "$PREFIX" ] || die "未检测到 Termux 环境（PREFIX 未设置）。
本脚本面向原版 Termux。若你在其他环境，请显式指定安装目录：
  PREFIX=/your/prefix bash install-moxsh-proot.sh"

# 架构：moxsh-proot 提供 aarch64（主流设备）与 x86_64（模拟器/Chromebook）两版。
case "$(uname -m)" in
  aarch64|arm64)  TRIPLE_ARCH="aarch64" ;;
  x86_64|amd64)   TRIPLE_ARCH="x86_64" ;;
  armv7l|armv8l|arm)
    die "检测到 32 位 ARM（$(uname -m)），暂不提供预编译二进制。
请从源码构建：https://github.com/${GITHUB_REPO}#从源码构建" ;;
  *) die "不支持的架构: $(uname -m)" ;;
esac

BINDIR="${PREFIX}/bin"
[ -d "$BINDIR" ] || mkdir -p "$BINDIR"
[ -w "$BINDIR" ] || die "${BINDIR} 不可写（PREFIX 是否正确？）"

command -v curl >/dev/null 2>&1 || die "未找到 curl（pkg install curl）"
command -v sha256sum >/dev/null 2>&1 || die "未找到 sha256sum（pkg install coreutils）"

# 选源（探测 GitHub/Gitee 延迟；需 curl 就位后执行）。
pick_source

# MOXSH_ROOT：钩子与容器数据根目录（对齐引擎 DistroManager 约定：
# $MOXSH_ROOT > $HOME/.moxsh）。
MOXSH_ROOT="${MOXSH_ROOT:-}"
if [ -z "$MOXSH_ROOT" ]; then
  HOME_DIR="${HOME:-}"
  [ -n "$HOME_DIR" ] || die "HOME 未设置，无法推断 MOXSH_ROOT（请显式设置）"
  MOXSH_ROOT="${HOME_DIR}/.moxsh"
fi

# ---------------------------------------------------------------- 下载 + 校验
TMP="$(mktemp -d "${TMPDIR:-/tmp}/moxsh-proot.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

# 带一次切源重试的资产下载（$1 = 资产名，$2 = 输出路径）。
# 当前源缺资产（404）/ 不通时，自动回退另一端重试一次（见 switch_source）。
fetch_asset_file() {
  local asset="$1" out="$2"
  if curl -fSL --retry 3 --connect-timeout 15 -o "$out" "${BASE_URL}/${asset}"; then
    return 0
  fi
  if switch_source; then
    curl -fSL --retry 3 --connect-timeout 15 -o "$out" "${BASE_URL}/${asset}"
    return $?
  fi
  return 1
}

# 下载并校验单个资产（$1 = 资产名）；失败即 die（"放不下就不装"）。
fetch_asset() {
  local asset="$1"
  log "下载 ${asset}（${RELEASE}）…"
  fetch_asset_file "$asset" "${TMP}/${asset}" \
    || die "下载失败：${asset}（已尝试 ${SOURCE} 及回退源）
可能原因：网络不通 / Release 尚未发布（滚动发布 proot-latest 由 CI 在 main 更新时自动重建）。
可尝试：MOXSH_PROOT_SOURCE=github|gitee 强制选端，或 MOXSH_PROOT_BASE_URL 指向自建镜像。"

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
  # 自检 2：--help 真实输出用法文本。先捕获再判（不能靠退出码管道——
  #   本脚本 set -o pipefail，且不同版本 --help 的退出码约定可能变化）。
  local help_out
  help_out="$("${BINDIR}/${name}" --help 2>&1 || true)"
  printf '%s\n' "$help_out" | grep -q "moxsh-proot" \
    || die "自检失败：${name} --help 未输出预期用法文本。"
}

# ---------------------------------------------------------------- 主体安装
fetch_asset "moxsh-proot-${TRIPLE_ARCH}-linux-android"
install_bin "moxsh-proot-${TRIPLE_ARCH}-linux-android" "moxsh-proot"
log "安装完成：${BINDIR}/moxsh-proot"

# distro 管理二进制（rootfs 安装/登录/备份；与引擎同一套 ptrace 核心）。
fetch_asset "moxsh-proot-distro-${TRIPLE_ARCH}-linux-android"
install_bin "moxsh-proot-distro-${TRIPLE_ARCH}-linux-android" "moxsh-proot-distro"
log "安装完成：${BINDIR}/moxsh-proot-distro"

# ---------------------------------------------------------------- 控制台入口
# 容器内固定命令 mox（roadmap §3：不开放自定义；与引擎 console::CONSOLE_SCRIPT
# 同源——bind-mount 反穿透到宿主 MOXSH_ROOT 执行管理子命令）。
if cat > "${BINDIR}/mox" <<'MOXEOF'
#!/bin/sh
# moxsh 容器内控制台（固定命令 mox，不开放自定义——roadmap §3）。
# 由引擎/安装器自动生成；删除本文件不影响宿主侧其他命令。
unset LD_PRELOAD
if [ -n "${MOXSH_CONTAINER:-}" ]; then
  # 容器内：反穿透到宿主真实 MOXSH_ROOT（bind-mount 挂载点）。
  case "${1:-}" in
    login|shell|launch)
      echo "mox: 已在容器内，不能嵌套进入其他容器（ptrace 不支持套娃）。" >&2
      echo "mox: 请先退出当前容器，在宿主侧执行：mox $*" >&2
      exit 1
      ;;
  esac
  ROOT=/moxsh-root
else
  # 宿主侧直接跑：普通 distro CLI 包装。
  ROOT="${MOXSH_ROOT:-$HOME/.moxsh}"
fi
exec env MOXSH_ROOT="$ROOT" "$(dirname "$0")/moxsh-proot-distro" "$@"
MOXEOF
then
  chmod 0755 "${BINDIR}/mox"
  log "控制台入口就绪：${BINDIR}/mox（进容器后固定命令，管理全部容器）"
else
  warn "mox 脚本写入失败——容器内控制台不可用，其余功能不受影响"
fi

# ---------------------------------------------------------------- 钩子种子
# 从仓库 tarball 提取 py/runtime + lua/ 到 $MOXSH_ROOT/hooks（引擎
# post-install / pre-login 钩子的默认内容）。失败仅警告——钩子是增强项。
# custom 源无 ARCHIVE_URL（用户镜像通常只放二进制），跳过种子。
if [ -n "$ARCHIVE_URL" ]; then
  log "种子钩子目录 ${MOXSH_ROOT}/hooks …"
  if curl -fSL --retry 2 --connect-timeout 15 -o "${TMP}/repo.tar.gz" "${ARCHIVE_URL}" 2>/dev/null; then
    mkdir -p "${MOXSH_ROOT}/hooks"
    # 顶层目录名两端不同（GitHub: <repo>-<release>/；Gitee: <repo>/），
    # 用 --wildcards '*/...' 通配兼容；--strip-components=1 去掉顶层。
    if tar -xzf "${TMP}/repo.tar.gz" -C "${TMP}" --strip-components=1 --wildcards \
        '*/py/runtime/provision.py' \
        '*/py/runtime/login_hook.py' \
        '*/lua/proot.lua' \
        '*/lua/hooks/post-install.lua' \
        '*/lua/hooks/pre-login.lua' 2>/dev/null; then
    cp "${TMP}/py/runtime/provision.py" "${TMP}/py/runtime/login_hook.py" \
       "${MOXSH_ROOT}/hooks/" 2>/dev/null || true
    mkdir -p "${MOXSH_ROOT}/hooks/lua"
    cp "${TMP}/lua/proot.lua" "${MOXSH_ROOT}/hooks/" 2>/dev/null || true
    cp "${TMP}/lua/hooks/post-install.lua" "${TMP}/lua/hooks/pre-login.lua" \
       "${MOXSH_ROOT}/hooks/" 2>/dev/null || true
    rmdir "${MOXSH_ROOT}/hooks/lua" 2>/dev/null || true
    log "钩子种子完成：${MOXSH_ROOT}/hooks（post-install / pre-login 生命周期）"
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

cat <<EOF

用法速查（对齐上游 proot 常用选项）：
  moxsh-proot -R ~/ubuntu -0 -- bash          # 推荐绑定集 + fake root 进发行版
  moxsh-proot -r ~/ubuntu -w /root -- sh      # 最小模式，指定初始目录
  moxsh-proot -S ~/ubuntu -0 -- apt update    # 装包场景（最小绑定集）
  moxsh-proot --help                          # 完整用法

rootfs 未安装？用 distro 管理器装一个（引擎同一套）：
  moxsh-proot-distro install ubuntu
  moxsh-proot-distro login ubuntu

容器内控制台（固定命令，进任意容器后直接输入）：
  mox                     # 拉起管理控制台（list/install/remove/net/env）
  mox install kali        # 在容器里再装一个发行版（反穿透到宿主）

钩子（安装/登录时自动执行，可编辑）：
  ${MOXSH_ROOT}/hooks/post-install.py     # 装完后供给检查（DNS/hosts）
  ${MOXSH_ROOT}/hooks/pre-login.py        # 登录时环境快照
  ${MOXSH_ROOT}/hooks/proot.lua           # 声明式配置（motd / DNS 偏好）

粉丝专属（灰度：MOXSH_FAN_ENABLE=1；入口命令不随安装泄露）：
  mox fan login                       # 起本机回环 → 唤起浏览器 → 登录 → 确认领取
                                       #   → 激活码自动跳回回环，联网校验后解锁
  mox fan redeem <兑换码>              # 桌面 Linux 手动路径（同样联网校验）
  mox fan status                       # 查看粉丝状态（在线校验模式）

 richer 更新体验（Go 安装器，支持 update/probe）：
  moxsh-installer update

有问题反馈：${ISSUES_URL}
EOF
