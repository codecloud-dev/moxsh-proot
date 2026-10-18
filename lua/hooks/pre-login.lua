-- pre-login 欢迎钩子（ProOT 引擎 Lua 钩子层，M8）。
-- 引擎在登录 shell 启动前于容器内执行；失败仅警告、不阻断登录。
--
-- 演示 Lua 配置层的消费方式：dofile 读 proot.lua 的用户偏好，
-- 据此决定 motd / 网络快照是否打印——「配置与逻辑分离」的最小样本。

local cfg_ok, cfg = pcall(dofile, "/moxsh/hooks/proot.lua")
if not cfg_ok then
    cfg = {} -- 配置缺失/语法错误：用默认值，不阻断登录
end

local c = cfg.container or {}
local n = cfg.network or {}

local cid = os.getenv("MOXSH_CONTAINER_ID") or "?"
io.write(string.format("[lua-hook] 容器 %s 登录中（Lua %s）\n", cid, _VERSION))

if c.motd then
    io.write(string.format("\n  %s\n\n", tostring(c.motd)))
end

if n.probe_ipv6 ~= false then
    -- IPv6 连通性快照（UDP connect 只建路由不发包，与引擎探测同语义）
    local sock = nil
    local ok, err = pcall(function()
        sock = require("socket") -- LuaSocket 未必安装——探测不到就跳过
    end)
    if ok and sock then
        local udp = sock.udp()
        udp:settimeout(2)
        local reach = udp:connect("2400:3200::1", 53) -- 只建路由不发包
        io.write(string.format("[lua-hook] IPv6 出网: %s\n",
            reach and "可达" or "不可达（v4 不受影响）"))
    else
        io.write("[lua-hook] IPv6 出网: 跳过（无 LuaSocket；用 Python 登录钩子查看）\n")
    end
end

os.exit(0)
