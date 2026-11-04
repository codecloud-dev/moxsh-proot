# mox-proot 成果展示 · TUI 简笔画图册

> 这一份是专门给你看的「成果清单 + TUI 简笔画」。所有界面都用字符画成示意草图  
> （简笔画，不是截图），每个框、每个箭头都对应仓库里真实存在的代码。
>
> 本次一口气做完：**惊喜三件套（时间上下文 / 古诗 / 彩蛋）** + T2 CLI 颜色进度条
>
> - T3 版本列表选版本 + T4 半截 TUI 升级 + T5 安装脚本精修 + T6 README 修复
> - T7 CI 等价校验。



---

## 0. 先说「那个 txt」和彩蛋

我把仓库、`/workspace`、上传目录、git 历史、交接 JSON 都翻了一遍：**没有找到那份写着  
「彩蛋 / 按时间上下文感知 / 古诗诗句」的 txt 规格文件**（能找到的 `.txt` 全是  
`termux-src` 里的许可证和构建缓存，跟需求无关）。这份需求只出现在你的消息里。

所以我按你描述的语义**逐条落地、没有弄丢**，并把它们钉在代码和文档里：

| 你提到的         | 落到哪里                                                              | 怎么触发                            |
| :----------- | :---------------------------------------------------------------- | :------------------------------ |
| **按时间上下文感知** | `src/proot/delight.rs` → `TimePhase::from_hour` / `current_phase` | 读本机时区小时，分晨/午/晚/夜                |
| **古诗诗句**     | `delight.rs` → `poem()` / `poem_from()`                           | 每个时相配一句 + 作者出处                  |
| **彩蛋**       | `delight.rs` → `easter_egg()` / `egg_enabled_via_env()`           | `mox egg`、`konami`、`MOX_EGG=1`  |
| 控制台无参欢迎      | `console.rs` 的 `CONSOLE_SCRIPT`（shell 自包含版）                       | 进容器直接敲 `mox`                    |
| TUI 欢迎屏      | `tui.rs` → `render_welcome`                                       | 启动 `mox-proot-tui`，任意键进入        |
| CLI 单独取用     | `cli.rs` → `cmd_time` / `cmd_egg`                                 | `mox-proot-distro time` / `egg` |

> 设计原则：**Rust 单一来源**（`delight.rs`）+ **shell 自包含副本**（`mox` 脚本）。  
> 后者不依赖任何二进制，即使 `mox-proot-distro` 坏了，进容器照样能收到问候和诗。

---

## 1. 惊喜三件套 · 时间轴简笔画

```
   05:00        11:00        17:00        23:00        05:00
     |------------|------------|------------|------------|
      晨           午            晚            夜
      ☀            ☕            🌙            🌃
   早安…        午安…         晚安…         夜深了…
   雪满山        日当午        无限好        不觉晓
   郑燮          李绅          李商隐        孟浩然
     |            |            |            |
     +------------+------------+------------+--> 读本地时区 (libc localtime)
```

**四相与诗句对照**

|  时相 |   时段  | 问候               | 古诗                         |
| :-: | :---: | :--------------- | :------------------------- |
|  晨  | 05–10 | 早安，新的一天从容器里开始 ☀  | 晨起开门雪满山，雪晴云淡日光寒。——郑燮《山中雪后》 |
|  午  | 11–16 | 午安，喝口水再战 ☕       | 锄禾日当午，汗滴禾下土。——李绅《悯农》       |
|  晚  | 17–22 | 晚安，夜色正好适合折腾 🌙   | 夕阳无限好，只是近黄昏。——李商隐《登乐游原》    |
|  夜  | 23–04 | 夜深了，注意身体，别熬太晚 🌃 | 春眠不觉晓，处处闻啼鸟。——孟浩然《春晓》      |

**控制台实跑（当晚 21 点）**

```
  $ mox
  _  _ ____ _  _
  |\/| |  |  \/
  |  | |__| _/\_
    mox-proot
  晚安，夜色正好适合折腾 🌙              <-- 时间上下文感知
    夕阳无限好，只是近黄昏。 ——李商隐《登乐游原》   <-- 古诗 + 出处
```

**彩蛋暗号**

