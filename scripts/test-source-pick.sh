#!/usr/bin/env bash
# install-moxsh-proot.sh 国内外分流逻辑回归测试（CI docs-check 调用，无网络）。
#
# 做法：从安装器提取「分流段」（probe_seconds / pick_source / set_source_*），
# 在子 shell 里 mock curl 的探测结果，验证选端决策与 URL 形态。
# 覆盖 7 个用例：国内快 / 国外快 / 国外快但 gitee 慢 / 双端不可达兜底 /
# 强制 gitee / 强制 github / 自定义镜像；另断言两端 archive URL 形态。
set -uo pipefail
cd "$(dirname "$0")/.."

SRC="scripts/install-moxsh-proot.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# 提取分流段：从「国内外分流」注释开始，到 switch_source 函数闭合结束。
awk '/^# -+ 国内外分流/{f=1}
     f{print}
     f&&/^switch_source\(\) \{/{g=1}
     g&&/^}/{exit}' "$SRC" > "$TMP/pick.sh"

if [ ! -s "$TMP/pick.sh" ]; then
  echo "FAIL: 未能从 $SRC 提取分流段（注释锚点变了？）" >&2
  exit 1
fi

# mock_pick <github探测输出> <gitee探测输出> [env...]
# 探测输出为空 = 该端不可达；非空 = 握手耗时（秒）。
mock_pick() {
  local gh="$1" gt="$2"; shift 2
  env "$@" GH_PROBE="$gh" GITEE_PROBE="$gt" \
    GITHUB_REPO="codecloud-dev/moxsh-proot" \
    GITEE_REPO="thousand-code-crafts/moxsh-proot" \
    RELEASE="proot-latest" \
    bash -c '
      curl() {
        local url="" a
        for a in "$@"; do case "$a" in http*) url="$a";; esac; done
        case "$url" in
          *raw.githubusercontent.com*)
            [ -n "$GH_PROBE" ] && { echo "$GH_PROBE"; return 0; }; return 22 ;;
          *gitee.com*)
            [ -n "$GITEE_PROBE" ] && { echo "$GITEE_PROBE"; return 0; }; return 22 ;;
        esac
        return 22
      }
      log() { :; }
      source "'"$TMP"'/pick.sh"
      pick_source >/dev/null 2>&1
      echo "$SOURCE|$BASE_URL|$ARCHIVE_URL"
    '
}

fail=0

# mock_switch <gh探测> <gt探测> <MOXSH_PROOT_SOURCE> <pick后执行的动作> [额外env...]
# 与 mock_pick 同款 mock，但支持在 pick_source 之后执行一段动作（测 switch_source）。
mock_switch() {
  local gh="$1" gt="$2" src="$3" post="$4"; shift 4
  env "$@" MOXSH_PROOT_SOURCE="$src" GH_PROBE="$gh" GITEE_PROBE="$gt" \
    GITHUB_REPO="codecloud-dev/moxsh-proot" \
    GITEE_REPO="thousand-code-crafts/moxsh-proot" \
    RELEASE="proot-latest" \
    bash -c '
      curl() {
        local url="" a
        for a in "$@"; do case "$a" in http*) url="$a";; esac; done
        case "$url" in
          *raw.githubusercontent.com*)
            [ -n "$GH_PROBE" ] && { echo "$GH_PROBE"; return 0; }; return 22 ;;
          *gitee.com*)
            [ -n "$GITEE_PROBE" ] && { echo "$GITEE_PROBE"; return 0; }; return 22 ;;
        esac
        return 22
      }
      log() { :; }
      warn() { :; }
      source "'"$TMP"'/pick.sh"
      pick_source >/dev/null 2>&1
      '"$post"'
      echo "$SOURCE|$BASE_URL|$ARCHIVE_URL"
    '
}

