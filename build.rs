// build.rs —— 把 C JNI 桥接层经 NDK clang 编译，链入 libmoxsh_proot.so
// cargo-ndk 已设置好 CC_aarch64_linux_android / AR / 链接标志等环境变量，
// cc crate 会自动采用对应工具链，无需手写路径。
//
// 五语言分工中 C 的职责边界：c/proot_bridge.c 只做 JVM 类型 ↔ Rust C-ABI
// 的转发（Java_com_moxsh_core_ProotNative_*），不承载任何业务逻辑。
fn main() {
    // 告知 rustc 检查自定义 cfg，消除 unexpected_cfgs 警告
    println!("cargo:rustc-check-cfg=cfg(has_neon_asm)");

    let target = std::env::var("TARGET").unwrap_or_default();
    let is_android = target.contains("android");

    if is_android {
        // C JNI 桥接：仅做 Android 端到 Rust C-ABI 的转发。
        // 仅在 Android 目标编译——本机 `cargo test`（无 NDK/jni.h）仍可纯 Rust 通过。
        cc::Build::new()
            .file("c/proot_bridge.c")
            .flag("-std=c11")
            .flag("-fvisibility=hidden")
            // 链接阶段 cc-rs 仍会附带 -D/-f* 等编译期 flag，clang 对
            // "argument unused" 告警——显式关掉，保持 CI 日志干净。
            .flag("-Wno-unused-command-line-argument")
            .compile("moxsh_proot_bridge");

        println!("cargo:rerun-if-changed=c/proot_bridge.c");
    }
}
