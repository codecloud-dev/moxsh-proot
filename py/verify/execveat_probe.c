/*
 * execveat 三形态探针（py/verify/verify_engine.py 的引擎层用例）。
 *
 * 背景：引擎对 execveat 的显式解释器调用做「收尾两件套」改写——
 * envp 从槽 3 搬到槽 2、syscall 号换成 execve（不换则内核按 execveat
 * 语义把 path 指针当 dirfd → EBADF）。本探针在 guest 内依次触发三种
 * 形态，每种成功时由 busybox echo 打点，失败时打印 errno：
 *
 *   1. fexecve      ：execveat(fd, "", argv, envp, AT_EMPTY_PATH)
 *   2. dirfd 相对   ：execveat(dirfd, "busybox", argv, envp, 0)
 *   3. cwd 相对     ：execveat(AT_FDCWD, "busybox", argv, envp, 0)
 *
 * 输出三行（顺序固定）：
 *   fexecve=OK | fexecve=ERR:<errno>
 *   dirfd=OK   | dirfd=ERR:<errno>
 *   fdcwd=OK   | fdcwd=ERR:<errno>
 *
 * 退出码：全 OK = 0；任一失败 = 1。
 *
 * **fork-per-form**：execveat 成功会替换进程，顺序执行只能打出第一行。
 * 故每种形态各 fork 一个子进程去 execveat，父进程 waitpid 收取退出码——
 * busybox echo 打点走继承的 stdout，父进程继续跑完三形态。
 *
 * 编译（宿主侧，进 rootfs 后由引擎拉起）：
 *   cc -O1 -o execveat_probe execveat_probe.c
 */

#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static int failures = 0;

/* 每种形态的 busybox echo 打点（argv[0] 之后的 "echo" 即 applet 名）。 */
static void try_form(const char *tag, int dirfd, const char *path, int flags) {
    char *const argv[] = {(char *)"/bin/busybox", (char *)"echo", (char *)tag, NULL};
    char *const envp[] = {(char *)"PATH=/bin", NULL};

    fflush(stdout);
    pid_t pid = fork();
    if (pid < 0) {
        printf("%s=ERR:fork\n", tag);
        failures++;
        return;
    }
    if (pid == 0) {
        /* 子进程：execveat 成功即被 busybox 替换（echo tag 后退出 0）。 */
        execveat(dirfd, path, argv, envp, flags);
        /* 返回即失败：打 errno 后以 1 退出（不与成功形态的 0 混淆）。 */
        printf("%s=ERR:%d\n", tag, errno);
        _exit(1);
    }
    int status = 0;
    if (waitpid(pid, &status, 0) < 0) {
        printf("%s=ERR:waitpid\n", tag);
        failures++;
        return;
    }
    if (!(WIFEXITED(status) && WEXITSTATUS(status) == 0)) {
        failures++;
    }
}

int main(void) {
    int fd = open("/bin/busybox", O_RDONLY | O_CLOEXEC);
    if (fd < 0) {
        printf("open=ERR:%d\n", errno);
        return 1;
    }

    try_form("fexecve=OK", fd, "", AT_EMPTY_PATH);

    int dirfd = open("/bin", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (dirfd < 0) {
        printf("opendir=ERR:%d\n", errno);
        return 1;
    }
    try_form("dirfd=OK", dirfd, "busybox", 0);
    close(dirfd);

    if (chdir("/bin") != 0) {
        printf("chdir=ERR:%d\n", errno);
        return 1;
    }
    try_form("fdcwd=OK", AT_FDCWD, "busybox", 0);

    return failures == 0 ? 0 : 1;
}
