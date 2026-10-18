-- post-install 供给钩子（ProOT 引擎 Lua 钩子层，M8）。
-- 引擎在发行版安装完成后于容器内执行（rootfs 有 lua/luajit 时生效）。
--
-- Lua 特长点落点：体积小、语义稳、无第三方依赖——适合「轻量供给 +
-- 声明式配置」这类不需要 Python 标准库重武器的场景；与 Python 钩子
-- 并存时按文件名排序依次执行（10-post-install.lua 先于 20-post-install.py）。
--
-- 约定（引擎注入的环境变量）：
--   MOXSH_CONTAINER_ID   容器 id
--   MOXSH_HOOK_KIND      钩子类型（post-install）
--   MOXSH_HOOK_DIR       钩子目录 guest 路径（/moxsh/hooks）
--
-- 退出码：0 = 通过；1 = 未就绪（不阻断安装，仅报告）。

local cid = os.getenv("MOXSH_CONTAINER_ID") or "?"
local kind = os.getenv("MOXSH_HOOK_KIND") or "?"

io.write(string.format("[lua-hook] 容器 %s %s 钩子（Lua %s）\n", cid, kind, _VERSION))

-- 供给检查 1：/etc/resolv.conf 必须有 nameserver（引擎应已写入）。
local f = io.open("/etc/resolv.conf", "r")
if not f then
    io.write("[lua-hook] ✗ 无法读取 /etc/resolv.conf\n")
    os.exit(1)
end
local servers = {}
for line in f:lines() do
    local ns = line:match("^%s*nameserver%s+([%d:.a-fA-F]+)")
    if ns then servers[#servers + 1] = ns end
end
f:close()

if #servers == 0 then
    io.write("[lua-hook] ✗ /etc/resolv.conf 无 nameserver（引擎应已写入，异常）\n")
    os.exit(1)
end
io.write(string.format("[lua-hook] ✓ nameserver: %s\n", table.concat(servers, ", ")))

-- 供给检查 2：root 家目录可写（fake root 下供给脚本的常见落点）。
local probe, err = io.open("/root/.moxsh-lua-probe", "w")
if not probe then
    io.write(string.format("[lua-hook] ✗ /root 不可写: %s\n", tostring(err)))
    os.exit(1)
end
probe:close()
os.remove("/root/.moxsh-lua-probe")
io.write("[lua-hook] ✓ /root 可写\n")

io.write("[lua-hook] 轻量供给就绪\n")
os.exit(0)
