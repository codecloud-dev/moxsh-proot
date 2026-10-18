//! ProOT 容器引擎（Rust 核心，clean-room）。
//!
//! 本 crate 是 **moxsh-proot** 引擎的 Rust 侧主体：ptrace 路径翻译核心
//! （[`proot::ptrace`]）、ELF/shebang 解析（[`proot::elf`]）、符号链接治理
//! （[`proot::symlink_fix`]）、发行版管理（[`proot::distro`]）、网络栈
//! （[`proot::net`]）与脚本运行时（[`proot::script`]，Python/Lua 钩子）。
//!
//! 对外暴露 `moxsh_proot_*` C-ABI 函数，供 [`crate::c`] 目录下的 JNI 桥
//! （c/proot_bridge.c）与独立可执行文件（src/bin/）调用。多语言分工见
//! 仓库根 README：Rust=引擎核心 / Go=安装器 / Python=脚本运行时 /
//! Lua=钩子配置层 / C=JNI 桥。

#![allow(clippy::missing_safety_doc)]
// unsafe 审计强化：unsafe fn 内的每一个 unsafe 操作必须显式包裹 unsafe 块，
// 使"哪些行在做不安全操作"一目了然（FFI 边界可读性/可审计性）。
#![deny(unsafe_op_in_unsafe_fn)]

pub mod crypto;
pub mod proot;

use std::os::raw::{c_char, c_int};

// ============ PRoot 容器引擎 C-ABI ============
//
// 错误码统一约定（ProotError::to_code 的 C 边界）：
//   0 成功 | -1 参数非法 | -2 rootfs 不存在 | -3 发行版未知 | -4 已安装
//   -5 未安装 | -6 需要下载缓存 | -7 SHA-256 校验失败 | -8 解压失败
//   -9 IO 错误 | -10 缓冲区不足 | -11 ptrace 失败
//
// 字符串返回统一采用"调用方缓冲 + 容量"模式（与 moxsh_copy_cells 一致）：
// 返回正数=写入长度（不含 NUL）；0=缓冲区不足（调用方应扩容重试）；
// 负数=错误码。避免堆指针跨界释放，调用方无需配对 free。

/// FFI panic 屏障（P3）：`extern "C"` 里 unwind 属未定义行为（Rust 1.81+ 会 abort，
/// 表现为宿主进程闪退）。所有重逻辑入口统一经 [ffi_guard] 执行：
/// 内部 panic 被捕获并折叠为约定错误码 `-99`（MOXSH_ERR_PANIC），宿主存活、
/// 调用侧可提示重试。只吞 panic 不吞错误：正常错误仍走各函数原有错误码。
///
/// 错误码约定补遗（与模块头注释的错误码表并列）：`-99 = 内核内部 panic（不应出现，
/// 出现请附日志上报）`。
pub const MOXSH_ERR_PANIC: c_int = -99;

/// 安装/恢复等长任务的进度回调：参数为 (消息, 已完成字节, 总字节)。
pub(crate) type ProgressCallback = Box<dyn FnMut(&str, u64, u64)>;

/// FFI panic 屏障（通用版）：捕获闭包 unwind 并折叠为 `on_panic` 兜底值。
/// 兜底值语义由各入口自行约定（错误码/空指针/-1/单元）。
fn ffi_guard_ex<T, F>(f: F, on_panic: T) -> T
where
    F: FnOnce() -> T + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(v) => v,
        Err(_) => on_panic,
    }
}

/// 返回 `c_int` 错误码的入口统一用本包装：异常折叠为 -99。
fn ffi_guard<F: FnOnce() -> c_int + std::panic::UnwindSafe>(f: F) -> c_int {
    ffi_guard_ex(f, MOXSH_ERR_PANIC)
}

/// 读取 C 字符串为 Rust String（非法 UTF-8 / null 返回 None）。
///
/// # Safety
/// `p` 必须指向合法以 NUL 结尾的字符串，或为 null。
unsafe fn read_opt_cstr(p: *const c_char) -> Option<String> {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        if p.is_null() {
            return None;
        }
        std::ffi::CStr::from_ptr(p)
            .to_str()
            .ok()
            .map(|s| s.to_string())
    }
}

/// 把字符串写入调用方缓冲（截断防护）。
///
/// # Safety
/// `out` 指向至少 `cap` 字节可写内存（可 null，此时仅探测所需长度）。
unsafe fn write_cstr_buf(out: *mut c_char, cap: usize, s: &str) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        let need = s.len() + 1; // 含 NUL
        if out.is_null() || cap < need {
            return 0; // 0 = 缓冲不足，调用方扩容重试
        }
        let slice = std::slice::from_raw_parts_mut(out as *mut u8, cap);
        slice[..s.len()].copy_from_slice(s.as_bytes());
        slice[s.len()] = 0;
        need as c_int
    }
}

