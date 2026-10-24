> **🌐 Canonical repository is on GitHub** — [`codecloud-dev/moxsh-proot`](https://mox-site.pages.dev/go?r=moxsh-proot) is the single source of truth: please file all issues and pull requests there. The [Gitee mirror](https://gitee.com/thousand-code-crafts/moxsh-proot) is a read-only copy synced hourly for faster access in mainland China.

<p align="center">
  <img src="assets/logo.svg" width="128" alt="moxsh liquid-glass logo">
</p>

<h1 align="center">moxsh-proot</h1>

<p align="center">
  <img src="https://img.shields.io/github/actions/workflow/status/codecloud-dev/moxsh-proot/rust-check.yml?branch=main&label=CI%20Build&color=8a7bff" alt="CI Build">
  <img src="https://img.shields.io/github/v/release/codecloud-dev/moxsh-proot?label=Latest&color=37d5d3" alt="Latest">
  <img src="https://img.shields.io/github/stars/codecloud-dev/moxsh-proot?style=social" alt="GitHub Stars">
  <img src="https://img.shields.io/badge/version-1.0.0-8a7bff" alt="version">
  <img src="https://img.shields.io/badge/license-GPL--3.0-37d5d3" alt="license">
  <img src="https://img.shields.io/badge/Core-Rust-000?logo=rust&logoColor=white" alt="Rust core">
  <img src="https://img.shields.io/badge/Installer-Go-00ADD8?logo=go&logoColor=white" alt="Go installer">
  <img src="https://img.shields.io/badge/Hooks-Python%20%2F%20Lua-3776AB?logo=python&logoColor=white" alt="Hooks">
  <img src="https://img.shields.io/badge/Bridge-C-A8B9CC?logo=c&logoColor=white" alt="C bridge">
  <img src="https://img.shields.io/badge/Platform-Linux%20%2F%20Termux-3DDC84?logo=android&logoColor=white" alt="Linux / Termux">
</p>


> No root, no privileges, no system partition touched — run a full Linux distro inside Termux / Linux. A from-scratch Rust ptrace path-translation engine, upstream-PRoot-compatible command surface, one-line install, works out of the box.

<p align="center"><a href="README.md">中文</a> · <a href="README.en.md">English</a></p>

<p align="center"><b>⭐ If moxsh-proot is useful to you, please give it a <a href="https://mox-site.pages.dev/go?r=moxsh-proot">star</a> — it helps more people get a smooth Linux on their phone and in containers!</b></p>



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

moxsh-proot is the infrastructure piece of the **MoX tool series**: the "PRoot distro" capability inside the moxsh terminal — the engine itself lives in this repo. One visual language, bringing desktop-grade tools into your pocket.

| Product | Status | One-liner |
| :---: | :---: | --- |
| **moxsh** | Released | Android liquid-glass terminal, Termux-compatible |
| **mox-site** | Released | The series' official portal ([site](https://codecloud-dev.github.io/mox-site/)) |
| **moxsh-proot** | Released | This repo: from-scratch Rust PRoot engine (the distro core of moxsh, standalone) |

### 🗺️ Roadmap (0.2.0 in progress)

| Milestone | Contents | Status |
| :--- | :--- | :--- |
| 0.2.0 | `mx:` shortlinks (`mx:alpine` installs on demand), on-disk launchers (type `kali` to enter), in-container Chinese environment (locale/fonts/IME), in-container console, base64 short command + CN/global auto-routing | In progress |
| 0.3.0 | Recipe system (`mx:xfce` one-command desktops: XFCE/LXQt/KDE/MATE + dual VNC channels) | Planned |
| 0.4.0 | TUI console (ratatui full-featured panel) | Planned |
| 1.0.0 | First stable release with shortlinks/recipes/TUI all in | Planned |

Full decision log: [docs/roadmap.md](docs/roadmap.md).

---

## ⚙️ Features

| Dimension | Notes |
| :--- | --- |
| **Rust ptrace engine** | Path translation, multi-process tree tracing, fake root — all in Rust; memory-safe, no external runtime |
| **Upstream-compatible surface** | `-r/-0/-b/-w/-R/-e` and the `--` split stay the same; zero migration for veteran users. Unsupported options fail loudly instead of pretending |
| **Two binaries** | `moxsh-proot` (single-command container) + `moxsh-proot-distro` (install / login / backup / restore for the big four distros) |
| **Explicit loader invocation** | Dynamic ELF automatically runs through the guest's own loader (same trick as qemu-user); covered at initial exec and at runtime execve |
| **Full symlink chain** | Absolute-target symlinks (Alpine applet style) resolve automatically at exec / stat / openat — even a raw, unnormalized tar-extracted rootfs runs |
| **fake root** | uid/gid show as 0; chown/setuid succeed in disguise — no real root, no system partition touched |
| **Network stack** | Unified DNS + dual-stack IPv6 probing + egress boundary report (`moxsh-proot-distro net`) |
| **Hook runtime** | post-install / pre-login lifecycle, Python / Lua dual interpreters, drop-in container scripts |
| **One-key recipes** | Declarative TOML + shell escape hatch, `recipe list/show/apply` one-key desktop/VNC setups |
| **One-line install** | bash probe layer (`curl \| bash`, zero deps) + Go installer (install/update/probe), two layers one source |

---

## 🔧 How it works

moxsh-proot is neither a VM nor an emulator — like upstream PRoot, it does
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
curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-moxsh-proot.sh | bash
```

**China-friendly mirror** (Gitee raw, direct connection):

```bash
curl -fsSL https://gitee.com/thousand-code-crafts/moxsh-proot/raw/main/scripts/install-moxsh-proot.sh | bash
```

Zero dependencies (just curl + coreutils). The binary source is auto-probed
(China / overseas) and every file is sha256-verified; two commands land on
disk: `moxsh-proot` and `moxsh-proot-distro`.

Ready to use:

```bash
moxsh-proot -R ~/ubuntu -0 -- bash        # recommended bind set + fake root into the distro
moxsh-proot --help                        # full usage
```

For a richer install/update experience, use the Go installer (Release asset
`moxsh-installer-<goos>-<arch>`, subcommands install / update / probe):

```bash
moxsh-installer install
moxsh-installer update
```

### 🔹 Install a distro

```bash
moxsh-proot-distro install alpine     # Ubuntu / Debian / Kali / Alpine
moxsh-proot-distro login alpine       # drop into a login shell
```

Or launch by short link (auto-installs on first run, drops you straight in):

```bash
moxsh-proot-distro launch mx:alpine
```

One-shot setup (data root + distro + launchers + a guided tour; re-runs
only fill in what's missing):

```bash
moxsh-proot-distro wizard                    # defaults: ~/.moxsh + alpine
moxsh-proot-distro wizard --root /sd/data --distro ubuntu   # customize
```

**Chinese locale works out of the box**: on login the engine injects
`LANG`/`TERM` (your host locale passes through; falls back to `C.UTF-8`,
so no mojibake). For full Chinese UI: install the `locales` package inside
the container, run `locale-gen zh_CN.UTF-8`, then edit `etc/moxsh/env`
inside the rootfs and set `LANG=zh_CN.UTF-8`. Inspect the effective
environment any time:

```bash
moxsh-proot-distro env alpine
```

### 🔹 In-container console

Inside any container, the fixed command `mox` (not customizable) brings up
the management console — bind-mounted reverse-tunneling back to the host,
so installing distros, checking the network, or inspecting the environment
all works without leaving the container:

```bash
# inside the container (any distro):
mox                 # management console (list / install / remove / net / env)
mox install kali    # install another distro from inside (lands in host MOXSH_ROOT)
mox net             # network boundary report
```

Nesting is not allowed (ptrace can't stack proot on proot), so
`mox login/shell/launch` inside a container asks you to exit first.
Set `MOXSH_PROOT_CONSOLE=0` to disable console injection entirely.

### 🔹 Requirements

- Linux or Termux (Android 9+, aarch64 / x86_64; 32-bit ARM needs a source build)
- ~50 MB free space (engine + one minirootfs)

### 🔹 Build from source

```bash
git clone https://github.com/codecloud-dev/moxsh-proot.git
cd moxsh-proot
cargo build --release
```

Full build / test / verify commands live in [docs/usage.md](docs/usage.md).

---

## 📦 Repo layout

One engine, five languages, each in its lane:

| Dir | Language | Responsibility |
| :--- | :--- | :--- |
| `src/proot/` | Rust | Engine core: ptrace translation, ELF/exec chain, distro, network stack, hook discovery |
| `src/bin/` | Rust | The two standalone binaries `moxsh-proot` / `moxsh-proot-distro` |
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
