> **🌐 主仓库在 GitHub** — [`codecloud-dev/moxsh-proot`](https://mox-site.pages.dev/go?r=moxsh-proot) 是唯一主源：Issue、Pull Request、功能讨论请统一提交到 GitHub。[Gitee 镜像](https://gitee.com/thousand-code-crafts/moxsh-proot) 仅供国内高速只读访问，每小时从 GitHub 自动同步。

<p align="center">
  <img src="assets/logo.svg" width="128" alt="mox-proot 液态玻璃标志">
</p>

<h1 align="center">mox-proot</h1>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/codecloud-dev/moxsh-proot/rust-check.yml?branch=main&label=CI%20Build&color=8a7bff" alt="CI Build">
  <img src="https://img.shields.io/github/v/release/codecloud-dev/moxsh-proot?label=Latest&color=37d5d3" alt="Latest">
  <img src="https://img.shields.io/github/stars/codecloud-dev/moxsh-proot?style=social" alt="GitHub Stars">
  <img src="https://img.shields.io/badge/version-0.9.0-8a7bff" alt="version">
  <img src="https://img.shields.io/badge/license-GPL--3.0-37d5d3" alt="license">
  <img src="https://img.shields.io/badge/Core-Rust-000?logo=rust&logoColor=white" alt="Rust 内核">
  <img src="https://img.shields.io/badge/Installer-Go-00ADD8?logo=go&logoColor=white" alt="Go 安装器">
  <img src="https://img.shields.io/badge/Hooks-Python%20%2F%20Lua-3776AB?logo=python&logoColor=white" alt="钩子脚本">
  <img src="https://img.shields.io/badge/Bridge-C-A8B9CC?logo=c&logoColor=white" alt="C 桥">
  <img src="https://img.shields.io/badge/Platform-Linux%20%2F%20Termux(Android)-3DDC84?logo=android&logoColor=white" alt="桌面 Linux / Termux(Android)">
</p>


> 不 root、不装特权、不动系统分区——在 Termux / Linux 里跑一整个 Linux 发行版。Rust 自研 ptrace 路径翻译引擎，对齐上游 PRoot 常用命令面，一键安装，开箱即用。

<p align="center"><a href="README.md">中文</a> · <a href="README.en.md">English</a></p>

<p align="center"><b>⭐ 如果 mox-proot 对你有用，欢迎点个 <a href="https://mox-site.pages.dev/go?r=moxsh-proot">Star</a> —— 它能让更多人在手机上、在容器里用上顺滑的 Linux!</b></p>

## 品牌字标

```
_  _ ____ _  _
|\/| |  |  \/
|  | |__| _/\_
  mox-proot — 多架构容器引擎
```

> 进入即见：容器内控制台 `mox` 无参进入时打印品牌标识；源码见 `src/proot/brand.rs`（`BANNER` / `TAGLINE`）。

## 🎁 惊喜体验三件套（时间上下文 / 古诗 / 彩蛋）

无参进入 `mox`（或 TUI 启动欢迎屏）时，会按**本机真实时区**把一天分成
`晨 / 午 / 晚 / 夜` 四相，送一句应景问候 + 一句古诗；另外还藏了彩蛋暗号。
纯本地、零依赖、零网络，源码单一来源在 `src/proot/delight.rs`。

| 时相 | 时段 | 问候 | 古诗 |
| :--: | :--: | :--- | :--- |
| 晨 | 05–10 | 早安，新的一天从容器里开始 ☀ | 晨起开门雪满山，雪晴云淡日光寒。——郑燮《山中雪后》 |
| 午 | 11–16 | 午安，喝口水再战 ☕ | 锄禾日当午，汗滴禾下土。——李绅《悯农》 |
| 晚 | 17–22 | 晚安，夜色正好适合折腾 🌙 | 夕阳无限好，只是近黄昏。——李商隐《登乐游原》 |
| 夜 | 23–04 | 夜深了，注意身体，别熬太晚 🌃 | 春眠不觉晓，处处闻啼鸟。——孟浩然《春晓》 |

```bash
mox                       # 无参：品牌字标 + 时相问候 + 古诗
mox egg                   # 彩蛋暗号：egg / 彩蛋 / konami / 蛋
mox-proot-distro time     # 单独打印时相 + 问候 + 古诗
MOX_EGG=1 mox-proot-tui   # 环境变量亦可显式点名彩蛋
```

## 🖥️ TUI 控制台与版本选择

```bash
mox-proot-tui                              # 六标签 TUI（欢迎屏 / Gauge 进度 / 二维码）
mox-proot-distro versions ubuntu           # 列出可选版本：24.04 LTS / 22.04 LTS / 20.04 LTS
mox-proot-distro install alpine --version "3.19 (minirootfs)"   # 选版本号安装
```

TUI 六个标签：**发行版 / 短链 / 配方 / 日志 / 向导 / 账号**。启动先出欢迎屏
（品牌字标 + 时相问候 + 古诗），任意键进入；安装改为后台线程，界面上跑
实时 Gauge + Spinner，列表带滚动条，账号页内嵌登录入口二维码。各面板的
简笔画示意图见 [docs/tui-showcase.md](docs/tui-showcase.md)。

## 🐛 欢迎来“批斗”我

> 这是个刚起步的项目，**bug 肯定有，而且不少**。我不装完美——
> 你踩到的每一个坑、每一个槽点，都是帮我把它养好的机会。

- 💥 遇到崩溃 / 跑不起来 / 结果不对？→ [提个 Bug 报告](https://mox-site.pages.dev/go?r=moxsh-proot&p=issues)
- 💡 有想要的功能？→ [开个需求](https://mox-site.pages.dev/go?r=moxsh-proot&p=issues)
- 🗯️ 单纯想吐槽、挑刺？→ 也欢迎开 issue，标签随便打 😄

每个 issue 我都会看，能修的尽快修。一起把它从“能跑”养到“好用” 💪

<p align="center">💛 觉得好用？欢迎到 <a href="https://afdian.com/a/cloudharbor">爱发电</a> 请作者喝杯咖啡 —— 国内可直接微信 / 支付宝收款，是独立开发最大的鼓励。</p>

<details>

<summary>目录</summary>

- [MoX 系列](#mox-系列)
- [核心特性](#核心特性)
- [它是怎么工作的](#它是怎么工作的)
- [快速上手](#快速上手)
- [仓库结构](#仓库结构)
- [和上游 PRoot 的关系](#和上游-proot-的关系)
- [文档](#文档)
- [参与进来](#参与进来)
- [支持我们](#支持我们)
- [许可证](#许可证)

</details>

---

## 🌟 MoX 系列

mox-proot 是 **MoX 工具系列**的基础设施组件：mox-proot 终端里的「PRoot 发行版」能力，引擎本体就住在这个仓库。一套审美，把桌面级工具搬进掌心。

|      产品      |   状态   | 一句话定位                                                            |
| :----------: | :----: | ---------------------------------------------------------------- |
| **mox-proot** | 已上线 | 本仓库：Rust 自研 PRoot 容器引擎（Android 液态玻璃终端 / Termux 生态的发行版内核，也可独立使用） |
| **mox-site** |  已上线 | 本系列官方门户（[官网](https://codecloud-dev.github.io/mox-site/)）              |

### 🗺️ 路线图（0.2.0 进行中）

| 里程碑 | 内容 | 状态 |
| :--- | :--- | :--- |
| 0.2.0 | `mx:` 短链系统（`mx:alpine` 即装即进）、落盘启动器（装了 kali 输 `kali` 就进）、容器内中文环境（locale/字体/输入法）、容器内控制台、base64 短命令 + 国内外自动分流 | 进行中 |
| 0.3.0 | 配方系统（`mx:xfce` 一键桌面，XFCE/LXQt/KDE/MATE + VNC 双通道） | 规划中 |
| 0.4.0 | TUI 控制台（ratatui 全功能面板） | 已实现（0.9.0 随 `mox-proot-tui` 交付） |
| 1.0.0 | 短链/配方/TUI 全齐的首个正式版 | 规划中 |

完整决策记录见 [docs/roadmap.md](docs/roadmap.md)。

---

## ⚙️ 核心特性

| 维度               | 说明                                             |
| :--------------- | ---------------------------------------------- |
| **Rust ptrace 引擎** | 路径翻译、多进程树跟踪、fake root 全部 Rust，内存安全、无外部运行时 |
| **对齐上游命令面** | `-r/-0/-b/-w/-R/-e` 与 `--` 切分照旧，老用户零迁移；不支持的选项明确报错，不假装 |
| **三二进制** | `mox-proot`（单命令容器）+ `mox-proot-distro`（四大发行版安装/登录/备份/恢复）+ `mox-proot-tui`（ratatui TUI 控制台） |
| **显式解释器调用** | 动态 ELF 自动走 guest 自带 loader（qemu-user 同款方案），初始 exec 与运行时 execve 双覆盖 |
| **符号链接全链路** | 绝对 target 链接（Alpine applet 形态）在 exec / stat / openat 自动解析，tar 裸解包的 rootfs 也能跑 |
| **fake root** | uid/gid 显示为 0、chown/setuid 系伪装成功——不需要真 root，不碰系统分区 |
| **网络栈** | DNS 统一 + IPv6 双栈探测 + 出网边界报告（`mox-proot-distro net`） |
| **钩子运行时** | post-install / pre-login 生命周期，Python / Lua 双解释器，容器内脚本即插即用 |
| **一键配方** | 声明式 TOML + shell 逃生舱，`recipe list/show/apply` 一键装桌面/VNC |
| **一键安装** | bash 探测层（`curl \| bash` 零依赖）+ Go 安装器（install/update/probe），双层同源 |

---

## 🔧 它是怎么工作的

mox-proot 不是虚拟机，也不是模拟器——和上游 PRoot 一样，它做的是
**ptrace 路径翻译**：子进程的每个文件系统调用都被拦截，guest 视角的
路径在寄存器层改写为宿主 rootfs 内的真实路径，`chroot`、`mount --bind`、
`binfmt_misc` 的效果由此实现，而**不需要任何特权**。

引擎由 Rust 从零重写（非上游 C 版的封装），在此之上补齐了移动端现实
问题的解法：

- **显式解释器调用**：内核 exec 带 `PT_INTERP` 的 ELF 时会按 ELF 头里的
  解释器路径在**宿主**命名空间查找，必然 ENOENT。引擎在 exec 停把命令
  改写为 `execve(loader, [loader, prog, args...])`（与 qemu-user 同款），
  guest 自带 musl/glibc loader 直接上场，其后的 openat 全部落在翻译链上；
- **绝对符号链接**：rootfs 内 `/bin/ls -> /bin/busybox` 这类绝对 target
  的链接，其 target 由内核在宿主空间按字面解析。引擎在 exec / stat /
  openat 全链路做组件级走查，把链接解析成 rootfs 内真实路径——tar 直接
  解包、未规范化的 rootfs 也能跑；
- **多进程树**：TRACEFORK/VFORK/CLONE 全收，`apt/dpkg` 这类 fork 密集型
  负载不挂死；长路径经 tracee 自有 scratch 区重指（loader-mem 等效机制）。

---

## 🚀 快速上手

### 🔹 一键安装（桌面 Linux / Termux(Android) 全适配）

**方式一：短命令**（推荐）——整条安装引导压成一段 base64 内嵌，复制即跑，
不挑域名；GitHub raw 不通自动回退 Gitee（国内免折腾镜像）：

<details>
<summary>展开短命令（约 1.5 KB，点开复制整段粘贴执行）</summary>

```bash
echo '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' | base64 -d | gzip -d | sh
```

</details>

**方式二：标准命令**：

```bash
curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-mox-proot.sh | bash
```

**国内备援**（Gitee raw，直连）：

```bash
curl -fsSL https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-mox-proot.sh | bash
```

零依赖（只要 curl + coreutils）。脚本**全平台自适应**：桌面 Linux
（x86_64/arm64）与 Termux（Android）自动识别安装目录与架构（glibc / musl 自动选静态版），
不再只绑 Termux。二进制按国内/国外自动探测选源下载，逐文件 sha256 校验，自动落盘
`mox-proot` 与 `mox-proot-distro` 两个命令。

> **国内/国外都稳**：二进制已同步到 Gitee 发布（与 GitHub 同源），GitHub 直连
> 超时自动回退 **Gitee** 源；两端皆不通时可显式设置 `MOX_PROOT_MIRROR_PROXIES`
> 加 ghproxy 镜像兜底（公共代理存活率不稳，默认关闭）。每个文件都做 sha256 强校验。
> 官网静态 `/dl` 通道已确认不做并从安装脚本移除，避免死链空等。

装完就能用：

```bash
mox-proot -R ~/ubuntu -0 -- bash        # 推荐绑定集 + fake root 进发行版
mox-proot --help                        # 完整用法
```

更丰富的安装/更新体验走 Go 安装器（Release 资产
`mox-proot-installer-<goos>-<arch>`，子命令 install / update / probe）：

```bash
mox-proot-installer install
mox-proot-installer update
```

### 🔹 装个发行版玩玩

```bash
mox-proot-distro install alpine     # Ubuntu / Debian / Kali / Alpine
mox-proot-distro login alpine       # 进登录 shell
```

也可以直接短链启动（未装自动装，装完直接进）：

```bash
mox-proot-distro launch mx:alpine
```

一条命令初始化（数据根 + 发行版 + 启动器 + 教学提示，重复跑只补缺的）：

```bash
mox-proot-distro wizard                    # 默认 ~/.mox-proot + alpine
mox-proot-distro wizard --root /sd/data --distro ubuntu   # 自定义
```

**中文环境开箱即用**：登录时引擎自动注入 `LANG`/`TERM`（宿主中文环境会
透传进容器；兜底 `C.UTF-8`，不乱码）。想要完整中文显示：容器内装
locales 包并 `locale-gen zh_CN.UTF-8`，然后编辑 rootfs 里的
`etc/mox-proot/env` 把 `LANG` 改成 `zh_CN.UTF-8`。随时可查当前生效环境：

```bash
mox-proot-distro env alpine
```

### 🔹 容器内控制台

进任意容器后，固定命令 `mox`（不开放自定义）直接拉起管理控制台——
bind-mount 反穿透到宿主，装新发行版、查网络、看环境都不用退出容器：

```bash
# 容器内（任意发行版里）：
mox                 # 管理控制台（list / install / remove / net / env）
mox install kali    # 在容器里再装一个发行版（落到宿主 MOX_ROOT）
mox net             # 网络边界报告
```

容器内不能再嵌套进其他容器（ptrace 不支持套娃），`mox login/shell/launch`
会提示你先退出。`MOX_PROOT_CONSOLE=0` 可关闭整套控制台注入。

### 🔹 系统要求

- 桌面 Linux（x86_64 / arm64），或 Termux（Android 9+，aarch64 / x86_64；32 位 ARM 需源码构建）
- 约 50 MB 可用空间（引擎 + 一个 minirootfs）

> 平台说明：mox-proot 是 **Linux / Android（Termux）的 ptrace 路径翻译引擎**，
> 仅在 Linux 内核上运行容器（macOS / Windows 无 ptrace 等价能力，无法跑容器）。
> 设备码扫码登录（`mox-proot login`）这类纯网络客户端在任意带 curl 的系统都可跑，
> 但**容器引擎本身不支持 macOS / Windows**。

### 🔹 从源码构建

```bash
git clone https://github.com/codecloud-dev/moxsh-proot.git
cd mox-proot
cargo build --release
```

完整构建/测试/验证命令见 [docs/usage.md](docs/usage.md)。

---

## 📦 仓库结构

一个引擎，五种语言，各守其位：

| 目录            | 语言     | 职责                                  |
| :------------ | :----- | ----------------------------------- |
| `src/proot/`  | Rust   | 引擎核心：ptrace 翻译、ELF/exec 链、发行版、网络栈、钩子发现 |
| `src/bin/`    | Rust   | `mox-proot` / `mox-proot-distro` / `mox-proot-tui` 三个独立二进制 |
| `go/`         | Go     | 安装器 + 下载器（install/update/probe），Linux / Termux 两靶 |
| `py/runtime/` | Python | 容器内钩子（供给检查 / 登录快照），宿主侧可直跑自证 |
| `py/verify/`  | Python | 三层验证脚本（单测 + 钩子直跑 + 真实 rootfs e2e） |
| `lua/`        | Lua    | 声明式配置（`proot.lua`）+ 钩子            |
| `c/`          | C      | Android JNI 桥（`proot_bridge.c`）      |
| `scripts/`    | bash   | 一键安装脚本（探测层）                       |

---

## 🔗 和上游 PRoot 的关系

- **行为层面**：对齐。常用选项（`-r/-0/-b/-w/-R/-S/-e`、`--` 切分）与
  proot-distro 子命令面（list/install/login/backup/restore）保持一致，
  老用户零迁移；
- **代码层面**：AI 辅助生成、clean-room 重写。引擎全部用 Rust 重新实现
  （ptrace 停止机、路径翻译、ELF 解析、发行版管理）；开发过程研读上游
  公开文档与源码以对齐行为契约，但**未复制其代码**，也不依赖修改上游；
- **生态层面**：兼容。跑的还是官方 rootfs 镜像（Alpine minirootfs 等），
  安装方式照旧；本仓库额外提供 Rust 引擎 + 一键安装 + 五语言钩子体系。

---

## 📖 文档

- [用法手册](docs/usage.md) —— 选项面、子命令、钩子、构建与验证
- [TUI 简笔画图册](docs/tui-showcase.md) —— 六个标签页 + 欢迎屏 + 惊喜三件套的 ASCII 示意图
- [路线图决策记录](docs/roadmap.md) —— 里程碑与设计取舍

---

## 🤝 参与进来

发现 bug 或想要新功能，欢迎提 [Issue](../../issues)；想贡献代码直接提 Pull Request。

---

## 💖 支持我们

如果这个项目对你有用，**求一个 Star、点个关注、顺手转发**——这对一个独立小项目来说，是天大的鼓励，也能让更多人在容器里用上 Linux。

- 在 GitHub 上点 ⭐ Star：点开仓库右上角的 Star
- 关注仓库，第一时间收到更新
- 转发给你的朋友、群里、或任何被 root 权限折磨过的开发者

也欢迎任何形式的共建：报 bug、提建议、交代码、写文档、做翻译。独立开发者 + AI 协作，但每一个方向都由人拍板。

---

## 📜 许可证

[GPL-3.0](LICENSE)

## 🤖 AI 辅助声明

本项目（含全部代码、文档与官网页面）由开发者 **codecloud-dev** 主导设计，**AI 辅助生成代码**：架构决策、需求定义与验收由人完成，代码实现与文档撰写由 AI 协作完成并经人工审核修订。