/// 安装发行版：`id` 为预置 id（ubuntu/debian/kali/alpine）或自定义容器名。
/// `cache_tar` 为已下载归档路径（可 null：内部 curl 兜底，Android 建议先下载）。
/// `dns_csv` 为宿主系统 DNS 的 CSV 串（如 `"192.168.1.1,223.5.5.5"`，可 null：
/// 用默认国内公共 DNS）——PRoot 共享宿主网络栈，但 guest glibc 读的是
/// rootfs 内的 resolv.conf，必须落成真实可解析地址。
/// 需要外部下载时返回 -6（ProotError::DownloadRequired）。
///
/// # Safety
/// `id`/`moxsh_root` 必须指向合法 NUL 结尾 UTF-8 字符串；`cache_tar`/`dns_csv` 可为 null。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_install(
    id: *const c_char,
    moxsh_root: *const c_char,
    cache_tar: *const c_char,
    dns_csv: *const c_char,
) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；unsafe 操作收敛于内层块。
    // P3 panic 屏障：安装链路（下载/解压/落盘）体量大，内部异常折叠为 -99。
    ffi_guard(move || unsafe {
        let (Some(id), Some(root)) = (read_opt_cstr(id), read_opt_cstr(moxsh_root)) else {
            return -1;
        };
        let mut mgr = proot::distro::DistroManager::new(std::path::Path::new(&root));
        // 宿主系统 DNS 优先（经 sanitize_dns_csv 清洗防 resolv.conf 注入）。
        if let Some(csv) = read_opt_cstr(dns_csv) {
            mgr.set_dns_servers_csv(&csv);
        }
        let mut progress: ProgressCallback = Box::new(|_, _, _| {});
        let r = match proot::distro::DistroManager::find_spec(&id) {
            Ok(spec) => {
                // 先把 String 绑定到局部，再取引用（避免临时值悬垂）。
                let cache_str = read_opt_cstr(cache_tar);
                let cache = cache_str.as_deref().map(std::path::Path::new);
                mgr.install(&spec, cache, &mut progress)
            }
            // 非预置 id：按自定义 rootfs 处理（cache_tar 必填）。
            Err(_) => match read_opt_cstr(cache_tar) {
                Some(tar) => mgr.install_custom(&id, std::path::Path::new(&tar), None, &mut progress),
                None => return proot::ProotError::DownloadRequired(id).to_code(),
            },
        };
        r.map(|_| 0).unwrap_or_else(|e| e.to_code())
    })
}

/// 删除发行版。
///
/// # Safety
/// `id`/`moxsh_root` 必须指向合法 NUL 结尾 UTF-8 字符串。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_remove(id: *const c_char, moxsh_root: *const c_char) -> c_int {
    // P3 panic 屏障：C-ABI 入口禁止 unwind（extern "C" 边界 unwind 是 UB/abort），
    // 异常折叠为 -99（MOXSH_ERR_PANIC）。
    ffi_guard_ex(move || unsafe { moxsh_proot_remove_impl(id, moxsh_root) }, MOXSH_ERR_PANIC)
}

unsafe extern "C" fn moxsh_proot_remove_impl(id: *const c_char, moxsh_root: *const c_char) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        let (Some(id), Some(root)) = (read_opt_cstr(id), read_opt_cstr(moxsh_root)) else {
            return -1;
        };
        proot::distro::DistroManager::new(std::path::Path::new(&root))
            .remove(&id)
            .map(|_| 0)
            .unwrap_or_else(|e| e.to_code())
    }
}

/// 生成发行版登录命令行（proot 完整调用）写入 `out_buf`（容量 `cap`）。
/// 返回所需长度（含 NUL，>0 成功）/ 0 缓冲不足 / 负错误码。
///
/// # Safety
/// `id`/`moxsh_root` 指向合法字符串；`out_buf` 指向至少 `cap` 字节可写内存。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_login(
    id: *const c_char,
    moxsh_root: *const c_char,
    out_buf: *mut c_char,
    cap: usize,
) -> c_int {
    // P3 panic 屏障：C-ABI 入口禁止 unwind（extern "C" 边界 unwind 是 UB/abort），
    // 异常折叠为 -99（MOXSH_ERR_PANIC）。
    ffi_guard_ex(move || unsafe { moxsh_proot_login_impl(id, moxsh_root, out_buf, cap) }, MOXSH_ERR_PANIC)
}

unsafe extern "C" fn moxsh_proot_login_impl(
    id: *const c_char,
    moxsh_root: *const c_char,
    out_buf: *mut c_char,
    cap: usize,
) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        let (Some(id), Some(root)) = (read_opt_cstr(id), read_opt_cstr(moxsh_root)) else {
            return -1;
        };
        match proot::distro::DistroManager::new(std::path::Path::new(&root)).login_cmd(&id) {
            Ok(cmd) => write_cstr_buf(out_buf, cap, &cmd),
            Err(e) => e.to_code(),
        }
    }
}

