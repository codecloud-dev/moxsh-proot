#!/bin/sh
# 配方：容器快照与克隆（ProoT 增值服务 proot-pro 专属）
# 对当前 rootfs 做 tar 增量快照并一键克隆，支持备份还原与多实例并行。
. /tmp/mox-proot-lib.sh
mox-proot_info "安装容器快照/克隆工具（ProoT 增值能力）…"

pkginstall tar pigz 2>/dev/null

SNAP_DIR="${MOX_ROOT:-$HOME/.mox-proot}/snapshots"
mkdir -p "$SNAP_DIR"

# 快照：把 $MOX_ROOT/rootfs 打包到快照目录（排除 proc/sys/dev 等伪文件系统）
cat > /usr/local/bin/mox-proot-snapshot <<EOF
#!/bin/sh
# 用法：mox-proot-snapshot <名称>  生成一份 rootfs 快照
set -e
NAME="\${1:-snap-\$(date +%Y%m%d-%H%M%S)}"
SRC="\${MOX_ROOT:-$HOME/.mox-proot}/rootfs"
DST="$SNAP_DIR/\$NAME.tar.gz"
tar --exclude='proc/*' --exclude='sys/*' --exclude='dev/*' --exclude='$SNAP_DIR' \\
  -C "\$SRC" -czpf "\$DST" . 2>/dev/null
echo "快照已生成：\$DST"
EOF
chmod +x /usr/local/bin/mox-proot-snapshot

# 克隆：从快照解包出一个新实例目录
cat > /usr/local/bin/mox-proot-clone <<EOF
#!/bin/sh
# 用法：mox-proot-clone <快照名> <新实例路径>
set -e
SRC="$SNAP_DIR/\${1}.tar.gz"
DST="\${2:?用法: mox-proot-clone <快照名> <新实例路径>}"
mkdir -p "\$DST"
tar -C "\$DST" -xzpf "\$SRC" 2>/dev/null
echo "已克隆到：\$DST"
EOF
chmod +x /usr/local/bin/mox-proot-clone

mox-proot_ok "快照/克隆就绪：mox-proot-snapshot <名> 备份，mox-proot-clone <名> <路径> 还原/并行"
