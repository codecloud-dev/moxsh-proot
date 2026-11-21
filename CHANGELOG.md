# 更新日志 · moxsh-proot

> 格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

本文件记录**显著的**用户可见变更。琐碎的排版、内部重构、CI 微调不单独列条目；完整逐笔记录请看 [提交历史](../../commits/main) 与 [Releases](../../releases)。

## [0.9.3] - 2026-11-21

### 修复

- **`--help` 补齐用户真正会找的入口**：帮助文本现在列出 `help`、`--version`、`login`、`tui`、`mx:<名字>` 及可直接复制的示例；新增守门测试防止后续改动又把入口藏起来。
## [Unreleased]

> 尚未发布的改动合并后写在这里，发布时归入对应版本段落。

### 新增 / 修复 / 变更

- _（暂无待发布条目）_

## [0.9.2] - 2026-11-19

> 本轮为自研仓库深度升级：真跑+读代码挖出 5 个真实问题（含 1 个 CI 长期红、
> 3 个静默失效/错误处理的真 bug、1 个网络依赖假绿测试），全部带回归测试。
> 发版/推送由主流程统一执行，本版本号为源码声明版本。

### 修复

- **docs-check CI 长期红：README 内嵌安装短命令与 bootstrap 失同步**。上一笔提交把
  `scripts/bootstrap.sh` 的 Gitee 路径从作废的 `thousand-code-crafts` 改成 `codecloud-dev`，
  却没重生成 README 里的 base64 短命令——`scripts/gen-shortcmd.sh --check` 解码后逐字节
  比对失败，CI 的 docs-check job 一直红，用户从 README 复制的短命令仍走旧路径。
  已重新生成并替换 `README.md`/`README.en.md` 两处短命令，解码结果与 bootstrap 完全一致。
- **Go 安装器断点续传永久卡在 HTTP 416**（可靠性）。`.part` 已是完整文件（或同长度
  陈旧文件）时服务器对 `Range` 回 416，旧实现在打开目标文件前就直接返回错误重试，
  所有通道永久失败——与代码注释「含 200 覆盖 416 场景」自相矛盾。现 416 时去掉
  `Range` 重新完整 GET 并做全量 SHA-256 校验。新增 `TestDownloadRecoversFrom416`
  （修复前 FAIL：HTTP 416；修复后 PASS）。
- **Go 安装器校验 fail-open**（供应链安全）。主通道 `<asset>.sha256` 暂时 404/不可达时，
  旧实现把期望值置空，对后续任意通道「零校验」放行——被污染镜像可静默装进系统。
  现改为逐通道获取本通道校验值，拿不到的通道直接跳过（fail-closed）。新增
  `TestDownloadFailsClosedWithoutChecksum`（修复前 FAIL：静默接受污染内容；修复后 PASS）。
- **Rust `parse_sha256sums` 不支持 sha256sum 二进制模式**（rootfs 校验静默跳过）。
  文档声明支持 `<hex> *filename`，但 `split_whitespace` 把 `*` 粘在文件名 token 上导致
  匹配失败；Ubuntu 官方 `SHA256SUMS` 正是 `hex *file` 格式，其校验值长期取不到。
  现剥离文件名前导 `*`。新增 `sha256sums_accepts_binary_star_format`（修复前 FAIL，修复后 PASS）。
- **网络依赖假绿测试**：`fetch_expected_sha256_https_returns_none` 用「联网后必返回 None」
  做断言，既依赖外网（出口可达时 Ubuntu 二进制清单被正确解析出校验值，测试反而误判失败），
  又与「curl 支持 HTTPS、校验真正生效」的实现矛盾。改为纯本地断言所有内置 checksum URL
  均为 https，不联网、不假绿。
- **当前 stable Rust（1.93）`clippy -D warnings` 新增 17 条错误会让 CI 红**：手写位循环、取模判倍
  数、`iter().any` 代替 `contains`、`1usize as *mut c_void` 造悬垂指针、`PathBuf::from()` 临时对象
  比较。全部按语义等价且兼容项目 MSRV 的方式改写（位旋转用 `rotate_right`、64 的倍数用位掩码、
  `NonNull::dangling()`、`Path::new()` 比较），不用 `#[allow]` 遮羞。

### 新增（基准）

- 新增 `go/internal/fetch/fetch_bench_test.go` 真实基准（本机环回，非公网，仅看量级）：
  - `BenchmarkParseChecksumText`：**136.1 ns/op（683.56 MB/s）**；
  - `BenchmarkDownloadLocal`（4 MiB 端到端，含流式 SHA-256）：**3.62 ms/op（1160.02 MB/s）**。
  - 数值随机器负载波动（`AMD EPYC 9K65`），仅用于回归性能量级，`go test ./...` 不执行基准。

> 验证（CI 钉死的 Rust 1.86.0 + Go 1.21.4）：`cargo test` 303 passed / 0 failed（含 lib 295 +
> 集成 2+1+5），`cargo clippy --all-targets -- -D warnings` RC=0；`go vet ./...` RC=0、
> `gofmt -l` 空、`go test ./...` RC=0；`gen-shortcmd.sh --check` / `test-source-pick.sh` /
> `test-console-sync.sh` / 两个 `sh -n` 全部 RC=0。主流程另用当前 stable Rust 1.93 复跑：
> `cargo test` RC=0、`cargo clippy --all-targets -- -D warnings` RC=0（修清 17 条新 lint 后）。

## [0.9.1] - 2026-11-14

### 变更

- **依赖升级（兼容最新）**：构建依赖 `cc` 1.11.1 → 1.11.2；传递依赖升级到兼容最新——
  `crc32fast` 1.5.5 / `errno` 0.3.15 / `itoa` 1.0.19 / `mio` 1.2.5 / `rustix` 1.1.6 /
  `smallvec` 1.16.5 / `syn` 3.0.10 / `xattr` 1.7.0（均经 `cargo update` 解析，受 `Cargo.lock` 钉死）。
- **修复 `mox-proot-tui` 在钉死工具链 1.86.0 下无法编译**：启用 `tui` feature 时依赖树
  解析到要求 rustc 1.88.0 的 `image` 0.25.10 / `instability` 0.3.14（→ `darling` 0.24.1），
  与项目钉死的 1.86.0 冲突。已在 `Cargo.lock` 中将二者钉到 1.86 兼容版本
  （`image` 0.25.9 / `instability` 0.3.10，darling 随降 0.20.11），使 TUI 二进制可在 1.86.0 构建。
- **TUI 模块 clippy 严格模式（-D warnings）清零**：移除 `tui.rs` / `qrcode_term.rs` 内与
  `mod.rs` 模块门控重复的 `#![cfg(feature = "tui")]` 属性；`App::new` 形参 `&PathBuf` → `&Path`
  （调用点自动协变，无需改动）；消除冗余闭包、无用 `format!`、恒真常量断言、`needless_borrow`
  等告警。

### 修复

- 随本版发布此前已在 `main` 合入的安装脚本 `tar`/`xz` 解压依赖自检（手册 §6.3），确保无解压
  工具时给出明确报错而非静默失败。

## 历史版本

历史变更请看 [Releases 页面](../../releases)；本文件自 2026-11-11 起统一维护，此前条目不再补录，避免与发布记录产生两个互相矛盾的版本来源。

---

**条目写法**：`### 新增` / `### 修复` / `### 变更` / `### 移除` / `### 安全`，每条一句话说清「用户能感知到什么变化」，涉及破坏性变更必须以 **`BREAKING`** 开头标出。
