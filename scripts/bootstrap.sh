#!/bin/sh
# mox-proot 微型引导（bootstrap）——base64 内嵌进「一键安装」短命令里。
#
# 职责边界（roadmap §5）：本脚本只做「并行探测选最快源 → 拉全量安装器 → exec 交棒」，
# **不重复实现安装逻辑**。全量脚本永远从 main 拉，天然最新，无需自更新；
# 国内/国外下载源分流由全量安装器（scripts/install-mox-proot.sh）自行探测，
# 本层只管「把安装器本身用最快的源拿到手」。
#
# 让引导下载特别快（参考 tmoe 的「就近源优选 + 短超时快失败」并重写优化）：
#   1) 并行 HEAD/GET 探测各镜像 RTT（后台并发，gitee 境内近、github 海外、jsdelivr 兜底）；
#   2) 按实测耗时选最快者下载；
#   3) 探测全败再串行回退其余源——绝不让用户干等一个卡死的源。
#
# 本文件由 scripts/gen-shortcmd.sh 生成为内嵌命令，修改后必须重新生成同步。

set -u

# 镜像源（标签 + URL）。国内 gitee 优先，海外 github，jsdelivr 作 CDN 兜底。
GH="https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-mox-proot.sh"
GT="https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-mox-proot.sh"
JS="https://cdn.jsdelivr.net/gh/codecloud-dev/moxsh-proot@main/scripts/install-mox-proot.sh"

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }

# 下载回来的必须是**安装器脚本**，不能是 200 状态的 HTML 错误页 / CDN 拦截页。
# 探源已过滤非 200，但 CDN 劫持、镜像缓存脏仍可能返回 200 + HTML；
# `exec sh "$TMP"` 之前再做一次内容断言，杜绝「把网页当脚本执行」。
is_installer() {
    [ -s "$1" ] || return 1
    head -c 1024 "$1" 2>/dev/null | grep -qi '<html\|<!doctype\|<head\|<script' && return 1
    head -c 1024 "$1" 2>/dev/null | grep -q 'mox-proot' || return 1
    return 0
}

command -v curl >/dev/null 2>&1 || die "未找到 curl（Termux 里执行: pkg install curl）"

TMP="${TMPDIR:-/tmp}/mox-proot-installer.$$"
trap 'rm -f "$TMP"' EXIT

# ---- 1) 并行探测：各源后台 GET 计时，结果写 per-label 文件，wait 收齐 ----
probe_dir="${TMPDIR:-/tmp}/mox-proot-probe.$$"
mkdir -p "$probe_dir"
for pair in "gh $GH" "gt $GT" "js $JS"; do
    set -- $pair
    label="$1"; url="$2"
    (
        # 【P0 修复，与 install-mox-proot.sh::probe_seconds 同源】
        # 旧实现只取 %{time_total}：curl 即使撞上 404 / 登录跳转 / CDN 错页 / 域名不存在，
        # 也会打印一个耗时数字，于是「打不开的源」被当成「可达且最快」，随后
        # 下载回来的是 HTML 错误页，却直接 `exec sh "$TMP"` 执行——这是引导层最危险的坑。
        # 现在同时取 http_code，**仅最终 200** 才记为候选；000/404/3xx/5xx 一律落选。
        out=$(curl -sSL -o /dev/null -w '%{http_code} %{time_total}' \
              --connect-timeout 4 --max-time 10 "$url" 2>/dev/null || true)
        code="${out%% *}"
        rtt="${out#* }"
        if [ "$code" = "200" ] && [ -n "$rtt" ] && [ "$rtt" != "$out" ]; then
            printf '%s %s %s\n' "$rtt" "$label" "$url" > "$probe_dir/$label"
        fi
    ) &
done
wait

# ---- 2) 按实测耗时选最快（sort -n 取首行，cut 取字段，避开浮点比较/awk 依赖）----
best=$(cat "$probe_dir"/* 2>/dev/null | sort -n | head -1 | cut -d' ' -f3)
best_label=$(cat "$probe_dir"/* 2>/dev/null | sort -n | head -1 | cut -d' ' -f2)
rm -rf "$probe_dir"

if [ -n "$best" ]; then
    say "从最快源($best_label)拉取安装器…"
    if curl -fsSL --connect-timeout 10 --max-time 180 "$best" -o "$TMP" 2>/dev/null && is_installer "$TMP"; then
        say "交棒全量安装器（依赖检查 / 选源下载 / 校验 / 落盘）…"
        exec sh "$TMP"
    fi
    say "最快源返回内容不是安装器，改走串行回退…"
fi

# ---- 3) 探测全败：串行回退兜底（绝不干等卡死的源）----
say "并行探测无果，串行回退各源…"
for pair in "gh $GH" "gt $GT" "js $JS"; do
    set -- $pair
    label="$1"; url="$2"
    if curl -fsSL --connect-timeout 10 --max-time 180 "$url" -o "$TMP" 2>/dev/null && is_installer "$TMP"; then
        say "交棒全量安装器（源 $label）…"
        exec sh "$TMP"
    fi
done

die "所有下载源（GitHub / Gitee / jsDelivr）都拉取失败，请检查网络后重试"