check() {
  local desc="$1" got="$2" want_source="$3" want_url_frag="$4" want_archive_frag="${5:-}"
  local source base archive
  IFS='|' read -r source base archive <<< "$got"
  if [ "$source" != "$want_source" ]; then
    echo "FAIL [$desc]: source=$source 期望 $want_source（完整输出: $got）"; fail=1; return
  fi
  case "$base" in
    *"$want_url_frag"*) ;;
    *) echo "FAIL [$desc]: BASE_URL=$base 不含 $want_url_frag"; fail=1; return ;;
  esac
  if [ -n "$want_archive_frag" ]; then
    case "$archive" in
      *"$want_archive_frag"*) ;;
      *) echo "FAIL [$desc]: ARCHIVE_URL=$archive 不含 $want_archive_frag"; fail=1; return ;;
    esac
  fi
  echo "ok   [$desc] -> $source"
}

# 1) 国内：GitHub 不可达，Gitee 快 → gitee
check "国内（gh 不可达 / gitee 0.30s）" \
  "$(mock_pick "" 0.30)" gitee "gitee.com/thousand-code-crafts/moxsh-proot/releases/download" "/repository/archive/"

# 2) 国外：GitHub 快，Gitee 不可达 → github
check "国外（gh 0.20s / gitee 不可达）" \
  "$(mock_pick 0.20 "")" github "github.com/codecloud-dev/moxsh-proot/releases/download" "/archive/refs/tags/"

# 3) 双端都通但 Gitee 慢 → github（浮点比较走 awk）
check "双通但 gitee 慢（gh 0.20s / gitee 2.50s）" \
  "$(mock_pick 0.20 2.50)" github "github.com/codecloud-dev/moxsh-proot/releases/download"

# 4) 双端都不可达 → 兜底 github（错误信息指向标准源）
check "双不可达兜底" \
  "$(mock_pick "" "")" github "github.com/codecloud-dev/moxsh-proot/releases/download"

# 5) 强制 gitee（即使 GitHub 更快也不探测）
check "强制 gitee" \
  "$(mock_pick 0.10 5.00 MOXSH_PROOT_SOURCE=gitee)" gitee "gitee.com/thousand-code-crafts/moxsh-proot/releases/download"

# 6) 强制 github
check "强制 github" \
  "$(mock_pick "" 0.10 MOXSH_PROOT_SOURCE=github)" github "github.com/codecloud-dev/moxsh-proot/releases/download"

# 7) 自定义镜像（BASE_URL 透传，ARCHIVE_URL 留空）
check "自定义镜像" \
  "$(mock_pick 0.10 0.10 MOXSH_PROOT_BASE_URL=https://mirror.example.com/moxsh)" custom "https://mirror.example.com/moxsh"

# 8) 未知 MOXSH_PROOT_SOURCE 值按自动处理（不炸，走探测）
check "未知 source 值按自动" \
  "$(mock_pick "" 0.30 MOXSH_PROOT_SOURCE=bogus)" gitee "gitee.com/thousand-code-crafts/moxsh-proot/releases/download"

# 9) 回退切换：github ↔ gitee 互换（下载受阻时的自救）
check "回退切换 github→gitee" \
  "$(mock_switch 0.10 0.10 github 'switch_source')" \
  gitee "gitee.com/thousand-code-crafts/moxsh-proot/releases/download" "/repository/archive/"
check "回退切换 gitee→github" \
  "$(mock_switch 0.10 0.10 gitee 'switch_source')" \
  github "github.com/codecloud-dev/moxsh-proot/releases/download" "/archive/refs/tags/"

# 10) custom 源不可切换（用户显式镜像，不擅自换端）：SOURCE/BASE_URL 原样不动
check "custom 源拒绝切换" \
  "$(mock_switch 0.10 0.10 custom 'switch_source || true' MOXSH_PROOT_BASE_URL=https://mirror.example.com/moxsh)" \
  custom "https://mirror.example.com/moxsh"

if [ "$fail" -ne 0 ]; then
  echo "分流回归：有用例失败" >&2
  exit 1
fi
echo "分流回归：全部通过"
