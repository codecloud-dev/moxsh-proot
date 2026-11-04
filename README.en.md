> **🌐 Canonical repository is on GitHub** — [`codecloud-dev/mox-proot`](https://mox-site.pages.dev/go?r=mox-proot) is the single source of truth: please file all issues and pull requests there. The [Gitee mirror](https://gitee.com/thousand-code-crafts/mox-proot) is a read-only copy synced hourly for faster access in mainland China.

<p align="center">
  <img src="assets/logo.svg" width="128" alt="mox-proot liquid-glass logo">
</p>

<h1 align="center">mox-proot</h1>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/codecloud-dev/mox-proot/rust-check.yml?branch=main&label=CI%20Build&color=8a7bff" alt="CI Build">
  <img src="https://img.shields.io/github/v/release/codecloud-dev/mox-proot?label=Latest&color=37d5d3" alt="Latest">
  <img src="https://img.shields.io/github/stars/codecloud-dev/mox-proot?style=social" alt="GitHub Stars">
  <img src="https://img.shields.io/badge/version-0.9.0-8a7bff" alt="version">
  <img src="https://img.shields.io/badge/license-GPL--3.0-37d5d3" alt="license">
  <img src="https://img.shields.io/badge/Core-Rust-000?logo=rust&logoColor=white" alt="Rust core">
  <img src="https://img.shields.io/badge/Installer-Go-00ADD8?logo=go&logoColor=white" alt="Go installer">
  <img src="https://img.shields.io/badge/Hooks-Python%20%2F%20Lua-3776AB?logo=python&logoColor=white" alt="Hooks">
  <img src="https://img.shields.io/badge/Bridge-C-A8B9CC?logo=c&logoColor=white" alt="C bridge">
  <img src="https://img.shields.io/badge/Platform-Linux%20%2F%20Termux(Android)-3DDC84?logo=android&logoColor=white" alt="Linux / Termux(Android)">
</p>


> No root, no privileges, no system partition touched — run a full Linux distro inside Termux / Linux. A from-scratch Rust ptrace path-translation engine, upstream-PRoot-compatible command surface, one-line install, works out of the box.

<p align="center"><a href="README.md">中文</a> · <a href="README.en.md">English</a></p>

<p align="center"><b>⭐ If mox-proot is useful to you, please give it a <a href="https://mox-site.pages.dev/go?r=mox-proot">star</a> — it helps more people get a smooth Linux on their phone and in containers!</b></p>

## 🎁 Delight trio (time context / poem / easter egg)

Entering `mox` with no args (or the TUI welcome screen) reads the **local timezone**
and maps the day into `dawn / noon / dusk / night`, then prints a matching greeting
plus a classical Chinese poem. A hidden easter-egg code is tucked away too.
Fully local, zero dependency, zero network — single source in `src/proot/delight.rs`.

| Phase | Hours | Greeting | Poem |
| :--: | :--: | :--- | :--- |
| Dawn | 05–10 | Good morning, a new day starts in a container | Zheng Xie, *After Snow in the Mountains* |
| Noon | 11–16 | Good afternoon, grab some water | Li Shen, *Sympathy for the Farmers* |
| Dusk | 17–22 | Good evening, a fine night to tinker | Li Shangyin, *On the Pleasure Plateau* |
| Night | 23–04 | It's late, don't stay up too long | Meng Haoran, *Spring Dawn* |

```bash
mox                       # no args: banner + time greeting + poem
mox egg                   # egg code: egg / 彩蛋 / konami / 蛋
mox-proot-distro time     # print the phase + greeting + poem standalone
MOX_EGG=1 mox-proot-tui   # env var also names the egg
```

## 🖥️ TUI console & version pick

```bash
mox-proot-tui                              # six-tab TUI (welcome / Gauge / QR)
mox-proot-distro versions ubuntu           # list versions: 24.04 LTS / 22.04 LTS / 20.04 LTS
mox-proot-distro install alpine --version "3.19 (minirootfs)"
```

Six tabs: **Distros / Shortlinks / Recipes / Logs / Wizard / Account**. A welcome
screen (banner + time greeting + poem) greets you first; installs run on a
background thread with a live Gauge + Spinner, lists have scrollbars, and the
account tab embeds a QR. Sketch-style ASCII diagrams of every panel live in
[docs/tui-showcase.md](docs/tui-showcase.md).

## 🐛 Welcome to roast me

> This is an early-stage project — **bugs exist, and probably plenty of them.** I'm not pretending it's perfect.
> Every pitfall you hit and every gripe you have is a chance to help make it better.

- 💥 Crashed / won't run / wrong results? → [File a bug report](https://mox-site.pages.dev/go?r=mox-proot&p=issues)
- 💡 Want a feature? → [Open a feature request](https://mox-site.pages.dev/go?r=mox-proot&p=issues)
- 🗯️ Just want to rant or nitpick? → Issues are welcome too, label it whatever 😄

I read every issue and fix what I can, fast. Let's grow this from "runs" to "delightful" 💪

<p align="center">💛 Found it useful? <a href="https://afdian.com/a/cloudharbor">Buy the author a coffee on AfDian</a> — CN payments (WeChat / Alipay) supported, the biggest encouragement for an indie dev.</p>

<details>

<summary>Contents</summary>

- [MoX family](#mox-family)
- [Delight trio](#-delight-trio-time-context--poem--easter-egg)
- [TUI console & version pick](#-tui-console--version-pick)
- [Features](#features)
- [How it works](#how-it-works)
- [Getting started](#getting-started)
- [Repo layout](#repo-layout)
- [Relationship with upstream PRoot](#relationship-with-upstream-proot)
- [Docs](#docs)
- [Get involved](#get-involved)
- [Support us](#support-us)
- [License](#license)
- [AI assistance statement](#ai-assistance-statement)

</details>

---

## 🌟 MoX family

mox-proot is the infrastructure piece of the **MoX tool series**: the "PRoot distro" capability inside the mox-proot terminal — the engine itself lives in this repo. One visual language, bringing desktop-grade tools into your pocket.

| Product | Status | One-liner |
| :---: | :---: | --- |
| **mox-proot** | Released | This repo: from-scratch Rust PRoot container engine — the distro core of the Android liquid-glass terminal / Termux ecosystem, also standalone |
| **mox-site** | Released | The series' official portal ([site](https://codecloud-dev.github.io/mox-site/)) |

### 🗺️ Roadmap (0.2.0 in progress)

| Milestone | Contents | Status |
| :--- | :--- | :--- |
| 0.2.0 | `mx:` shortlinks (`mx:alpine` installs on demand), on-disk launchers (type `kali` to enter), in-container Chinese environment (locale/fonts/IME), in-container console, base64 short command + CN/global auto-routing | In progress |
| 0.3.0 | Recipe system (`mx:xfce` one-command desktops: XFCE/LXQt/KDE/MATE + dual VNC channels) | Planned |
| 0.4.0 | TUI console (ratatui full-featured panel) | Implemented (v0.9.0 via `mox-proot-tui`) |
| 1.0.0 | First stable release with shortlinks/recipes/TUI all in | Planned |

Full decision log: [docs/roadmap.md](docs/roadmap.md).

---

## ⚙️ Features

| Dimension | Notes |
| :--- | --- |
| **Rust ptrace engine** | Path translation, multi-process tree tracing, fake root — all in Rust; memory-safe, no external runtime |
| **Upstream-compatible surface** | `-r/-0/-b/-w/-R/-e` and the `--` split stay the same; zero migration for veteran users. Unsupported options fail loudly instead of pretending |
| **Three binaries** | `mox-proot` (single-command container) + `mox-proot-distro` (install / login / backup / restore for the big four distros) + `mox-proot-tui` (ratatui TUI console) |
| **Explicit loader invocation** | Dynamic ELF automatically runs through the guest's own loader (same trick as qemu-user); covered at initial exec and at runtime execve |
| **Full symlink chain** | Absolute-target symlinks (Alpine applet style) resolve automatically at exec / stat / openat — even a raw, unnormalized tar-extracted rootfs runs |
| **fake root** | uid/gid show as 0; chown/setuid succeed in disguise — no real root, no system partition touched |
| **Network stack** | Unified DNS + dual-stack IPv6 probing + egress boundary report (`mox-proot-distro net`) |
| **Hook runtime** | post-install / pre-login lifecycle, Python / Lua dual interpreters, drop-in container scripts |
| **One-key recipes** | Declarative TOML + shell escape hatch, `recipe list/show/apply` one-key desktop/VNC setups |
| **One-line install** | bash probe layer (`curl \| bash`, zero deps) + Go installer (install/update/probe), two layers one source |

---

## 🔧 How it works

mox-proot is neither a VM nor an emulator — like upstream PRoot, it does
**ptrace path translation**: every filesystem syscall of the child process is
intercepted, and guest-side paths are rewritten in registers to real paths
inside the host rootfs. The effects of `chroot`, `mount --bind` and
`binfmt_misc` follow from that — **without any privilege**.

The engine is written from scratch in Rust (not a wrapper around the upstream
C version), and on top of that it solves real mobile-world problems:

- **Explicit loader invocation**: when the kernel execs an ELF with `PT_INTERP`,
  it looks for the interpreter path in the **host** namespace, which is
  guaranteed ENOENT. At exec-stop the engine rewrites the command to
  `execve(loader, [loader, prog, args...])` (same trick as qemu-user), so the
  guest's own musl/glibc loader takes the stage and every subsequent openat
  lands on the translation chain;
- **Absolute symlinks**: for links like `/bin/ls -> /bin/busybox` whose target
  is absolute, the kernel resolves the target literally in host space. The
  engine walks every component at exec / stat / openat and resolves links to
  real paths inside the rootfs — so a plain, unnormalized tar-extracted rootfs
  still runs;
- **Process trees**: TRACEFORK/VFORK/CLONE are all traced, so fork-heavy loads
  like `apt/dpkg` never hang; long paths are re-pointed through the tracee's
  own scratch region (loader-mem equivalent).

---

## 🚀 Getting started

### 🔹 One-line install (vanilla Termux / Linux)

**Option 1: short command** (recommended) — the entire bootstrap is embedded as
one base64 blob: copy, paste, run. No domain to remember; falls back from
GitHub raw to Gitee raw automatically (no mirror hunting in China):

<details>
<summary>Expand short command (~1.5 KB — open to copy the whole line)</summary>

```bash
echo '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' | base64 -d | gzip -d | sh
```

</details>

**Option 2: standard command**:

```bash
curl -fsSL https://raw.githubusercontent.com/codecloud-dev/mox-proot/main/scripts/install-mox-proot.sh | bash
```

**China-friendly mirror** (Gitee raw, direct connection):

```bash
curl -fsSL https://gitee.com/thousand-code-crafts/mox-proot/raw/main/scripts/install-mox-proot.sh | bash
```

Zero dependencies (just curl + coreutils). The binary source is auto-probed
(China / overseas) and every file is sha256-verified; two commands land on
disk: `mox-proot` and `mox-proot-distro`.

Ready to use:

```bash
mox-proot -R ~/ubuntu -0 -- bash        # recommended bind set + fake root into the distro
mox-proot --help                        # full usage
```

For a richer install/update experience, use the Go installer (Release asset
`mox-proot-installer-<goos>-<arch>`, subcommands install / update / probe):

```bash
mox-proot-installer install
mox-proot-installer update
```

### 🔹 Install a distro

```bash
mox-proot-distro install alpine     # Ubuntu / Debian / Kali / Alpine
mox-proot-distro login alpine       # drop into a login shell
```

Or launch by short link (auto-installs on first run, drops you straight in):

```bash
mox-proot-distro launch mx:alpine
```

One-shot setup (data root + distro + launchers + a guided tour; re-runs
only fill in what's missing):

```bash
mox-proot-distro wizard                    # defaults: ~/.mox-proot + alpine
mox-proot-distro wizard --root /sd/data --distro ubuntu   # customize
```

**Chinese locale works out of the box**: on login the engine injects
`LANG`/`TERM` (your host locale passes through; falls back to `C.UTF-8`,
so no mojibake). For full Chinese UI: install the `locales` package inside
the container, run `locale-gen zh_CN.UTF-8`, then edit `etc/mox-proot/env`
inside the rootfs and set `LANG=zh_CN.UTF-8`. Inspect the effective
environment any time:

```bash
mox-proot-distro env alpine
```

### 🔹 In-container console

Inside any container, the fixed command `mox` (not customizable) brings up
the management console — bind-mounted reverse-tunneling back to the host,
so installing distros, checking the network, or inspecting the environment
all works without leaving the container:

```bash
# inside the container (any distro):
mox                 # management console (list / install / remove / net / env)
mox install kali    # install another distro from inside (lands in host MOX_ROOT)
mox net             # network boundary report
```

Nesting is not allowed (ptrace can't stack proot on proot), so
`mox login/shell/launch` inside a container asks you to exit first.
Set `MOX_PROOT_CONSOLE=0` to disable console injection entirely.

### 🔹 Requirements

- Linux or Termux (Android 9+, aarch64 / x86_64; 32-bit ARM needs a source build)
- ~50 MB free space (engine + one minirootfs)

> Platform note: mox-proot is a **Linux / Android (Termux) ptrace path-translation
> engine** — it runs containers only on a Linux kernel (macOS / Windows have no
> ptrace equivalent). Pure network clients such as device-code login
> (`mox-proot login`) work anywhere with curl, but **the container engine itself
> does not support macOS / Windows**.

### 🔹 Build from source

```bash
git clone https://github.com/codecloud-dev/mox-proot.git
cd mox-proot
cargo build --release
```

Full build / test / verify commands live in [docs/usage.md](docs/usage.md).

---

## 📦 Repo layout

One engine, five languages, each in its lane:

| Dir | Language | Responsibility |
| :--- | :--- | :--- |
| `src/proot/` | Rust | Engine core: ptrace translation, ELF/exec chain, distro, network stack, hook discovery |
| `src/bin/` | Rust | Three standalone binaries: `mox-proot` / `mox-proot-distro` / `mox-proot-tui` |
| `go/` | Go | Installer + downloader (install/update/probe), dual target Linux + Termux |
| `py/runtime/` | Python | In-container hooks (supply check / login snapshot); runnable standalone on the host |
| `py/verify/` | Python | Three-layer verification scripts (unit + hook direct-run + real rootfs e2e) |
| `lua/` | Lua | Declarative config (`proot.lua`) + hooks |
| `c/` | C | Android JNI bridge (`proot_bridge.c`) |
| `scripts/` | bash | One-line install script (probe layer) |

---

## 🔗 Relationship with upstream PRoot

- **Behavior**: aligned. Common options (`-r/-0/-b/-w/-R/-S/-e`, the `--` split)
  and the proot-distro subcommand surface (list/install/login/backup/restore)
  stay the same; zero migration for veteran users;
- **Code**: AI-assisted, clean-room rewrite. The engine is reimplemented
  entirely in Rust (ptrace stop machine, path translation, ELF parsing, distro
  management); upstream public docs and source were studied during development
  to align the behavior contract, but **no code was copied** and we don't
  depend on patching upstream;
- **Ecosystem**: compatible. It still runs the official rootfs images (Alpine
  minirootfs etc.) and installs the same way; this repo adds the Rust engine,
  one-line install and a five-language hook system on top.

---

## 📖 Docs

- [Usage manual](docs/usage.md) — option surface, subcommands, hooks, build & verify
- [TUI sketch gallery](docs/tui-showcase.md) — ASCII sketches of all six tabs, the welcome screen & the delight trio
- [Roadmap decisions](docs/roadmap.md) — milestones & design trade-offs

---

## 🤝 Get involved

Found a bug or want a feature? Open an [Issue](../../issues); to contribute code, just open a Pull Request.

---

## 💖 Support us

If this project is useful to you, **a Star, a follow, or a share** is the biggest encouragement for a tiny indie project — and helps more people run Linux in a container.

- Star it on GitHub: hit the Star button at the top-right of the repo
- Follow the repo for first-notice updates
- Share it with friends, groups, or any developer tortured by root privileges

All forms of contribution are welcome: bugs, ideas, code, docs, translations. Indie dev + AI collaboration, but every direction is decided by a human.

---

## 📜 License

[GPL-3.0](LICENSE)

## 🤖 AI assistance statement

This project (all code, docs and site pages) is led by the developer **codecloud-dev**, with **AI-assisted code generation**: architecture decisions, requirements and acceptance are done by humans; implementation and docs are AI-collaborative and reviewed/edited by humans.
