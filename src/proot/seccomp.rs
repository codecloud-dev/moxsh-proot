//! seccomp 加速器（P0 性能优化）。
//!
//! 用 `SECCOMP_RET_TRACE` 把「逐 syscall 双停」降为「仅拦截面触发一次
//! seccomp 事件」——非拦截 syscall 直接 `SECCOMP_RET_ALLOW` 零开销直通，
//! 带来量级性能提升（典型负载 99% syscall 不被拦截，旧模型每条都要
//! 2 次 ptrace 停止 + 2 次 `waitpid`）。也是 sysvipc 等地基：拦截面越小、
//! 直通越多，后续挂接特殊子系统（共享内存/IPC）的摩擦面越小。
//!
//! # 门控与失败回落
//! - 仅当子进程 env `MOX_SECCOMP=1` 时由 spawn 在 `TRACEME` 后、`execve`
//!   前安装；默认关，保留既有 `PTRACE_SYSCALL` 双停路径为安全默认。
//! - 安装失败（无 `CAP_SYS_ADMIN` 且 `PR_SET_NO_NEW_PRIVS` 也失败、或 cBPF
//!   被拒）→ 子进程 `_exit(124)`，tracer 立即报错而非「静默跑成未翻译」——
//!   tracer 端无法感知子进程 seccomp 是否真的装上了，故采用 fail-loud 而非
//!   fail-silent，避免隔离悄悄失效。
//!
//! # 与上游 proot 6.x 一致的设计
//! - BPF 只盯 syscall 号（`seccomp_data.nr`）；拦截号 → `RET_TRACE`，其余
//!   → `RET_ALLOW`；并先校验 `arch` 防止 x32 ABI 号段（0x40000000+）误判。
//! - `SECCOMP_RET_TRACE` 在 syscall **入口**产生一次 `PTRACE_EVENT_SECCOMP`
//!   停止，出口仍照常产生 syscall 出口停止——故「需出口处理」的 syscall
//!   （getcwd/readlinkat 反向还原、fake root 出口伪装）在入口处理后用一次
//!   `PTRACE_SYSCALL` 取出口停，其余用 `PTRACE_CONT` 省掉出口停（拦截且
//!   不需出口的 syscall 从 2 停降到 1 停）。
//! - mmap **故意不**纳入拦截集合（它走 `PassThrough`，计划为 `NONE`）：
//!   mmap 较频繁，纳入会抵消加速器在 mmap 密集负载上的收益；scratch
//!   偷换分支会自行用 `PTRACE_SYSCALL` 观察其出口停（见
//!   [`crate::proot::ptrace`] 的首条 syscall 偷换 machinery）。

use crate::proot::ptrace::{syscall_plan, SyscallPlan};

// ---- seccomp / cBPF 常量（旧环境 libc 可能缺失，本地定义）----
const SECCOMP_RET_ALLOW: u32 = 0x7fff_0000;
const SECCOMP_RET_TRACE: u32 = 0x7ff0_0000;

const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;
const PR_SET_SECCOMP: libc::c_int = 22;
const SECCOMP_MODE_FILTER: libc::c_ulong = 2;

// `struct seccomp_data` 字段偏移（linux/seccomp.h）。
const SECCOMP_DATA_NR_OFFSET: u32 = 0;
const SECCOMP_DATA_ARCH_OFFSET: u32 = 4;

// AUDIT_ARCH 值（linux/audit.h）。
#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xC000_003E; // AUDIT_ARCH_X86_64
#[cfg(not(target_arch = "x86_64"))]
const AUDIT_ARCH: u32 = 0xC000_00B7; // AUDIT_ARCH_AARCH64

// 经典 cBPF（sock_filter）。
#[repr(C)]
pub(crate) struct SockFilter {
    code: u16,
    jt: u8,
    jf: u8,
    k: u32,
}

#[repr(C)]
pub(crate) struct SockFprog {
    len: u16,
    filter: *mut SockFilter,
}

const BPF_LD: u16 = 0x00;
const BPF_W: u16 = 0x00;
const BPF_ABS: u16 = 0x20;
const BPF_JMP: u16 = 0x05;
const BPF_RET: u16 = 0x06;
const BPF_JEQ: u16 = 0x10;
const BPF_K: u16 = 0x00;

