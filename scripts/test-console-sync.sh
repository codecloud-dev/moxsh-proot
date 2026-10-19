#!/usr/bin/env bash
# 校验 mox 脚本两端一致：安装器 heredoc ↔ 引擎 console::CONSOLE_SCRIPT。
# 安装器（sh）与引擎（Rust）各写一份同样内容，CI 盯防漂移（roadmap §3）。
set -euo pipefail
cd "$(dirname "$0")/.."

tmp_i="$(mktemp)"
tmp_e="$(mktemp)"
trap 'rm -f "$tmp_i" "$tmp_e"' EXIT

# 1) 安装器 heredoc 内容（从 cat 指令行之后到 MOXEOF 结束标记之前）。
awk '/^if cat > .*mox.* <<.MOXEOF.$/{f=1;next} /^MOXEOF$/{f=0} f' \
  scripts/install-moxsh-proot.sh > "$tmp_i"

# 2) 引擎 raw string 常量内容（第一行剥掉 r#" 前缀，末行 "#; 删除）。
sed -n '/pub const CONSOLE_SCRIPT: &str = r#"/,/^"#;/p' \
  src/proot/console.rs | sed '1s/^.*r#"//; $d' > "$tmp_e"

if [ ! -s "$tmp_i" ]; then
  echo "FAIL: 未能从 scripts/install-moxsh-proot.sh 提取 mox heredoc" >&2
  exit 1
fi
if [ ! -s "$tmp_e" ]; then
  echo "FAIL: 未能从 src/proot/console.rs 提取 CONSOLE_SCRIPT" >&2
  exit 1
fi

if diff -u "$tmp_e" "$tmp_i"; then
  echo "OK: mox 脚本两端一致（安装器 heredoc ↔ console::CONSOLE_SCRIPT）"
else
  echo "FAIL: mox 脚本两边漂移（见上方 diff：引擎 vs 安装器）" >&2
  exit 1
fi
