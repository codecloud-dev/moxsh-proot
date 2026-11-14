#!/usr/bin/env bash
# 真实函数单元测：source 安装脚本（MOX_PROOT_UNIT=1 只暴露函数，不跑网络探测），
# 用受控 PATH 验证 check_runtime_deps 在缺 tar / 缺 xz 时给出「人话 + 怎么办」错误，
# 在依赖齐全时返回 0。直接吃脚本里的真函数体，不复制逻辑（避免假绿）。
set -u
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
SCRIPT="$SCRIPT_DIR/install-mox-proot.sh"
pass=0; fail=0

# 0) 语法检查
if bash -n "$SCRIPT" 2>/tmp/deps_syntax.err; then
  echo "PASS: bash -n 语法 OK"; pass=$((pass+1))
else
  echo "FAIL: 语法错误"; cat /tmp/deps_syntax.err; fail=$((fail+1))
fi

# 造一个只含指定命令的临时 bin 目录（用真实二进制做软链）
mkbin() {
  local d; d="$(mktemp -d)"
  # 把 bash 自己也放进去，使受控 PATH="$d" 仍能启动子 shell。
  local b; b="$(command -v bash 2>/dev/null)" && ln -sf "$b" "$d/bash"
  for t in "$@"; do
    local p; p="$(command -v "$t" 2>/dev/null)" || { echo "环境缺 $t，跳过"; rm -rf "$d"; return 1; }
    ln -sf "$p" "$d/$t"
  done
  echo "$d"
}

# 1) 缺 xz（但有 curl/sha256sum/tar）→ 必须失败且提示 xz + 各平台装法
d="$(mkbin curl sha256sum tar)" || true
if [ -n "$d" ]; then
  out="$(MOX_PROOT_UNIT=1 PATH="$d" bash -c "source '$SCRIPT'; check_runtime_deps" 2>&1)"; rc=$?
  if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q "xz"; then
    echo "PASS: 缺 xz 报错并给安装指引 (rc=$rc)"; pass=$((pass+1))
  else
    echo "FAIL: 缺 xz 未正确拦截 (rc=$rc) out=$out"; fail=$((fail+1))
  fi
  rm -rf "$d"
fi

# 2) 缺 tar（但有 curl/sha256sum/xz）→ 必须失败且提示 tar
xzbin="$(command -v xz 2>/dev/null || command -v unxz 2>/dev/null)"
if [ -n "$xzbin" ]; then
  d="$(mkbin curl sha256sum)"; ln -sf "$xzbin" "$d/xz"
  if [ -n "$d" ]; then
    out="$(MOX_PROOT_UNIT=1 PATH="$d" bash -c "source '$SCRIPT'; check_runtime_deps" 2>&1)"; rc=$?
    if [ $rc -ne 0 ] && printf '%s' "$out" | grep -q "tar"; then
      echo "PASS: 缺 tar 报错并给安装指引 (rc=$rc)"; pass=$((pass+1))
    else
      echo "FAIL: 缺 tar 未正确拦截 (rc=$rc) out=$out"; fail=$((fail+1))
    fi
    rm -rf "$d"
  fi
fi

# 3) 依赖齐全（curl/sha256sum/tar/xz）→ 返回 0
xzbin="$(command -v xz 2>/dev/null || command -v unxz 2>/dev/null)"
if [ -n "$xzbin" ]; then
  d="$(mkbin curl sha256sum tar)"; ln -sf "$xzbin" "$d/xz"
  if [ -n "$d" ]; then
    out="$(MOX_PROOT_UNIT=1 PATH="$d" bash -c "source '$SCRIPT'; check_runtime_deps" 2>&1)"; rc=$?
    if [ $rc -eq 0 ]; then
      echo "PASS: 依赖齐全返回 0"; pass=$((pass+1))
    else
      echo "FAIL: 依赖齐全却报错 (rc=$rc) out=$out"; fail=$((fail+1))
    fi
    rm -rf "$d"
  fi
fi

echo "────────────"
echo "依赖自检单测：通过 $pass / 失败 $fail"
[ $fail -eq 0 ]