/// 拦截号集合：所有 `syscall_plan != NONE` 的平台原生号，外加 mmap
/// （scratch 偷换的出口停可被观察所需）。
///
/// 单一数据源来自 [`syscall_plan`]——与拦截计划表始终保持一致，新增拦截
/// syscall 不必回头改 seccomp 集合（否则极易漏，导致该 syscall 在 seccomp
/// 模式下一声不响地直通）。
pub(crate) fn intercepted_native_nrs() -> Vec<u32> {
    let mut v = Vec::new();
    // 仅纳入 `syscall_plan != NONE` 的平台原生号。mmap 故意不纳入：
    // 它走 scratch 偷换（plan 为 PassThrough），其出口停由偷换分支用
    // `PTRACE_SYSCALL` 单独观察；普通 mmap 走 `SECCOMP_RET_ALLOW` 零开销
    // 直通，避免把所有 mmap 都拖进 seccomp 事件（mmap 较频繁，纳入会抵消
    // 加速器在 mmap 密集负载上的收益）。
    for n in 0u64..=1024 {
        if syscall_plan(n) != SyscallPlan::NONE {
            v.push(n as u32);
        }
    }
    v
}

/// 构造 seccomp cBPF 过滤器（单元可测）。
///
/// `intercepted` 为需 `RET_TRACE` 的原生号集合。程序结构：
/// ```text
/// 0:   LD   [arch]            // 取 arch
/// 1:   JEQ  ARCH, +1, +0     // 命中→跳过下条 RET ALLOW；否则落到 RET ALLOW
/// 2:   RET  ALLOW            // arch 不匹配：放行（防 x32 ABI 误判）
/// 3:   LD   [nr]             // 取 syscall 号
/// 4..: JEQ  ni, +jt, +0      // 命中→跳到 RET TRACE
/// K:   RET  ALLOW
/// K+1: RET  TRACE
/// ```
pub(crate) fn build_filter(intercepted: &[u32]) -> Vec<SockFilter> {
    let k = intercepted.len();
    let mut f = Vec::with_capacity(4 + k + 2);
    // 0: 取 arch
    f.push(SockFilter { code: BPF_LD | BPF_W | BPF_ABS, jt: 0, jf: 0, k: SECCOMP_DATA_ARCH_OFFSET });
    // 1: arch 命中 → jt=1 跳到下条 LD nr（越过 RET ALLOW）；否则 jf=0 落到 RET ALLOW
    f.push(SockFilter { code: BPF_JMP | BPF_JEQ | BPF_K, jt: 1, jf: 0, k: AUDIT_ARCH });
    // 2: arch 不匹配 → 放行
    f.push(SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW });
    // 3: 取 syscall 号
    f.push(SockFilter { code: BPF_LD | BPF_W | BPF_ABS, jt: 0, jf: 0, k: SECCOMP_DATA_NR_OFFSET });
    // 4..K-1: 拦截号 → RET TRACE（jt = 距 RET TRACE 的指令数）
    for (i, &nr) in intercepted.iter().enumerate() {
        let jt = (k - i) as u8; // (4+K+1) - (4+i) - 1 = K - i
        f.push(SockFilter { code: BPF_JMP | BPF_JEQ | BPF_K, jt, jf: 0, k: nr });
    }
    // K: 未命中 → 放行
    f.push(SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_ALLOW });
    // K+1: 命中拦截号 → TRACE（tracer 收 PTRACE_EVENT_SECCOMP）
    f.push(SockFilter { code: BPF_RET | BPF_K, jt: 0, jf: 0, k: SECCOMP_RET_TRACE });
    f
}

