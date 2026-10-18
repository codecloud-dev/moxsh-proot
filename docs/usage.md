# moxsh-proot 用法手册

> 对齐上游 PRoot 常用选项面（`-r/-0/-b/-w/-R/-S/-e`、`--` 后接命令），
> 由 Rust 自研 ptrace 引擎实现，非上游 C 版的封装。

## 1. moxsh-proot（单命令容器）

### 1.1 最小示例

```bash
# 推荐绑定集 + fake root，进 rootfs 里的 bash
moxsh-proot -R ~/ubuntu -0 -- bash

# 最小模式（装包场景用最小绑定集），指定初始目录
moxsh-proot -S ~/alpine -w /root -- sh

# 裸 rootfs 模式：只翻路径，不加任何推荐绑定
moxsh-proot -r ~/rootfs -0 -- /bin/sh
```

### 1.2 选项面

| 选项 | 说明 |
| :--- | :--- |
| `-r, --rootfs <path>` | guest 根目录（rootfs） |
| `-R <path>` | `-r` + 推荐绑定集（`/etc` 元文件、`/tmp`、`/run` 等，日常使用） |
| `-S <path>` | `-r` + 最小绑定集（装包场景） |
| `-0, --root-id` | fake root：uid/gid 显示为 0，伪装特权成功（无真 root） |
| `-b, --bind <h:g>` | 绑定宿主路径到 guest（`host:guest`，host 在前，可多次） |
| `-w, --cwd <dir>` | 初始工作目录（绝对路径按 guest 视角翻译） |
| `-e, --env <K=V>` | 注入环境变量（给了则清空继承环境，可多次） |
| `--` | 其后按原样视为命令 |

未给命令时默认 `/bin/sh`；未给 `-e` 时继承当前环境。

### 1.3 与上游的行为差异（诚实清单）

- **动态链接器**：初始 exec 与运行时 execve 均自动走「显式解释器调用」
  （与 qemu-user 同款：`execve(loader, [loader, prog, args...])`），
  guest 自带 musl/glibc loader 直接可用，无需宿主配套。
- **符号链接**：rootfs 内绝对 target 的符号链接（Alpine minirootfs 的
  applet 链接形态）在 exec / stat / openat 全链路自动解析，tar 直接
  解包未规范化的 rootfs 也能跑。
- **`-q/-k/-v` 等上游选项未实现**：不支持时明确报错，不假装支持。

## 2. moxsh-proot-distro（发行版管理）

```bash
moxsh-proot-distro list                  # 已安装 / 可安装一览
moxsh-proot-distro install alpine        # 安装（内部下载 + sha256 校验）
moxsh-proot-distro install ubuntu --cache ~/ubuntu.tar.xz
moxsh-proot-distro login alpine          # 登录 shell
moxsh-proot-distro shell alpine          # 非登录 shell
moxsh-proot-distro remove alpine
moxsh-proot-distro backup alpine ~/a.tar.gz
moxsh-proot-distro restore alpine ~/a.tar.gz
moxsh-proot-distro net                   # 网络边界报告（DNS/IPv6/出网）
```

内置四大发行版规格：Ubuntu 24.04 LTS / Debian 12 / Kali rolling /
Alpine 3.20（minirootfs）。

## 3. 钩子（容器内脚本运行时）

安装后钩子种子在 `$MOXSH_ROOT/hooks`（默认 `~/.moxsh/hooks`），
安装与登录时由引擎自动执行：

| 文件 | 时机 | 作用 |
| :--- | :--- | :--- |
| `provision.py` / `post-install.lua` | post-install | 供给检查（DNS/hosts/可写性） |
| `login_hook.py` / `pre-login.lua` | pre-login | 环境快照 / motd |

命名约定 `<kind>.<后缀>`，支持 `NN-` 数字排序前缀；解释器缺失时
优雅跳过（不阻断安装/登录）。声明式配置见 `lua/proot.lua`。

## 4. 从源码构建

```bash
cargo build --release          # 引擎 + 两个二进制（需 Rust 1.86+）
cargo test                    # 134 单测 + 1 集成
cargo clippy --all-targets -- -D warnings

go -C go build ./...          # Go 安装器（install/update/probe）
python3 py/verify/verify_engine.py   # 三层验证（单测 + 钩子直跑 + 真实 rootfs e2e）
```

e2e 验证会自动下载 Alpine minirootfs（约 4MB）到临时目录。

## 5. 支持与反馈

- Issue: <https://github.com/codecloud-dev/moxsh-proot/issues>
- 许可证：GPL-3.0
