# moxsh-proot 路线图（roadmap）

本文件是 `mx:` 短链、配方系统、TUI 控制台、容器内控制台等一批大功能的**唯一决策记录**。
所有条目均经项目主人逐轮确认（10 轮问答、70 道），实现以此为准，不猜不改。

## 0. 总顺序与版本

顺序（主人定）：**引擎 → TUI → 品牌 → 官网**。版本 0.x 渐进：

| 版本 | 内容 | 状态 |
| --- | --- | --- |
| 0.2.0 | `mx:` 短链系统 + 容器内中文环境 + 容器内控制台 + base64 bootstrap + 安装向导 | 进行中 |
| 0.3.0 | 配方系统（recipes/ + index.toml + 桌面四件套 + VNC 双通道） | 未开始 |
| 0.4.0 | TUI 控制台（ratatui 第三二进制） | 未开始 |
| 1.0.0 | 短链/配方/TUI 全齐后的首个正式版 | 未开始 |

## 1. `mx:` 短链系统（0.2.0）

- **短链集**：只内置四大发行版——`mx:alpine` / `mx:ubuntu` / `mx:debian` / `mx:kali`，
  与 `distro install` 子命令面一一对应。用户纠正过：**不造「工具链镜像」概念**。
- **调用两层**：① 选项值 scheme：`moxsh-proot -r mx:alpine -- bash`；
  ② 子命令：`moxsh-proot-distro launch mx:alpine`（安装管理型入口）。
- **自定义短链**：`~/.config/moxsh-proot/links.toml` 写 `[links] mydev = "https://…/rootfs.tar.xz"`；
  CLI `--link name=url` 临时覆盖（flag 优先）。短链只映射 rootfs 地址，不做配方型（配方走 recipes）。
- **未装行为**：`mx:alpine` 未安装时**自动下载后进入**（不询问，首次也静默）。
- **缓存**：短链 rootfs 与 distro install **共用同一缓存**（`MOXSH_ROOT` 下），不双份占空间。
- **进容器两层**：① install 后在 `~/.local/bin`（或 PREFIX/bin）落盘启动器
  （装了 kali 输 `kali` 就进容器）；② 安装时往 `.bashrc`/`.zshrc` 注入 shell 函数兜底。
- **进/退命令自定义**：全局默认 + 每容器覆盖（`links.toml` 同级配置）。
  防误触示例：`enter = "echo 确认进入？&& read"`。**控制台命令不开放自定义**（见 §3）。
- **下载路径**：安装时询问（默认 `~/.moxsh`）+ 配置文件 + 环境变量三层。

## 2. 容器内中文环境（0.2.0）✅ 引擎侧已完成

现状：引擎只注入 `HOME`/`PATH`，**无 `LANG`/`LC_ALL`/`TERM`**——中文乱码根因。

- **locale 注入** ✅：`ProotConfig::env_command()` 注入 `LANG`/`LC_ALL`/`LC_CTYPE`/`TERM`——
  宿主透传优先，宿主没设才回落 `C.UTF-8` / `xterm-256color`；用户 `-e` 显式值最高。
- **env 文件机制** ✅：`<rootfs>/etc/moxsh/env`（每行 `KEY=VALUE`，`#` 注释），登录时读入合并；
  安装后自动写默认文件（`LANG=C.UTF-8`），已存在不覆盖。随 backup/restore 走。
- **排查入口** ✅：`moxsh-proot-distro env <id>` 预览登录时实际注入的完整环境。
- **中文字体**：配方装 Noto CJK / 文泉驿（配方形态，见 §4）。
- **中文输入法**：fcitx5 配方（配合 VNC 桌面打中文）。
- **安装器自身**：安装过程中文不乱码（两层都要：安装器层 + 容器层）。

## 3. 容器内控制台（0.2.0）✅ 引擎+安装器已完成（TUI 化属 0.4.0）

- **固定命令名** ✅：控制台命令固定不可自定义，定名 **`mox`**（安装时写 bin 目录 + 引擎 install 时同源落盘）。
- **容器内可见（两层都做）** ✅：
  ① bind-mount 宿主 bin 目录进容器固定挂载点 `/moxsh-host`（tmoe/社区标准做法的
  精准化——只绑 bin 不绑整个 PREFIX）；
  ② 引擎自动反穿透：`MOXSH_ROOT` 绑到 `/moxsh-root`，mox 脚本 exec 时把
  `MOXSH_ROOT=/moxsh-root` 交给 distro 程序——它经 proot 翻译回宿主真实目录，
  管理操作天然穿透（**不绕过翻译，让翻译为我们工作**）。
