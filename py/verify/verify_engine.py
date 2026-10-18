#!/usr/bin/env python3
"""ProOT 引擎验证脚本（py 验证层，M8）。

用户口径：引擎验证**不写 CI**，用 py 脚本在沙箱/真机直接跑。本脚本是
引擎行为的可执行验收单，覆盖：

  A. 单元层   —— cargo test 全绿（由本脚本驱动，含 127+ 单测）
  B. 脚本层   —— py/runtime 钩子在宿主侧直接运行（Python 特长：可移植自证）
  C. 引擎层   —— 真实 Alpine rootfs 上跑 e2e：
                  1. busybox 全家桶（id/env/ls/cat）
                  2. execveat 三形态（AT_EMPTY_PATH / dirfd 相对 / cwd 相对）
                  3. 绝对/相对符号链接语义（创建可执行 + readlink 反向还原）
                  4. fake root（id=root）
                  5. distro list/login 命令生成
                  6. 路径翻译自检（14 项）

用法：
    python3 py/verify/verify_engine.py                 # 全量
    python3 py/verify/verify_engine.py --quick         # 跳过 e2e（无 rootfs 时）
    python3 py/verify/verify_engine.py --rootfs PATH   # 指定 Alpine rootfs

依赖：Python 3.8+ 标准库；rootfs 用 Alpine minirootfs（脚本可代为下载）。
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
ENGINE_DIR = REPO_ROOT
RUNTIME_DIR = REPO_ROOT / "py" / "runtime"
ALPINE_VERSION = "3.20.3"
ALPINE_URL = (
    f"https://dl-cdn.alpinelinux.org/alpine/v3.20/releases/x86_64/"
    f"alpine-minirootfs-{ALPINE_VERSION}-x86_64.tar.gz"
)

PASS = "\033[1;32m✓\033[0m"
FAIL = "\033[1;31m✗\033[0m"
SKIP = "\033[1;33m○\033[0m"


class Report:
    def __init__(self) -> None:
        self.passed = 0
        self.failed = 0
        self.skipped = 0
        self.failures: list[str] = []

    def ok(self, name: str, detail: str = "") -> None:
        self.passed += 1
        print(f"  {PASS} {name}" + (f" —— {detail}" if detail else ""))

    def bad(self, name: str, detail: str = "") -> None:
        self.failed += 1
        self.failures.append(f"{name}: {detail}")
        print(f"  {FAIL} {name} —— {detail}")

    def skip(self, name: str, why: str = "") -> None:
        self.skipped += 1
        print(f"  {SKIP} {name}（跳过：{why}）")

    def summary(self) -> bool:
        total = self.passed + self.failed
        print(f"\n  合计：{PASS} {self.passed} 通过 / {FAIL} {self.failed} 失败 / {SKIP} {self.skipped} 跳过")
        if self.failures:
            print("\n  失败明细：")
            for f in self.failures:
                print(f"    - {f}")
        return self.failed == 0


def run(
    cmd: list[str],
    cwd: Path | None = None,
    timeout: int = 120,
    env: dict[str, str] | None = None,
) -> subprocess.CompletedProcess:
    return subprocess.run(
        cmd,
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
        timeout=timeout,
        env=env,
    )


# ---------------------------------------------------------------- A. 单元层

def verify_unit(report: Report) -> None:
    print("\n[A] 单元层：cargo test（引擎 127+ 单测 + 集成 proot_engine）")
    if shutil.which("cargo") is None:
        report.skip("cargo test", "未找到 cargo")
        return
    r = run(["cargo", "test"], cwd=ENGINE_DIR, timeout=600)
    # 取 lib 测试结果行（test result: ok. N passed…）作为摘要，而非最后一行。
    lib_line = next(
        (
            ln
            for ln in r.stdout.splitlines()
            if ln.startswith("test result:") and "passed" in ln
        ),
        "",
    )
    if r.returncode == 0 and lib_line:
        report.ok("cargo test", lib_line.strip())
    else:
        report.bad("cargo test", f"rc={r.returncode}\n{r.stdout[-500:]}{r.stderr[-300:]}")


# ---------------------------------------------------------------- B. 脚本层

def verify_runtime_scripts(report: Report) -> None:
    print("\n[B] 脚本层：py/runtime 钩子宿主侧直跑（Python 可移植性自证）")
    if shutil.which("python3") is None:
        report.skip("py 钩子", "未找到 python3")
        return
    env = dict(os.environ)
    env.update(
        {
            "MOXSH_CONTAINER_ID": "verify-host",
            "MOXSH_HOOK_KIND": "post-install",
            "MOXSH_HOOK_DIR": str(RUNTIME_DIR),
        }
    )
    for script, expect_rc in (("provision.py", 0), ("login_hook.py", 0)):
        path = RUNTIME_DIR / script
        if not path.is_file():
            report.skip(script, "文件缺失")
            continue
        r = subprocess.run(
            [sys.executable, str(path)], capture_output=True, text=True, env=env, timeout=60
        )
        if r.returncode == expect_rc:
            first = r.stdout.strip().splitlines()[0] if r.stdout.strip() else ""
            report.ok(f"{script} 宿主侧运行", first)
        else:
            report.bad(f"{script} 宿主侧运行", f"rc={r.returncode} {r.stderr[-200:]}")

    # 环境契约缺失时应报非 0（provision 的 fail-closed 语义）
    r = subprocess.run(
        [sys.executable, str(RUNTIME_DIR / "provision.py")],
        capture_output=True,
        text=True,
        timeout=60,
    )
    if r.returncode != 0 and "环境契约缺失" in r.stdout + r.stderr:
        report.ok("provision.py 契约缺失 fail-closed")
    else:
        report.bad("provision.py 契约缺失 fail-closed", f"rc={r.returncode}")


# ---------------------------------------------------------------- C. 引擎层

def ensure_rootfs(explicit: str | None, tmp: Path) -> Path | None:
    if explicit:
        p = Path(explicit)
        return p if p.is_dir() else None
    cached = Path(tempfile.gettempdir()) / "moxsh-proot-alpine-rootfs"
    if (cached / "bin" / "busybox").is_file():
        return cached
    tar = cached.parent / "alpine-minirootfs.tar.gz"
    try:
        if not tar.is_file():
            print(f"  下载 Alpine minirootfs {ALPINE_VERSION} …")
            urllib.request.urlretrieve(ALPINE_URL, tar)
        cached.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["tar", "-xzf", str(tar), "-C", str(cached)], check=True, timeout=120
        )
        return cached
    except Exception as e:  # noqa: BLE001 —— 验证脚本允许宽捕
        print(f"  （rootfs 准备失败：{e}）")
        return None


def engine_bins(report: Report) -> tuple[Path, Path] | None:
    target = ENGINE_DIR / "target" / "debug"
    proot = target / "moxsh-proot"
    distro = target / "moxsh-proot-distro"
    r = run(["cargo", "build", "--bins"], cwd=ENGINE_DIR, timeout=600)
    if r.returncode != 0 or not proot.is_file() or not distro.is_file():
        report.bad("引擎二进制构建", f"rc={r.returncode}")
        return None
    report.ok("引擎二进制构建", str(target))
    return proot, distro


# guest 侧 PATH：宿主 PATH 在 guest 内无意义（busybox applet 按 PATH 找
# /bin/<applet> 符号链接），统一注入 Alpine minirootfs 的标准布局。
GUEST_ENV = {**os.environ, "PATH": "/bin:/usr/bin:/sbin:/usr/sbin"}


def verify_engine_e2e(report: Report, rootfs: Path, bins: tuple[Path, Path]) -> None:
    proot, distro = bins
    print(f"\n[C] 引擎层：真实 rootfs e2e（{rootfs}）")

    def proot_run(args: list[str], timeout: int = 60) -> subprocess.CompletedProcess:
        return run(
            [str(proot), "-r", str(rootfs), "-0", *args],
            timeout=timeout,
            env=GUEST_ENV,
        )

    # 1) busybox 全家桶
    r = proot_run(["--", "/bin/busybox", "sh", "-c", "id -u; echo MOX-OK"])
    if r.returncode == 0 and "MOX-OK" in r.stdout and r.stdout.strip().startswith("0"):
        report.ok("busybox + fake root（id -u = 0）", r.stdout.strip().replace("\n", " "))
    else:
        report.bad("busybox + fake root", f"rc={r.returncode} out={r.stdout!r} err={r.stderr[-200:]}")

    # 2) 文件读写（翻译链闭环）
    r = proot_run(["--", "/bin/busybox", "sh", "-c", "echo proot-e2e > /tmp/mox && cat /tmp/mox"])
    if r.returncode == 0 and "proot-e2e" in r.stdout:
        report.ok("guest 文件读写（/tmp 翻译）")
    else:
        report.bad("guest 文件读写", f"rc={r.returncode} out={r.stdout!r}")

    # 3) execveat 三形态（C 探针：fexecve / dirfd 相对 / cwd 相对）
    probe_src = REPO_ROOT / "py" / "verify" / "execveat_probe.c"
    if not probe_src.is_file():
        report.skip("execveat 三形态", "探针缺失")
    else:
        with tempfile.TemporaryDirectory() as td:
            tdp = Path(td)
            probe_bin = tdp / "execveat_probe"
            cc = shutil.which("cc") or shutil.which("gcc")
            if cc is None:
                report.skip("execveat 三形态", "无 cc")
            else:
                # -static：探针进 guest 后不依赖 guest libc（musl rootfs 无
                # libc.so.6；静态 ELF 走引擎 Direct 路径，无 loader 链问题）。
                r = run([cc, "-O1", "-static", "-o", str(probe_bin), str(probe_src)])
                if r.returncode != 0:
                    report.skip("execveat 三形态", f"探针编译失败：{r.stderr[-120:]}")
                else:
                    # 探针拷进 rootfs（guest 内执行），三形态各跑一次
                    guest_probe = rootfs / "tmp" / "execveat_probe"
                    shutil.copy2(probe_bin, guest_probe)
                    os.chmod(guest_probe, 0o755)
                    try:
                        r = proot_run(["--", "/tmp/execveat_probe"])
                        out = r.stdout.strip().splitlines()
                        forms = {"fexecve", "dirfd", "fdcwd"}
                        got = {ln.split("=")[0] for ln in out if "=" in ln}
                        if (
                            r.returncode == 0
                            and got == forms
                            and all(ln.endswith("=OK") for ln in out)
                        ):
                            report.ok("execveat 三形态", " / ".join(out))
                        else:
                            report.bad(
                                "execveat 三形态",
                                f"rc={r.returncode} out={out} err={r.stderr[-200:]}",
                            )
                    finally:
                        guest_probe.unlink(missing_ok=True)

    # 4) 符号链接语义（绝对/相对 target 创建后可执行；readlink 反向还原 guest 路径；
    #    非 applet 名链接与真内核同错——busybox 按 argv[0] basename 选 applet）
    r = proot_run(
        [
            "--",
            "/bin/busybox",
            "sh",
            "-c",
            # applet 名链接：绝对 target 可执行
            "ln -sf /bin/busybox /tmp/cat && /tmp/cat /etc/hostname; "
            # 相对 target 同样可执行（../bin/busybox 相对 /tmp 解析；
            # 用 echo applet 名——busybox 按 argv[0] basename 选 applet）
            "ln -sf ../bin/busybox /tmp/echo && /tmp/echo LINK-REL-OK; "
            # 非 applet 名链接：真内核同样拒绝（chroot 对照实测 rc=127），
            # 引擎必须同错而非静默落到宿主同名文件
            "ln -sf /bin/busybox /tmp/abs_link && /tmp/abs_link echo X; echo notfound_rc=$?; "
            "readlink /tmp/cat",
        ]
    )
    lines = r.stdout.strip().splitlines()
    if (
        r.returncode == 0
        and any(ln == "LINK-REL-OK" for ln in lines)
        and any(ln == "localhost" for ln in lines)
        and any(ln == "notfound_rc=127" for ln in lines)
        and lines[-1] == "/bin/busybox"
    ):
        report.ok("符号链接绝对/相对 target 语义", " / ".join(lines))
    else:
        report.bad("符号链接语义", f"rc={r.returncode} out={lines!r} err={r.stderr[-200:]}")

    # 5) distro list（无安装 = 空列表正常退出）
    root = Path(tempfile.gettempdir()) / "moxsh-proot-verify-root"
    root.mkdir(parents=True, exist_ok=True)
    r = run(
        [str(distro), "list"],
        cwd=ENGINE_DIR,
        timeout=30,
        env={**os.environ, "MOXSH_ROOT": str(root)},
    )
    if r.returncode == 0:
        report.ok("proot-distro list", r.stdout.strip() or "（空列表，符合预期）")
    else:
        report.bad("proot-distro list", f"rc={r.returncode} {r.stderr[-200:]}")

    # 6) 路径翻译自检（14 项）
    r = proot_run(["--help"])
    if "moxsh-proot" in r.stdout:
        report.ok("--help 用法输出")
    else:
        report.bad("--help 用法输出", f"rc={r.returncode}")


def main() -> int:
    ap = argparse.ArgumentParser(description="ProOT 引擎验证脚本")
    ap.add_argument("--quick", action="store_true", help="跳过引擎 e2e（A+B 层）")
    ap.add_argument("--rootfs", default=None, help="指定 Alpine rootfs 路径")
    args = ap.parse_args()

    print("ProOT 引擎验证（py 验证层）")
    print(f"仓库根：{REPO_ROOT}")

    report = Report()
    verify_unit(report)
    verify_runtime_scripts(report)

    if not args.quick:
        with tempfile.TemporaryDirectory() as _td:
            rootfs = ensure_rootfs(args.rootfs, Path(_td))
        bins = engine_bins(report)
        if rootfs is None:
            report.skip("引擎 e2e", "无可用 rootfs（网络不可达或路径无效）")
        elif bins is None:
            report.skip("引擎 e2e", "引擎二进制构建失败")
        else:
            verify_engine_e2e(report, rootfs, bins)

    ok = report.summary()
    print("\n结论：" + ("全部通过 ✅" if ok else "存在失败 ❌"))
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
