// ProOT 引擎 C JNI 桥接层（五语言分工中的 C 职责：仅做 JVM ↔ Rust C-ABI 转发）。
//
// 本文件由 moxsh-terminal 的 cpp/bridge.cpp「M4 PRoot 容器引擎转发」段落
// C 化演进而来：桥接逻辑不依赖任何 C++ 特性（无类/模板/RAII），用 C11 重写，
// 减少一条工具链依赖，也让「C = 桥接层」的分工名实相符。
//
// 对应 Rust 侧 C-ABI（src/lib.rs）：
//   moxsh_proot_install / moxsh_proot_remove / moxsh_proot_login
//   moxsh_proot_list_installed / moxsh_proot_backup / moxsh_proot_restore
//   moxsh_proot_translate_selftest
//
// 惯例：jstring ↔ const char*（GetStringUTFChars/ReleaseStringUTFChars），
// jbyteArray ↔ char* 缓冲（GetByteArrayElements + Release 传 0 提交）。
// 返回值统一为 Rust C-ABI 的错误码/长度：>0 所需长度 / 0 缓冲不足 / <0 错误码。
//
// 本文件不承载任何业务逻辑；仅在 Android 目标由 build.rs 编入
// libmoxsh_proot.so（见 Cargo.toml 的 crate-type = cdylib）。

#include <jni.h>
#include <stdint.h>
#include <stddef.h>

// ---- Rust 侧 C-ABI 转发声明（实现见 src/lib.rs） ----
extern int moxsh_proot_install(const char* id, const char* moxsh_root,
                               const char* cache_tar, const char* dns_csv);
extern int moxsh_proot_remove(const char* id, const char* moxsh_root);
extern int moxsh_proot_login(const char* id, const char* moxsh_root,
                             char* out, size_t cap);
extern int moxsh_proot_list_installed(const char* moxsh_root, char* out, size_t cap);
extern int moxsh_proot_backup(const char* id, const char* moxsh_root, const char* out_tar);
extern int moxsh_proot_restore(const char* id, const char* moxsh_root, const char* tar_path);
extern int moxsh_proot_translate_selftest(void);

// ---- 桥接辅助（匿名 namespace 的 C 等价：static 内部链接） ----

// 取 jstring 的 UTF-8；可选参数（null 透传 null，与 Rust 侧 read_opt_cstr 对应）。
static const char* proot_jstr(JNIEnv* env, jstring s) {
    return s == NULL ? NULL : env->GetStringUTFChars(s, NULL);
}

static void proot_jstr_free(JNIEnv* env, jstring s, const char* c) {
    if (s != NULL && c != NULL) env->ReleaseStringUTFChars(s, c);
}

// 字符串返回约定：Rust 经 GetByteArrayElements 指针直接写入（Release 传 0 提交），
// 返回值原样透传：>0 所需长度 / 0 缓冲不足 / <0 错误码。
static jint proot_string_return(jint n) {
    return n;
}

// ---- JNI 入口（Java_com_moxsh_core_ProotNative_*） ----

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeInstall(JNIEnv* env, jobject,
                                              jstring id, jstring moxshRoot,
                                              jstring cacheTar, jstring dnsCsv) {
    const char* i = proot_jstr(env, id);
    const char* r = proot_jstr(env, moxshRoot);
    const char* t = proot_jstr(env, cacheTar);
    const char* d = proot_jstr(env, dnsCsv);
    int n = moxsh_proot_install(i, r, t, d);
    proot_jstr_free(env, id, i); proot_jstr_free(env, moxshRoot, r);
    proot_jstr_free(env, cacheTar, t); proot_jstr_free(env, dnsCsv, d);
    return n;
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeRemove(JNIEnv* env, jobject,
                                             jstring id, jstring moxshRoot) {
    const char* i = proot_jstr(env, id);
    const char* r = proot_jstr(env, moxshRoot);
    int n = moxsh_proot_remove(i, r);
    proot_jstr_free(env, id, i); proot_jstr_free(env, moxshRoot, r);
    return n;
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeLogin(JNIEnv* env, jobject,
                                            jstring id, jstring moxshRoot, jbyteArray buf) {
    const char* i = proot_jstr(env, id);
    const char* r = proot_jstr(env, moxshRoot);
    jsize cap = env->GetArrayLength(buf);
    jbyte* p = env->GetByteArrayElements(buf, NULL);
    int n = moxsh_proot_login(i, r, (char*)p, (size_t)cap);
    env->ReleaseByteArrayElements(buf, p, 0);
    proot_jstr_free(env, id, i); proot_jstr_free(env, moxshRoot, r);
    return proot_string_return(n);
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeListInstalled(JNIEnv* env, jobject,
                                                    jstring moxshRoot, jbyteArray buf) {
    const char* r = proot_jstr(env, moxshRoot);
    jsize cap = env->GetArrayLength(buf);
    jbyte* p = env->GetByteArrayElements(buf, NULL);
    int n = moxsh_proot_list_installed(r, (char*)p, (size_t)cap);
    env->ReleaseByteArrayElements(buf, p, 0);
    proot_jstr_free(env, moxshRoot, r);
    return proot_string_return(n);
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeBackup(JNIEnv* env, jobject,
                                             jstring id, jstring moxshRoot, jstring outTar) {
    const char* i = proot_jstr(env, id);
    const char* r = proot_jstr(env, moxshRoot);
    const char* o = proot_jstr(env, outTar);
    int n = moxsh_proot_backup(i, r, o);
    proot_jstr_free(env, id, i); proot_jstr_free(env, moxshRoot, r); proot_jstr_free(env, outTar, o);
    return n;
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeRestore(JNIEnv* env, jobject,
                                              jstring id, jstring moxshRoot, jstring tarPath) {
    const char* i = proot_jstr(env, id);
    const char* r = proot_jstr(env, moxshRoot);
    const char* t = proot_jstr(env, tarPath);
    int n = moxsh_proot_restore(i, r, t);
    proot_jstr_free(env, id, i); proot_jstr_free(env, moxshRoot, r); proot_jstr_free(env, tarPath, t);
    return n;
}

JNIEXPORT jint JNICALL
Java_com_moxsh_core_ProotNative_nativeTranslateSelfTest(JNIEnv*, jobject) {
    return moxsh_proot_translate_selftest();
}
