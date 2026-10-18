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

<p align="center"><b>⭐ If moxsh-proot is useful to you, please give it a <a href="https://github.com/codecloud-dev/moxsh-proot">star</a> — it helps more people get a smooth Linux on their phone and in containers!</b></p>



## 🐛 Welcome to roast me

> This is an early-stage project — **bugs exist, and probably plenty of them.** I'm not pretending it's perfect.
> Every pitfall you hit and every gripe you have is a chance to help make it better.

- 💥 Crashed / won't run / wrong results? → [File a bug report](https://github.com/codecloud-dev/moxsh-proot/issues)
- 💡 Want a feature? → [Open a feature request](https://github.com/codecloud-dev/moxsh-proot/issues)
- 🗯️ Just want to rant or nitpick? → Issues are welcome too, label it whatever 😄

I read every issue and fix what I can, fast. Let's grow this from "runs" to "delightful" 💪

<p align="center">💛 Found it useful? <a href="https://afdian.com/a/cloudharbor">Buy the author a coffee on AfDian</a> — CN payments (WeChat / Alipay) supported, the biggest encouragement for an indie dev.</p>

<p align="center"><img src="assets/afdian-qr.jpg" width="200" alt="Afdian sponsorship QR code"></p>

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
| **moxbox** | Planned · tentative | File manager & system suite in the same glass UI; may or may not happen |
| **moxcode** | Planned · tentative | Lightweight mobile IDE: terminal + editor + preview; may or may not happen |

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

```bash
curl -fsSL https://raw.githubusercontent.com/codecloud-dev/moxsh-proot/main/scripts/install-moxsh-proot.sh | bash
```

Zero dependencies (just curl + coreutils); the binary is downloaded from GitHub
Releases with sha256 verification and lands as two commands:
`moxsh-proot` and `moxsh-proot-distro`.

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
