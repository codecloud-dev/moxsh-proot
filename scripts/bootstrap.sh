#!/bin/sh
# moxsh-proot 微型引导（bootstrap）——base64 内嵌进「一键安装」那条短命令里。
#
# 职责边界（roadmap §5）：本脚本只做「检查 curl → 拉全量安装器 → exec 交棒」，
# **不重复实现安装逻辑**。全量脚本永远从 main 拉，天然最新，无需自更新；
# 国内/国外下载源分流由全量安装器（scripts/install-moxsh-proot.sh）自行探测，
# 本层只管「GitHub raw 不通就回退 Gitee raw」把安装器本身拿到手。
#
# 为什么是两层：内嵌全量脚本 base64 后约 10KB，并不短；内嵌本引导
# （gzip+base64 后不到 1KB）解码即拉全量——命令短，且逻辑永远最新。
#
# 本文件由 scripts/gen-shortcmd.sh 生成为内嵌命令，修改后必须重新生成同步。

set -u

GH="https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-moxsh-proot.sh"
GT="https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-moxsh-proot.sh"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

command -v curl >/dev/null 2>&1 || die "未找到 curl（Termux 里执行: pkg install curl）"

TMP="${TMPDIR:-/tmp}/moxsh-installer.$$"
trap 'rm -f "$TMP"' EXIT

say "拉取安装器…"
for U in "$GH" "$GT"; do
  if curl -fsSL --connect-timeout 10 --max-time 180 "$U" -o "$TMP" 2>/dev/null && [ -s "$TMP" ]; then
    say "交棒全量安装器（依赖检查 / 选源下载 / 校验 / 落盘）…"
    exec sh "$TMP"
  fi
done

die "两个下载源（GitHub / Gitee）都拉取失败，请检查网络后重试"