/// 列出已安装发行版，写入 `out_buf`（行分隔协议：每行 `id|initialized|rootfs`）。
///
/// # Safety
/// `moxsh_root` 指向合法字符串；`out_buf` 指向至少 `cap` 字节可写内存。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_list_installed(
    moxsh_root: *const c_char,
    out_buf: *mut c_char,
    cap: usize,
) -> c_int {
    // P3 panic 屏障：C-ABI 入口禁止 unwind（extern "C" 边界 unwind 是 UB/abort），
    // 异常折叠为 -99（MOXSH_ERR_PANIC）。
    ffi_guard_ex(move || unsafe { moxsh_proot_list_installed_impl(moxsh_root, out_buf, cap) }, MOXSH_ERR_PANIC)
}

unsafe extern "C" fn moxsh_proot_list_installed_impl(
    moxsh_root: *const c_char,
    out_buf: *mut c_char,
    cap: usize,
) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        let Some(root) = read_opt_cstr(moxsh_root) else {
            return -1;
        };
        let list = match proot::distro::DistroManager::new(std::path::Path::new(&root)).list_installed() {
            Ok(l) => l,
            Err(e) => return e.to_code(),
        };
        let text: String = list
            .iter()
            .map(|d| format!("{}|{}|{}\n", d.id, d.initialized as u8, d.rootfs.display()))
            .collect();
        write_cstr_buf(out_buf, cap, text.trim_end_matches('\n'))
    }
}

/// 备份发行版 rootfs 到 `out_tar`（tar.gz）。
///
/// # Safety
/// `id`/`moxsh_root`/`out_tar` 必须指向合法 NUL 结尾 UTF-8 字符串。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_backup(
    id: *const c_char,
    moxsh_root: *const c_char,
    out_tar: *const c_char,
) -> c_int {
    // P3 panic 屏障：C-ABI 入口禁止 unwind（extern "C" 边界 unwind 是 UB/abort），
    // 异常折叠为 -99（MOXSH_ERR_PANIC）。
    ffi_guard_ex(move || unsafe { moxsh_proot_backup_impl(id, moxsh_root, out_tar) }, MOXSH_ERR_PANIC)
}

unsafe extern "C" fn moxsh_proot_backup_impl(
    id: *const c_char,
    moxsh_root: *const c_char,
    out_tar: *const c_char,
) -> c_int {
    // SAFETY：FFI thin wrapper——契约见函数 # Safety 文档；整体即不安全上下文。
    unsafe {
        let (Some(id), Some(root), Some(out)) = (
            read_opt_cstr(id),
            read_opt_cstr(moxsh_root),
            read_opt_cstr(out_tar),
        ) else {
            return -1;
        };
        proot::distro::DistroManager::new(std::path::Path::new(&root))
            .backup(&id, std::path::Path::new(&out))
            .map(|_| 0)
            .unwrap_or_else(|e| e.to_code())
    }
}

/// 从备份 tar.gz 恢复发行版（覆盖已有 rootfs）。
///
/// # Safety
/// `id`/`moxsh_root`/`tar_path` 必须指向合法 NUL 结尾 UTF-8 字符串。
#[no_mangle]
pub unsafe extern "C" fn moxsh_proot_restore(
    id: *const c_char,
    moxsh_root: *const c_char,
    tar_path: *const c_char,
) -> c_int {
    // P3 panic 屏障：C-ABI 入口禁止 unwind（extern "C" 边界 unwind 是 UB/abort），
    // 异常折叠为 -99（MOXSH_ERR_PANIC）。rootfs 恢复 = 大 tar 解压 + 覆盖写，
    // 异常折叠为 -99。
    ffi_guard(move || unsafe {
        let (Some(id), Some(root), Some(tar)) = (
            read_opt_cstr(id),
            read_opt_cstr(moxsh_root),
            read_opt_cstr(tar_path),
        ) else {
            return -1;
        };
        proot::distro::DistroManager::new(std::path::Path::new(&root))
            .restore(&id, std::path::Path::new(&tar))
            .map(|_| 0)
            .unwrap_or_else(|e| e.to_code())
    })
}

/// 路径翻译自检：运行内置用例，返回通过数。
/// 等于 `proot::TRANSLATE_SELFTEST_TOTAL`（14）即全部通过，供宿主启动自检。
#[no_mangle]
pub extern "C" fn moxsh_proot_translate_selftest() -> c_int {
    proot::translate_selftest() as c_int
}

#[cfg(test)]
mod tests {
    use super::proot;
    use std::os::raw::c_int;

    /// translate 自检——纯路径逻辑（不触碰文件系统/网络），随测试跑全量，
    /// 断言通过数等于内置总数（与 so 内 `moxsh_proot_translate_selftest` 同源）。
    #[test]
    fn translate_selftest_all_pass() {
        assert_eq!(proot::translate_selftest(), proot::TRANSLATE_SELFTEST_TOTAL);
    }

    /// C-ABI 自检入口返回值断言（14 以内 usize→c_int 无损）。
    #[test]
    fn cabi_translate_selftest_code_matches() {
        let code = super::moxsh_proot_translate_selftest();
        assert_eq!(code, proot::TRANSLATE_SELFTEST_TOTAL as c_int);
    }
}
