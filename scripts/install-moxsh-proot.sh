#!/usr/bin/env bash
# moxsh-proot 一键安装脚本（原版 Termux / Linux 用户专用）。
#
# 把 ProOT 容器引擎（Rust ptrace 路径翻译，非上游 C 版 proot 的封装）
# 装成 `moxsh-proot` 命令，之后即可：
#
#   moxsh-proot -R ~/ubuntu -0 -- bash
#
# 安装方式（原版 Termux）：
#   curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-moxsh-proot.sh | bash
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

REPO="${MOXSH_PROOT_REPO:-codecloud-dev/moxsh-proot}"
RELEASE="${MOXSH_PROOT_RELEASE:-proot-latest}"
# 默认走 GitHub Release；国内网络慢时可用 MOXSH_PROOT_BASE_URL 指向镜像
# （镜像需保持相同的 <BASE_URL>/<ASSET> 与 <BASE_URL>/<ASSET>.sha256 布局）。
BASE_URL="${MOXSH_PROOT_BASE_URL:-https://github.com/${REPO}/releases/download/${RELEASE}}"

log()  { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m警告:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

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
请从源码构建：https://github.com/${REPO}#从源码构建" ;;
  *) die "不支持的架构: $(uname -m)" ;;
esac

BINDIR="${PREFIX}/bin"
[ -d "$BINDIR" ] || mkdir -p "$BINDIR"
[ -w "$BINDIR" ] || die "${BINDIR} 不可写（PREFIX 是否正确？）"

command -v curl >/dev/null 2>&1 || die "未找到 curl（pkg install curl）"
command -v sha256sum >/dev/null 2>&1 || die "未找到 sha256sum（pkg install coreutils）"

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

# 下载并校验单个资产（$1 = 资产名）；失败即 die（"放不下就不装"）。
fetch_asset() {
  local asset="$1"
  log "下载 ${asset}（${RELEASE}）…"
  # --retry 3：Termux 移动网络抖动比桌面常见；-f 让 404 等也走失败分支。
  curl -fSL --retry 3 --connect-timeout 15 \
    -o "${TMP}/${asset}" "${BASE_URL}/${asset}" \
    || die "下载失败：${BASE_URL}/${asset}
可能原因：网络不通 / Release 尚未发布（首次发布由 CI 推送 tag proot-v* 触发）。"

  curl -fSL --retry 3 --connect-timeout 15 \
    -o "${TMP}/${asset}.sha256" "${BASE_URL}/${asset}.sha256" \
    || die "下载校验文件失败：${BASE_URL}/${asset}.sha256"

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

# ---------------------------------------------------------------- 钩子种子
# 从仓库 tarball 提取 py/runtime + lua/ 到 $MOXSH_ROOT/hooks（引擎
# post-install / pre-login 钩子的默认内容）。失败仅警告——钩子是增强项。
log "种子钩子目录 ${MOXSH_ROOT}/hooks …"
HOOKS_TARBALL="https://github.com/${REPO}/archive/refs/tags/${RELEASE}.tar.gz"
if curl -fSL --retry 2 --connect-timeout 15 -o "${TMP}/repo.tar.gz" "${HOOKS_TARBALL}" 2>/dev/null; then
  mkdir -p "${MOXSH_ROOT}/hooks"
  # tarball 顶层目录为 <repo>-<release>/；strip 后按相对路径取钩子相关文件。
  if tar -xzf "${TMP}/repo.tar.gz" -C "${TMP}" --strip-components=1 \
      "moxsh-proot-${RELEASE}/py/runtime/provision.py" \
      "moxsh-proot-${RELEASE}/py/runtime/login_hook.py" \
      "moxsh-proot-${RELEASE}/lua/proot.lua" \
      "moxsh-proot-${RELEASE}/lua/hooks/post-install.lua" \
      "moxsh-proot-${RELEASE}/lua/hooks/pre-login.lua" 2>/dev/null; then
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
  warn "钩子 tarball 下载失败（${HOOKS_TARBALL}），跳过种子——引擎无钩子照常工作。"
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

钩子（安装/登录时自动执行，可编辑）：
  ${MOXSH_ROOT}/hooks/post-install.py     # 装完后供给检查（DNS/hosts）
  ${MOXSH_ROOT}/hooks/pre-login.py        # 登录时环境快照
  ${MOXSH_ROOT}/hooks/proot.lua           # 声明式配置（motd / DNS 偏好）

 richer 更新体验（Go 安装器，支持 update/probe）：
  moxsh-installer update

有问题反馈：https://github.com/${REPO}/issues
EOF
