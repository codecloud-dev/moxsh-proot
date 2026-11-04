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
