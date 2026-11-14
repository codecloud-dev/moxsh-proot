# 更新日志 · moxsh-proot

> 格式基于 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循[语义化版本](https://semver.org/lang/zh-CN/)。

本文件记录**显著的**用户可见变更。琐碎的排版、内部重构、CI 微调不单独列条目；完整逐笔记录请看 [提交历史](../../commits/main) 与 [Releases](../../releases)。

## [Unreleased]

> 尚未发布的改动合并后写在这里，发布时归入对应版本段落。

### 新增 / 修复 / 变更

- _（暂无待发布条目）_

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
