> **🌐 Canonical repository is on GitHub** — [`codecloud-dev/moxsh-proot`](https://mox-site.pages.dev/go?r=moxsh-proot) is the single source of truth: please file all issues and pull requests there. The [Gitee mirror](https://gitee.com/thousand-code-crafts/moxsh-proot) is a read-only copy synced hourly for faster access in mainland China.

<p align="center">
  <img src="assets/logo.svg" width="128" alt="mox-proot liquid-glass logo">
</p>

<h1 align="center">mox-proot</h1>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/codecloud-dev/moxsh-proot/rust-check.yml?branch=main&label=CI%20Build&color=8a7bff" alt="CI Build">
  <img src="https://img.shields.io/github/v/release/codecloud-dev/moxsh-proot?label=Latest&color=37d5d3" alt="Latest">
  <img src="https://img.shields.io/github/stars/codecloud-dev/moxsh-proot?style=social" alt="GitHub Stars">
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

<p align="center"><b>⭐ If mox-proot is useful to you, please give it a <a href="https://mox-site.pages.dev/go?r=moxsh-proot">star</a> — it helps more people get a smooth Linux on their phone and in containers!</b></p>

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

- 💥 Crashed / won't run / wrong results? → [File a bug report](https://mox-site.pages.dev/go?r=moxsh-proot&p=issues)
- 💡 Want a feature? → [Open a feature request](https://mox-site.pages.dev/go?r=moxsh-proot&p=issues)
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
curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-mox-proot.sh | bash
```

**China-friendly mirror** (Gitee raw, direct connection):

```bash
curl -fsSL https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-mox-proot.sh | bash
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
git clone https://github.com/codecloud-dev/moxsh-proot.git
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