```
  $ mox egg        （或 mox 彩蛋 / mox konami / mox 蛋）
  🥚 彩蛋：你找到了隐藏菜单！
     mox-proot 的每一行代码都是手写对齐上游 PRoot 行为——没有抄。
     试试无参 mox 进入，看时间送你的那句诗。

  $ MOX_EGG=1 mox-proot-tui     <-- 环境变量：欢迎屏直接把彩蛋展开
  $ mox-proot-distro time       <-- 单独打印时相 + 问候 + 古诗
  $ mox-proot-distro egg        <-- CLI 版彩蛋
```

---

## 2. TUI 总骨架（六个标签页）

```
  ┌──────────────────────────────────────────────────────────┐
  │  mox-proot │ TUI 控制台                                   │  <- 标题
  ├──────────────────────────────────────────────────────────┤
  │ [发行版] │ 短链 │ 配方 │ 日志 │ 向导 │ 账号             │  <- Tab 切换
  ├──────────────────────────────────────────────────────────┤
  │                                                          │
  │                   主体区（随 Tab 换）                    │  <- Min(4)
  │                                                          │
  ├──────────────────────────────────────────────────────────┤
  │ ↑↓ 选择 · Enter 登录/安装 · r 删除 · b 备份 · Tab 切换 · q│  <- 状态栏/帮助
  └──────────────────────────────────────────────────────────┘
       ▲                ▲                            ▲
       │                │                            └─ 有 status 时前置绿字状态
       │                └─ 六个 render_* 函数各画一块
       └─ Layout 竖直三段：Length(3) / Min(4) / Length(2)
```

**启动欢迎屏**（T4 新增，任意键进主界面，`q`/`Esc` 退出）

```
  ┌────────────────────── mox-proot TUI ──────────────────────┐
  │                                                          │
  │              _  _ ____ _  _                              │
  │              |\/| |  |  \/                               │  <- 品牌字标
  │              |  | |__| _/\_                              │
  │                mox-proot                                 │
  │                                                          │
  │           晚安，夜色正好适合折腾 🌙                       │  <- 时间上下文
  │      夕阳无限好，只是近黄昏。 ——李商隐《登乐游原》        │  <- 古诗
  │                                                          │
  │           数据根：~/.mox-proot                            │
  │           时相：晚   ·   已装 2 个发行版                  │
  │                                                          │
  │   🥚 彩蛋暗号：mox egg / mox-proot-distro egg             │
  │      （MOX_EGG=1 时此处直接展开彩蛋正文）                 │
  │                                                          │
  │        按任意键进入控制台   ·   q 退出                    │
  └──────────────────────────────────────────────────────────┘
```



---

## 3. 发行版页（列表 + 滚动条）

```
  ┌─ 发行版（Enter 登录已装 / 安装可装）────────────────── ▲ ┐
  │ >> [已装] alpine                                     █  │
  │    [已装] ubuntu                                     █  │  <- 高亮 >> 黄底
  │    [可装] debian   Debian (12 (bookworm))            ░  │
  │    [可装] kali     Kali (rolling …)                   ░  │
  │    [可装] …                                          ░  │
  │                                                      ▼  │
  └──────────────────────────────────────────────────────────┘
                                                     ▲
                              Scrollbar（T4 新增）───┘
                              ScrollbarState::new(rows.len())
                                .position(selected)

  键位： ↑/↓ 移动   Enter 登录(已装)/安装(可装)   r 删除   b 备份
```

**安装浮层（T4 新增：后台线程 + Gauge + Spinner）**

安装搬到工作线程，进度经 `mpsc::channel` 回传，每帧 `poll_install()` 消费，界面不卡死：

```
        ┌────────────── ⠹ 安装进行中 ────────────────────┐
        │  ⠹ 解压 rootfs                                │  <- Spinner 帧
        │  ███████████████░░░░░░░░  67%  (67/100)     │  <- Gauge
        │  已用时 12s   ·   q 退出 / 进度在后台继续       │
        └────────────────────────────────────────────────┘
              ▲                       ▲
              │                       └─ 无总量时显示 indeterminate
              └─ ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ 十帧轮回，零额外依赖

  线程模型（简笔画）：
    [TUI 主线程] --spawn--> [worker: DistroManager::install]
          ^                          |
          |      mpsc::channel        | Progress{stage,done,total}
          +--------------------------+ Finished(Result)
          |
     每帧 poll_install() -> 更新 InstallTask 快照 -> 重绘 Gauge/Spinner
```

