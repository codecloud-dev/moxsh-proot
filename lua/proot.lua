-- proot 容器配置层（Lua 声明式配置，M8）。
--
-- 定位：引擎（Rust）只认结构化事实（rootfs/bindings/fake_root/cwd/env），
-- 面向用户的「偏好」由本文件承载，供 Lua 钩子 dofile 后读取——
-- Python 钩子经 MOXSH_HOOK_DIR 环境变量同样能找到它。
--
-- 安装器把本文件与 lua/hooks/、py/runtime/ 一并种子到 $MOXSH_ROOT/hooks/，
-- 用户改配置无需碰引擎二进制；语法错误时钩子优雅降级（见 hooks 内写法）。
--
-- 生效方式：钩子内 `dofile("/moxsh/hooks/proot.lua")`（guest 路径）。

return {
    -- 容器身份
    container = {
        -- 登录欢迎语（pre-login.lua 打印；设 nil/false 关闭）
        motd = "proot 容器 —— 无 root 的完整 Linux（moxsh 引擎）",
        -- 登录钩子是否打印网络快照
        show_network = true,
    },

    -- 网络偏好（与引擎 net 模块的关系：引擎负责写 resolv.conf/hosts，
    -- 这里只表达「用户偏好」——如首选的公共 DNS 顺序）
    network = {
        -- 首选 DNS（引擎默认国内公共 DNS；留空则用引擎默认）
        dns = { "223.5.5.5", "119.29.29.29" },
        -- 登录时是否做 IPv6 连通性快照（慢网可关）
        probe_ipv6 = true,
    },

    -- 供给钩子行为
    provisioning = {
        -- post-install 是否校验 DNS 解析（archive.ubuntu.com）
        verify_dns = true,
        -- 校验失败时是否仅警告（true = 不阻断安装）
        warn_only = true,
    },
}