/// 在子进程（已 `TRACEME`、尚未 `execve`）安装 seccomp 过滤器。
///
/// 返回 `false` 表示安装失败——调用方应 `_exit(124)`（fail-loud 回落）。
/// 调用方（[`crate::proot::ptrace::Tracer::spawn_traced_with_cwd`]）需在
/// `TRACEME` 之后、本调用之前，先以 `kill(getpid(), SIGSTOP)` 把控制权交还
/// tracer，使 tracer 能在本过滤器生效前设好 `PTRACE_O_TRACESECCOMP`（否则
/// 初始 `execve` 被本过滤器拦截时内核找不到监听 tracer，会中止该 syscall
/// 导致 guest 直接 `exit 127`）。
///
/// # Safety
/// 仅在 `fork` 后的子进程、`execve` 之前调用；过滤器随 `execve` 持久化。
pub(crate) unsafe fn install_in_child() -> bool {
    let intercepted = intercepted_native_nrs();
    let mut prog = build_filter(&intercepted);
    // 无新权限：非特权安装 seccomp 必需（否则 prctl 返 EPERM）。跨 execve
    // 继承，使 tracer 后续注入也无需 CAP_SYS_ADMIN（本引擎由子进程侧直接
    // 安装，故此处设置即可）。
    let no_new_privs = unsafe { libc::prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
    if no_new_privs != 0 {
        return false;
    }
    let mut fprog = SockFprog {
        len: prog.len() as u16,
        filter: prog.as_mut_ptr(),
    };
    let rc = unsafe {
        libc::prctl(
            PR_SET_SECCOMP,
            SECCOMP_MODE_FILTER,
            &mut fprog as *mut SockFprog as libc::c_ulong,
        )
    };
    // prog 的堆缓冲已被内核在 prctl 内同步拷贝进内核，可安全释放；
    // 不 forget（drop 不会造成 use-after-free）。
    let ok = rc == 0;
    if !ok {
        drop(prog);
    }
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_has_arch_and_allow_tail() {
        let v = intercepted_native_nrs();
        assert!(!v.is_empty(), "拦截集合不应为空");
        // 拦截集合来自 syscall_plan，至少应包含 openat 等路径拦截 syscall
        // 的原生号（x86_64 上 openat=257）。
        assert!(v.contains(&257), "openat 应在拦截集合内");
        let f = build_filter(&v);
        // 0: LD arch, 1: JEQ arch, 2: RET ALLOW, 3: LD nr, ... tail: RET ALLOW, RET TRACE
        assert_eq!(f[0].code, BPF_LD | BPF_W | BPF_ABS);
        assert_eq!(f[0].k, SECCOMP_DATA_ARCH_OFFSET);
        assert_eq!(f[1].code, BPF_JMP | BPF_JEQ | BPF_K);
        assert_eq!(f[1].k, AUDIT_ARCH);
        assert_eq!(f[1].jt, 1); // 命中 arch → 越过 RET ALLOW
        assert_eq!(f[2].k, SECCOMP_RET_ALLOW);
        assert_eq!(f[3].code, BPF_LD | BPF_W | BPF_ABS);
        assert_eq!(f[3].k, SECCOMP_DATA_NR_OFFSET);
        // 尾两条：ALLOW 在前、TRACE 在后
        assert_eq!(f[f.len() - 2].k, SECCOMP_RET_ALLOW);
        assert_eq!(f[f.len() - 1].k, SECCOMP_RET_TRACE);
    }

    #[test]
    fn filter_jt_targets_trace() {
        // 验证跳转偏移公式：每条 JEQ 命中时落点都必须是 RET TRACE（末条）。
        // 用 x86_64 上确定在集合内的号（257=openat, 2=open, 59=execve）。
        let mut v = vec![2u32, 59, 257];
        // 去重以免重复号扰乱长度断言
        v.sort_unstable();
        v.dedup();
        let k = v.len();
        let f = build_filter(&v);
        // RET TRACE 位于索引 4 + k + 1（见 build_filter 注释）
        let trace_idx = 4 + k + 1;
        assert_eq!(trace_idx, f.len() - 1);
        // 第 i 条 JEQ 的 true 落点 = (4+i) + 1 + jt == trace_idx
        for i in 0..k {
            let jeq = &f[4 + i];
            assert_eq!(jeq.code, BPF_JMP | BPF_JEQ | BPF_K);
            let landing = (4 + i) + 1 + (jeq.jt as usize);
            assert_eq!(landing, trace_idx, "JEQ[{}] 跳转未落到 RET TRACE", i);
            assert_eq!(jeq.jf, 0, "JEQ[{}] 不命中应落到下一条", i);
        }
    }
}