---

## 4. 短链页 / 配方页 / 日志页 / 向导页

```
  短链                          配方
  ┌────────────────────────┐    ┌──────────────────────────────┐
  │ 短链（Enter 安装/启动）│    │ 配方（N 个）                 │
  │ >> mx:alpine           │    │ >> XFCE   轻量桌面 …         │
  │    mx:ubuntu           │    │    LXQt   …                  │
  │    mx:debian           │    │    KDE    …                  │
  │    mx:kali             │    │    VNC    双通道 …           │
  └────────────────────────┘    └──────────────────────────────┘
     空列表时给内置短链提示        索引来自 recipes/index.toml

  日志 / 诊断                    安装向导
  ┌────────────────────────┐    ┌──────────────────────────────┐
  │ 数据根: ~/.mox-proot   │    │ 安装向导：一次性初始化        │
  │ 已安装发行版: 2        │    │  数据根 + 首个发行版 +        │
  │ 可安装预置: 2          │    │  启动器 + 控制台              │
  │ 配方索引: 6 个         │    │                              │
  │                        │    │ 按 Enter 运行                 │
  │ 提示：操作实时反映在此  │    │  mox-proot-distro wizard      │
  └────────────────────────┘    └──────────────────────────────┘
```

---

## 5. 账号页（设备绑定 + 内嵌二维码）

```
  窄屏（<72 列）                     宽屏（≥72 列）：右侧内嵌二维码
  ┌────────────────────────────┐    ┌──────────────────────┬───────────────┐
  │ 账号（设备绑定·扫码登录）  │    │ 绑定状态：已绑定 ✓   │ 登录入口（示意）│
  │ 绑定状态：已绑定 ✓         │    │ device_token:        │  ▛▀▀▜ ▄▀▄ ▛▀▜  │
  │ device_token: a1b2c3d4…    │    │   a1b2c3d4…          │  ▌▄▄▐ █▄█ ▌▄▐  │
  │                            │    │                      │  ▜▄▄▛ ▀▄▀ ▜▄▛  │
  │ 提示：解绑删 device_token  │    │ 提示…                │   qrcode_term    │
  └────────────────────────────┘    └──────────────────────┴───────────────┘
                                                    ▲
                                   半块字符 █▀▄ 渲染，一行塞两个模块

  按 Enter：临时离开 TUI -> 原生终端打印「带 user_code 的一次性二维码」
            -> 手机扫码 GitHub/Gitee 授权 -> 轮询落盘 device_token -> 回到本页
```

> 说明：账号页里这枚是**固定入口示意图**（不会被 TUI 边框裁切）；真正带验证码、  
> 能扫的二维码仍由 `run_login_flow` 在原生终端渲染。两处不冲突。

---

## 6. T2 · CLI 颜色 / NO_COLOR / 进度条（简笔画）

```
  $ mox-proot-distro install alpine
  [安装] [>                       ] 0%    从国内镜像下载：…
  [安装] [========>               ] 35%   校验 SHA-256
  [安装] [==================>     ] 78%   解压 rootfs
  [安装] [========================] 100%  安装完成：alpine
          ▲ 24 格彩条，NO_COLOR 时退化成 [35%] 纯文本
```

```
  NO_COLOR=1 或 非 TTY（管道/CI）  ->  全部关色，输出纯文本
  仅 stderr 为终端且无 NO_COLOR     ->  C_CYAN/GREEN/YELLOW/RED/BOLD 上色
  安装失败                          ->  红色 "proot-distro: 安装失败:"
```

---

## 7. T3 · 版本列表与选版本号（简笔画）

```
  $ mox-proot-distro versions ubuntu
  发行版 ubuntu 可选版本：
    24.04 LTS (base)         Ubuntu（默认）   <-- 绿色标注
    22.04 LTS                Ubuntu
    20.04 LTS                Ubuntu

  $ mox-proot-distro install ubuntu --version "22.04 LTS"
     ... 校验命中清单 -> 安装 -> 回显「已选版本：22.04 LTS（默认：24.04 LTS）」

  $ mox-proot-distro install alpine --version 9.99
  proot-distro: 版本 9.99 不在 alpine 的可选清单
                （3.20 (minirootfs) / 3.19 (minirootfs) / edge）
```

