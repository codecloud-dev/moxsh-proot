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

## 5. 账号页（设备绑定 + 真·后端二维码）

```
  窄屏（<72 列）                    宽屏（≥72 列）：右侧内嵌二维码
  ┌────────────────────────────┐   ┌────────────────────────┬──────────────────┐
  │ 账号（设备绑定·扫码登录）  │   │ 绑定状态：未绑定       │ 扫码绑定（真码） │
  │ 按 l 申请设备码（真实请求）│   │ 验证码：A7K2-9FQ1      │  ▛▀▀▜ ▄▀▄ ▛▀▜   │
  │ 有效期 600s · 按 r 检查授权│   │ 有效期 600s · 按 r 检查│  ▌▄▄▐ █▄█ ▌▄▐   │
  │ 后端已签发一次性码，可直接扫│   │ 后端：mox-site.pages.dev│  ▜▄▄▛ ▀▄▀ ▜▄▛   │
  └────────────────────────────┘   └────────────────────────┴──────────────────┘
                                                  ▲
                     半块字符 █▀▄ 渲染；内容是后端返回的
                     verification_uri_complete（带一次性 code），
                     不再是扫了也没有 user_code 的固定「示意」图

  按 l    ：POST /device/code → 拿 user_code + 一次性链接 → 右侧换成真二维码
  按 r    ：POST /device/token 轮询 → 手机确认后换 device_token 落盘 → 「已绑定」
  按 Enter：临时离开 TUI → 原生终端打印全尺寸二维码（不被盒子裁切），流程同上
```

> **后端链路**：终端 → `https://mox-site.pages.dev`（官网 Pages 同域反代）
> → `https://mox-id.moxsh.app`（Worker 真实域），`/activate` 复用
> GitHub / Gitee 双 OAuth。旧的 `mox-id.mox-proot.app` 是**从未部署的占位域名**，
> 已清除；设备码模块也从 `tui` feature 门禁放出，默认发布的两个二进制
> 现在真的带 `login` 子命令（无 tui 时二维码降级为「链接 + 验证码」）。

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
| 附   | 仓库保持旧名 `moxsh-proot`：全部仓库 URL 坐标回归（代码内命名不动）| 脚本 / README / 徽章 / `go.mod` / `Cargo.toml`  |  ✅  |
| 附   | 接上真实后端：`backend.rs` 单一来源 + 设备码解门禁 + TUI 真设备码      | `backend.rs`(新)、`device_code.rs`、`tui.rs`      |  ✅  |
| 附   | 安装自检双向修复：假阴性误杀好二进制 / 假阳性放过 glibc 失败           | `scripts/install-mox-proot.sh`                   |  ✅  |
| 附   | 移除 `hooks/lua` 空 mkdir+rmdir；关闭实测 404 的 `WEBSITE_DL` 兜底       | `scripts/install-mox-proot.sh`                   |  ✅  |
| 附   | 修复 `mox` 脚本两端漂移，改为字标内嵌 + 单测盯防                | `console.rs`、`install-mox-proot.sh`            |  ✅  |

**验证命令（全部通过）**

```bash
cargo check                      # 核心库（无 tui）
cargo check --features tui       # TUI 三二进制
cargo test --lib                 # 247 通过（含彩蛋/时间/古诗/后端契约单测）
cargo test --features tui --lib  # 256 通过（含 TUI 专项 + 真设备码 URI 测试）
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

---

## 10. 本轮追加：真实后端接线 + 安装自检事故复盘

### 10.1 为什么「后端一直没接上」

```
  【之前】后端 mox-id 早就部署了，客户端三条线却各自指着不同地方：

    device_code  ──► https://mox-id.mox-proot.app      （占位域名，从未部署）
    fan 页面     ──► https://codecloud-dev.github.io/mox-site/fan.html（旧 Pages）
    fan 验签     ──► https://mox-id.codecloud.workers.dev/fan/verify （国内不稳）
    redeem 校验  ──► https://mox-site.pages.dev/entitlement/check    （✅ 这个是对的）

  更致命的一层：device_code / qrcode_term / tui 全被 #[cfg(feature="tui")] 门控，
  而发布用 `cargo build --bins`（不带 tui）→ 默认两个二进制压根没有 login 命令。
  → 后端写得再完整，正式产物里也没有一条路能走到它。

  【现在】
    backend.rs 单一来源 ──► https://mox-site.pages.dev  （官网同域反代）
                                │  functions/[[path]].js
                                ▼
                          https://mox-id.moxsh.app      （Worker 真实域）
                                │ 兜底
                                ▼
                    https://mox-id.codecloud.workers.dev

    device_code 只依赖 curl + 手写 JSON → 移出 feature 门禁 → 默认二进制带 login
    TUI 账号页按 l 申请真设备码、按 r 轮询落盘，二维码不再是「示意」
```

### 10.2 安装自检的两个方向都是坏的

```
  旧实现：help_out="$("$bin" --help 2>&1 || true)"
          printf '%s\n' "$help_out" | grep -q "moxsh-proot" || die ...

  假阴性（真实安装测试撞到）：
    "$bin" --help  →  「proot-distro（moxsh 兼容实现）」
    grep "moxsh-proot" → 0 命中 → die
    二进制完全正常，却报安装失败。← 你这次遇到的

  假阳性（上一轮测试报告第 7 条）：
    glibc 加载失败 → 加载器报错行以 "mox-proot:" 开头
    || true 吞掉退出码 + grep 命中命令名 → 判成「自检通过」
    → 装完提示成功，一跑就 GLIBC_2.39 not found

  现在：① 先判退出码（非 0 直接 die，能抓动态链接失败）
        ② 内容只断言「非空 + 用法/Usage/proot-distro/proot 」，与品牌名解耦
        ③ 失败回显真实输出，给出 musl 静态版的下一步
  实测：真二进制 PASS / 旧帮助文本 PASS / glibc 报错文本不会伪装 PASS
```

### 10.3 顺手清掉的两个旧问题

- `hooks/lua` 建了又 `rmdir`：引擎 `discover_hooks` 用 `read_dir` **只扫一层**，
  钩子必须落在 `hooks/` 根下，移进子目录反而永远加载不到 → 删掉空操作并写明原因。
- `WEBSITE_DL=https://mox-site.pages.dev/dl` 实测 404，旧版每个资产白等 12s 超时
  → 默认关闭，只有显式设置 `MOX_PROOT_WEBSITE_DL` 才尝试。

### 10.4 上传与发版

本地提交 `77d8f43` 已 push 到 GitHub / Gitee 的**旧名仓库** `moxsh-proot`。
push 触发 `release-latest.yml` 滚动发版，会按 `release.yml` 里既有的
`ubuntu-22.04`（glibc 2.35）源码构建 + musl 静态 target 重新生成资产，
之前测出的 GLIBC 2.39 基线与「musl 资产缺失」问题随之刷新。
