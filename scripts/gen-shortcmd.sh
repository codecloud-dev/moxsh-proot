#!/usr/bin/env bash
# 生成 base64 内嵌短命令（README / 官网 / 文档用，roadmap §5）。
#
# 形态：`echo '<b64>' | base64 -d | gzip -d | sh`
#   - 内嵌的是 scripts/bootstrap.sh（微型引导，gzip+base64 后约 1KB）；
#   - 引导负责选线路 + 拉全量安装器，逻辑永远最新（从 main 拉）；
#   - Termux（coreutils）/ 桌面 Linux / busybox 均有 base64+gzip。
#
# 改了 bootstrap.sh 后必须重跑本脚本并同步 README / 官网命令框。
# --check：不生成，只校验 README 中的短命令与当前 bootstrap.sh 一致（CI 用）。
set -euo pipefail
cd "$(dirname "$0")/.."

B64="$(gzip -9c scripts/bootstrap.sh | base64 -w0)"

# 自校验：解码回来必须与源文件逐字节一致（防管道/换行损坏）。
decoded="$(printf '%s' "$B64" | base64 -d | gzip -dc)"
if [ "$decoded" != "$(cat scripts/bootstrap.sh)" ]; then
  echo "自校验失败：解码结果与 scripts/bootstrap.sh 不一致" >&2
  exit 1
fi

if [ "${1:-}" = "--check" ]; then
  cmd="echo '${B64}' | base64 -d | gzip -d | sh"
  rc=0
  for f in README.md README.en.md; do
    if ! grep -qF "$cmd" "$f"; then
      echo "不同步：$f 里的短命令与 scripts/bootstrap.sh 对不上，请重跑 scripts/gen-shortcmd.sh 并更新文档" >&2
      rc=1
    fi
  done
  [ "$rc" -eq 0 ] && echo "OK: README 短命令与 bootstrap.sh 同步"
  exit "$rc"
fi

echo "echo '${B64}' | base64 -d | gzip -d | sh"