| 发行版    | 可选版本                                         |
| :----- | :------------------------------------------- |
| ubuntu | 24.04 LTS (base) · 22.04 LTS · 20.04 LTS     |
| debian | 12 (bookworm) · 11 (bullseye)                |
| kali   | rolling (nethunter minimal)                  |
| alpine | 3.20 (minirootfs) · 3.19 (minirootfs) · edge |

---

## 8. T5 · 安装脚本精修（简笔画）

```
  $ curl … install-mox-proot.sh | bash

  _  _ ____ _  _                       <-- 紧凑品牌字标（4 行，手机端不折行）
  |\/| |  |  \/
  |  | |__| _/\_
    mox-proot
    多架构容器引擎 · 一键安装

  ==> 下载 mox-proot-linux-amd64（proot-latest）…
  ==> 校验 sha256…
  ✔ /…/bin/mox-proot 就绪                <-- 收尾自检（T5 新增）
  ✔ /…/bin/mox-proot-distro 就绪
  ✔ /…/bin/mox 就绪
  ✔ 引擎自检通过：mox-proot 0.9.0
  ... 用法速查（含三件套 / versions / --version 提示）
```

> 精修保住了原有的彩色 `log/warn/die`，并补上 `NO_COLOR` 约定（非 TTY 自动关色，  
> |  |  |  
> | --- | --- |  
> `curl | bash` 重定向时不会输出乱码转义）。`mox` 控制台脚本与引擎 `console.rs`  
> 同源，同样带时间问候 + 古诗 + 彩蛋。



---

## 9. 成果清单与验证


| #   | 任务                                           | 关键文件                                           |  状态 |
| :-- | :------------------------------------------- | :--------------------------------------------- | :-: |
| 450 | 惊喜三件套（时间上下文 / 古诗 / 彩蛋）                       | `delight.rs`(新)、`mod.rs`、`console.rs`、`cli.rs` |  ✅  |
| 451 | T2 CLI 颜色 / NO_COLOR / 进度条                   | `cli.rs`                                       |  ✅  |
| 452 | T3 版本列表与选版本号                                 | `distro.rs`、`cli.rs`                           |  ✅  |
| 453 | T4 半截 TUI 升级（欢迎屏/Gauge/Spinner/Scrollbar/QR） | `tui.rs`                                       |  ✅  |
| 454 | T5 安装脚本精修                                    | `scripts/install-mox-proot.sh`                 |  ✅  |
| 455 | T6 README 修复（macOS 徽章 / 重复行 / 二进制数）          | `README.md`、`README.en.md`、`assets/logo.svg`   |  ✅  |
| 456 | 成果文档（本篇）                                     | `docs/tui-showcase.md`                         |  ✅  |
| 457 | T7 CI 等价校验 + 提交                              | 全仓                                             |  ✅  |
| 附   | 修复改名遗留的 404：README 一键安装短命令仍指向 `moxsh-proot`  | `README.md`、`README.en.md`                     |  ✅  |
| 附   | 修复 `mox` 脚本两端漂移，改为字标内嵌 + 单测盯防                | `console.rs`、`install-mox-proot.sh`            |  ✅  |

**验证命令（全部通过）**

```bash
cargo check                      # 核心库（无 tui）
cargo check --features tui       # TUI 三二进制
cargo test --lib                 # 238 通过（含彩蛋/时间/古诗/版本单测）
cargo test --features tui --lib  # 251 通过（含 5 个 TUI 专项）
cargo +1.86.0 clippy --all-targets -- -D warnings   # CI 等价、干净目录、零警告
cargo +1.86.0 test               # CI 等价：238 + 6 集成
go vet ./...                     # Go 安装器
sh -n scripts/bootstrap.sh && sh -n scripts/install-mox-proot.sh
bash scripts/gen-shortcmd.sh --check     # 短命令同步（顺手修了改名 404）
bash scripts/test-source-pick.sh         # 分流回归 12 用例
bash scripts/test-console-sync.sh        # mox 脚本两端一致
python3 -m py_compile py/runtime/*.py py/verify/*.py
```

> 本文件是**示意草图**：框线、间距、二维码均为手绘近似，用于说明布局与交互，  
> 不是逐像素截图。真实渲染由 `ratatui` 完成。