- **容器内边界**：全命令可用，但 TUI 标题栏常显**所在容器名**防迷失（TUI 0.4.0）。
- **`unset LD_PRELOAD`** ✅：Termux-exec 的 execve hook 与 proot 冲突，mox 脚本启动即清。
- **容器内执行短链零依赖** ✅：直接复用 bind-mount 进来的宿主二进制，不装任何东西。
- **嵌套保护** ✅：ptrace 不支持 proot 套 proot，容器内 `mox login/shell/launch` 劝退
  （靠 `MOXSH_CONTAINER=1` 标记；管理类子命令照常）。
- **可关**：`MOXSH_PROOT_CONSOLE=0` 关闭全部控制台注入（回归老行为）。

## 4. 配方系统（0.3.0）

- **存放**：`recipes/` 目录放**本仓库**（不新建仓库），**用时下载、不随包内置**。
- **索引**：`recipes/index.toml`（名字/描述/base/min_ram_mb/url/sha256），可被用户覆盖。
- **格式**：TOML 主体（声明式：装哪些包/写哪些配置）+ **shell 逃生舱**（复杂步骤挂片段）。
- **安全**：官方配方 sha256 校验；自定义配方与官方同权，文档明示「配方=任意代码」。
- **前置**：无容器先报错（要求先 `mx:alpine` 或 install）。
- **桌面配方**：XFCE / LXQt / KDE / MATE 四件套；索引带 `min_ram_mb`，启动前内存门槛提醒（低内存警告但允许）。
- **VNC 双通道**：tigervnc 默认（Termux 装 VNC Viewer App 连 127.0.0.1:5901）+ noVNC 可选（网页 6080）。
- **短链即配方**：`mx:xfce` 走配方执行框架（短链的 type=profile 扩展）。
- 参考 tmoe（2moe/tmoe）的桌面/VNC/中文/美化实现，**只仿照不照抄**——本引擎 clean-room
  重写，其绑定路径/LD_LIBRARY_PATH 那套不通用，拿过来必须改。

## 5. 安装链路（0.2.0）

- **base64 短命令**：命令内嵌 `base64+gzip` 微型 bootstrap（几十行：探测环境→选线路→
  后台拉全量安装脚本+引擎二进制）。文档另附内嵌全量版作离线备援。
- **国内外分流**：延迟探测 + IP 归属双管齐下；`MOXSH_PROOT_BASE_URL` 可强制覆盖。
- **bootstrap 自更新**：运行前探远端最新版，有新版换用新的再继续。
- **依赖全自动装**：Termux `pkg` / 桌面包管理器（apt/dnf/pacman/apk）/ 降级路径
  （无 root、网络不通 → 手动下载二进制 + 离线模式）。
- **主程序 = 安装向导**：环境体检（空间/架构/网络/依赖）→ 短链库清单选择 →
  下载 rootfs → 落启动器 + shell 函数 → **完成即进容器**（默认是）。

## 6. TUI 控制台（0.4.0）

- Rust ratatui，**第三个二进制 `moxsh-proot-tui`** + `moxsh-proot tui` 子命令两层入口。
- 全功能：发行版管理 / 短链浏览安装 / 自定义短链管理 / 配方浏览安装（TUI 为主+CLI 补充）/
  日志与诊断面板 / 安装引导。
- 长期独立维护；只管原版 Termux（moxsh App 内自有原生 UI，不冲突）。
- 引擎 FFI 暴露三组新 API 给 App：短链+配方 / locale+反查 / 引导状态。

## 7. 品牌与文档（品牌位）

- **新图标**：液态玻璃基因 + 全新符号 + 容器剖面意象三风格融合；先出 2-3 个 SVG 候选
  （含 16px 小尺寸效果）由主人挑，**不复用 moxsh 图标**。
- README：删 moxbox/moxcode 待定条目；产品表三行 + 路线图小节（指本文件）；
  赞助留爱发电链接、去二维码；中英双版保持。

## 8. 官网与后端（官网位）

- 官网：独立产品页 `/proot/`（特性/原理/短链速查/安装命令框）+ 首页卡片 +
  下载页双端（GitHub+Gitee）+ 命令框置顶 + 按地理位置/延迟智能推荐。**官网不换线路**（维持 Cloudflare）。
- 轻量后端：**通用**（全系产品共用，不止 PRoot）高性能 API：
  ① 版本检查（product/arch/libc → 最新版+地址）；
  ② 元数据/配方索引（聚合 recipes/index.toml，官网速查表同源）。
  性能是硬要求：CDN 友好缓存头、边缘缓存、静态化优先、限流防刷。
- 旧模块（auth/membership/marketplace/afdian/contact/api-proxy/star）安全加固与重构。

## 9. 运维红线

- **moxsh-terminal 删 proot 代码前，必须在工作目录留完整副本**（主人明确要求：
  「不能丢了，以后要交上去」）。副本路径：`/root/ws/backup/`。
- Gitee（thousand-code-crafts/moxsh-proot）：GitHub 主 + Gitee 镜像，**先手动同步**。
- 仓库必须公开（主人会亲自查）。
