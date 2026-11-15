# 隐私政策 / Privacy Policy

**适用仓库**：`codecloud-dev/moxsh-proot`（PRoot 容器引擎 / `mox` 命令行工具）
**生效日期 / Effective date**：2026-11-15
**责任主体**：`codecloud-dev` —— 一个 GitHub 个人账号，由个人开发者运营，**不是公司、不是法人实体**；没有公司名称、注册号、注册地址或客服电话。

> 中文在前，英文在后，同一份文件；口径一致，歧义以中文为准。
> 本政策按 `src/proot/*.rs`（尤其 `backend.rs` / `device_claim.rs` / `redeem.rs` / `fan.rs` / `distro.rs` / `net.rs`）、`go/internal/fetch/` 与 `recipes/index.toml` 的**真实代码行为**逐条写。凡未实现的能力写明「当前版本未实现 / 未收集」。
> 撰写口径参考 GDPR 与《个人信息保护法》的最小必要、目的限定、透明原则，不引用、不复制任何条文原文。

---

## 一、会联网做什么

| 行为 | 端点 / 目标 | 是否携带用户数据 |
| --- | --- | --- |
| 下载 rootfs / 发行版包与校验清单 | 官方源：`cdimage.ubuntu.com`、`us.lxd.images.canonical.com`、`kali.download`、`dl-cdn.alpinelinux.org`；国内清华 TUNA 镜像（`mirrors.tuna.tsinghua.edu.cn`）；`SHA256SUMS` | 纯文件下载，不携带账号/设备标识 |
| 设备码登录（RFC 8628） | `POST /device/code`、`POST /device/token`，经官网 `https://mox-site.pages.dev` 进入 `mox-id` | 设备声明 5 字段（见 2.2） |
| 设备绑定 | 本机 `127.0.0.1` 一次性回环 + 官网 `bind.html`（URL 带 `redirect` / `state` / `device_token` / `name` / `platform`）→ GitHub 登录 → `POST /devices/register` | 设备令牌与粗粒度设备描述 |
| 兑换码 / 爱发电订单核销 | `POST /products/redeem`（登录态）、`GET /entitlement/check?device_token=...` | 兑换码或订单号 + 设备令牌 |
| 粉丝码在线校验 | `POST /fan/verify` | HS256 签发的粉丝码 |
| 容器网络探测 | 阿里公共 DNS `223.5.5.5:443`（TCP connect）、IPv6 `[2400:3200::1]:53`（UDP connect 建路由，**不发包**） | 无，仅连通性判定 |
| 后端候选域 | 客户端唯一入口 `https://mox-site.pages.dev`；上游 `https://mox-id.moxsh.app`；兜底 `https://mox-id.3042980037.workers.dev`（`backend.rs`） | — |

> `backend.rs` 注释明确：旧的 `https://mox-id.mox-proot.app` 是**从未部署的占位域名**，`codecloud-dev.github.io/mox-site` 是已废弃的 Pages 地址。

---

## 二、逐项数据清单

### 2.1 rootfs / 包下载（`distro.rs` / `go/internal/fetch/fetch.go`）

- 按宿主架构（`aarch64` / `x86_64`）与地区（国内优先清华 TUNA 镜像，国外走官方源）解析下载 URL；`curl_fetch()` 用 curl 子进程（Android 端由宿主 Kotlin 层 OkHttp / 系统网络栈下载后传本地路径），带**断点续传**（`Range`）与实时进度回调。
- 下载 `SHA256SUMS` 校验完整性；`DistroManager::install()` 做「下载 → 校验 → 解压 → 初始化」。
- 发行版：Ubuntu Base 24.04、Debian bookworm（LXD images）、Kali NetHunter minimal、Alpine minirootfs 3.20.3。
- **这些是纯文件 GET**，请求中不含账号、邮箱、设备令牌或任何可识别信息；源站/镜像站按其自身日志记录请求。
- 当前版本**未实现**：下载行为回传、安装统计、遥测、崩溃上报。

### 2.2 设备声明（`device_claim.rs` / `src/device_claim.ts` 契约）

`POST /device/code` 必须带 `X-Mox-Device-Claim`（反代剥离 header 时 JSON body 兜底），内容是**固定字段顺序的 canonical JSON 原文**：

| 字段 | 值 | 为什么不算隐私 |
| --- | --- | --- |
| `device_id` | 本机随机 32 位 hex，落盘 `$MOX_ROOT/device.toml` 可删可重置 | 与硬件无关，不是 MAC/IMEI/序列号 |
| `platform` | 编译期常量 `linux` / `android` / `darwin` / `windows` / `unknown` | 粗粒度 OS 类别 |
| `arch` | 编译期常量 `x86_64` / `aarch64` / `armv7` / `i686` / `riscv64` / `unknown` | CPU 架构 |
| `client` | `mox-proot` 等客户端名 | 软件标识 |
| `client_version` | 语义版本（来自 `CARGO_PKG_VERSION`） | 软件版本 |

后端 `validateDeviceClaim()` 白名单只认这 5 个键，任何额外字段/隐私字段一律 400；`device_claims` 表存 canonical JSON、SHA-256 哈希、人眼可读 `label`、时间。轮询换 token 时带**同一声明**，服务端比对哈希防设备码被别的设备截获冒领。

**明确不收集、绝不落库**：主机名、用户名、MAC/IMEI/序列号、精确 OS 版本串、磁盘路径、IP、地理位置、浏览器 UA 原文、任何文件内容。`cf-connecting-ip` 只用于**内存中的登录限速计数**。

### 2.3 设备码扫码登录（`device_code.rs` / `0022`）

`mox-proot login` → `POST /device/code` 拿 `device_code` + `user_code` + `verification_uri_complete`（`<publicBase>/activate?code=<user_code>`）→ 终端渲染**半块二维码** → 手机扫码在 `/activate` 用 **GitHub / Gitee 登录**并点「确认绑定」→ worker 标记已授权并注册/复用一台 `device` 行、下发 `device_token` → 终端轮询 `POST /device/token` 拿到令牌。状态：`pending → authorized → consumed / expired`；换 token 后即 `consumed`。

登录身份由 GitHub / Gitee OAuth 提供（资料字段见 mox-id 政策）；本引擎只保存最终的 `device_token` 到本机 `device.toml`。

### 2.4 设备绑定（`redeem.rs` 流程第 1–2 步）

`mox redeem login` 在 `127.0.0.1` 起一次性回环监听，唤起浏览器打开官网 `bind.html`，URL query 携带：`redirect=<回环地址>`、`state=<随机>`、`device_token=<本地随机串>`、`name=<设备名>`、`platform=<平台>`。页内 **GitHub 登录** → 点「绑定此设备」= `POST /devices/register` `{device_token, name, platform}` → 页面带 `ok=1&state` 跳回回环（state 须一致，防伪造），脚本自动收到确认。

`device_token` 是**本地随机 URL 安全串**，不含任何可伪造材料；泄漏等价于「换一台设备」，不是身份凭据。

### 2.5 兑换码 / 爱发电订单 / 授权缓存（`redeem.rs` / `products.ts`）

- 买家在官网个人中心或兑换页用**兑换码**（形如 `PROOT-7K3M-9QP2-X4F8`）或**爱发电订单号 `out_trade_no`** 二选一，经 `POST /products/redeem` 把授权赋予指定设备。后端校验：订单真实已付且未核销、`device_token` 属于登录用户、兑换与核销走 D1 batch 原子化。
- `mox redeem status` → `GET /entitlement/check?device_token=<hex>`（**无登录，token 即凭据**）联网拉取全部授权，响应是纯文本，一行一个 `ok <product_id> <tier> <exp_unix秒>`（`exp=0` 永久），并附服务端 **Ed25519 签名**（公钥内嵌客户端、只验签，私钥永不落客户端）。
- **本地缓存**：`entitlements.toml`（含 `fetched_at`）；超过 `ENTS_TTL_DAYS` 视为陈旧（门控显示锁定并提示刷新）；**有限期订阅**（`exp>0`，如 `proot-pro` 月度 30 天）即便缓存陈旧也按 `exp` 判定，离线仍能用到到期当天、过后立即失效。
- 本地文件：`$MOX_ROOT/device.toml`（设备令牌）、`$MOX_ROOT/entitlements.toml`（授权缓存）、`$MOX_ROOT/fan.toml`（粉丝档位）。删 `device.toml` = 换设备，需重新绑定。
- **不接收**：银行卡号、支付密码、支付账户凭据、爱发电账号密码。

### 2.6 粉丝码（`fan.rs` / `0011`）

`mox fan` 用服务端 **HS256 签发**的激活码（`fan_codes` 台账：`code`、`user_id`、`github_id`、`login`、`tier`、`exp`、`issued_at`、`verified_at`、`verify_count`、`revoked`），每次 `POST /fan/verify` 在线校验并落 `verified_at` / `verify_count`（审计 + 防滥用观测），`exp=0` 为永久但服务端可随时 `revoked` 吊销。手机脚本只 `curl` 转发、不解析 JWT。

### 2.7 容器网络配置（`net.rs`）

PRoot **不做 network namespace 隔离**，guest 直接走宿主内核网络栈。引擎为让容器内域名解析可用，会：

- 读宿主系统 DNS（C-ABI 传入、经 `sanitize_dns_csv` 清洗）或回退公共 DNS：IPv4 `223.5.5.5`（阿里）/ `119.29.29.29`（腾讯），IPv6 `2400:3200::1` / `2402:4e00::`；
- 写 rootfs 内 `/etc/resolv.conf`（含 `options timeout:2 attempts:2`）、同步宿主 `/etc/hosts` 的额外条目；
- 用 `std::net` 做 IPv6 路由探测（UDP connect **只建路由、不发包**）与出网探测（TCP connect 阿里 DNS 443，2 秒超时）。

这些是**本地网络配置与连通性探测**，不向任何服务器上传探测结果，不收集地理位置或网络画像。

---

## 三、不收集什么（当前版本）

- 不上传：终端命令内容、容器内文件内容、剪贴板、浏览历史、通讯录、位置、相册、设备指纹、使用时长。
- 设备声明明确拒绝：主机名、用户名、MAC/IMEI/序列号、精确 OS 版本、磁盘路径、IP、UA 原文、文件内容（见 2.2）。
- 不持有：支付卡号、支付密码、爱发电账号密码、GitHub client_secret、授权签名私钥。
- 不植入：广告 SDK、统计/埋点 SDK、崩溃自动上报、遥测、用户画像、跨站追踪、自动化决策。
- **未实现**：实名认证、年龄核验、云同步（本引擎无此能力）、账号体系（身份在 mox-id）、行为分析、对外数据出售。

---

## 四、存多久、存哪、与谁共享

| 数据 | 位置 | 留存 / 删除 |
| --- | --- | --- |
| `device.toml`（设备令牌） | 本机 `$MOX_ROOT`（默认 `~/.mox`） | 由你控制；删除文件 = 换设备，需重新绑定 |
| `entitlements.toml`（授权缓存 + `fetched_at`） | 本机 `$MOX_ROOT` | 由你控制；`ENTS_TTL_DAYS` 后视为陈旧，联网 `mox redeem status` 刷新；删文件即清 |
| `fan.toml`（粉丝档位） | 本机 `$MOX_ROOT` | 由你控制 |
| rootfs / 包缓存 | 本机 | 由你控制，`mox-proot-distro remove` / 删目录 |
| 设备声明、设备码、订单、授权、粉丝码台账 | `mox-id` Cloudflare D1 | 按 **mox-id 隐私政策**：会话 30 天、访客上报 90 天、账号/订单/授权至注销；财务流水注销后匿名化保留 |
| 兑换码 / 订单号 | 提交 `mox-id` 校验并入账 | 随订单台账保留 |

**共享方**（各自独立按其条款处理）：

- **Cloudflare**：`mox-id` 运行于 Workers / D1；官网 Pages 为客户端唯一入口。
- **GitHub / Gitee**：设备码确认页的 OAuth 登录身份；Gitee 亦为国内只读镜像与引擎二进制下载源。
- **爱发电**：按订单号查证已付订单；只传订单号，不传支付凭据。
- **发行版源站与镜像**：Ubuntu cdimage、Canonical LXD images、kali.download、Alpine CDN、清华 TUNA —— 纯文件下载，其访问日志由各自记录。
- **mox-id / mox-site**：同属 `codecloud-dev`，是设备绑定、兑换、授权的实际处理者。

我们**不出售、不出租、不用于广告、不向数据经纪人提供**任何数据。

---

## 五、你能怎么查看、重置、删除

- **查看本机状态**：`mox redeem status`（联网刷新授权缓存）、`mox fan` 相关状态命令、`mox-proot-distro list`。
- **重置设备身份**：删除 `$MOX_ROOT/device.toml`（或整个 `$MOX_ROOT`），重新 `mox redeem login` 绑定；旧设备令牌随之作废。
- **清除授权缓存 / 粉丝状态**：删 `entitlements.toml` / `fan.toml`。
- **删除容器数据**：`mox-proot-distro remove <id>` 或删除容器目录；清 `~/.mox` 可移除全部引擎侧身份与缓存。
- **注销账号 / 删除服务端数据**：在官网个人中心 `DELETE /me`（硬删身份、财务流水匿名化、身份墓碑），**不可恢复**；或按 mox-id 政策的渠道申请按 `device_token` 清理授权。
- **撤销 OAuth 授权**：GitHub 在 `github.com/settings/connections/applications/<client_id>`；Gitee 在其账号设置。
- **撤销设备**：官网 `GET /devices` / 设备绑定页可管理；`mox-id` 侧授权吊销在下次 `redeem status` 刷新时即时生效。
- **阻止联网**：当前版本**未提供细粒度联网开关**；不执行 `login` / `redeem status` / `fan` 与不安装发行版即可完全离线运行已有 rootfs。

---

## 六、儿童

不面向未满 14 周岁的儿童；当前版本**未实现年龄核验**，不有意收集儿童信息。若监护人发现儿童绑定过设备或兑换过授权，请通过 GitHub Issues 联系我们，我们会指导清除本机文件并按 mox-id 政策处理服务端记录。

## 七、跨境

rootfs 下载源站与国内镜像可能位于不同国家/地区；`mox-id` / `mox-site` 运行在 Cloudflare 全球边缘网络，D1 数据可能在你所在国家/地区之外处理；GitHub / Gitee / 爱发电亦按各自条款在其所在地处理收到的数据。本引擎本身只做文件下载与必要握手，不自建跨境通道，不对第三方留存地作承诺。

## 八、第三方服务条款入口（不为其背书）

| 服务 | 条款 / 隐私政策入口 |
| --- | --- |
| GitHub | `https://docs.github.com/en/site-policy/github-terms/github-terms-of-service` · `https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement` |
| Gitee | `https://gitee.com/`（以其站内公布的条款为准） |
| 爱发电 | `https://afdian.com/`（以其站内公布的条款为准；备用域 `afdian.net` / `ifdian.net`） |
| Cloudflare | `https://www.cloudflare.com/privacypolicy/`（及其站内公布的 Terms） |
| Ubuntu / Canonical / Kali / Alpine / 清华 TUNA 镜像 | 见各站公布的条款与日志政策；我们只做文件下载 |

以各站最新公布条款为准，我们不代替其解释、不担保可用性、不对其行为负责。

## 九、变更通知与联系方式

- 变更直接更新本仓库 `PRIVACY.md`，留痕于 Git 提交与 CHANGELOG；不收集联系方式，故**不主动推送**，重大变更在仓库公告。
- **唯一联系方式**：在对应仓库开 **GitHub Issues**（`https://github.com/codecloud-dev/<repo>/issues`），或联系 GitHub 账号 **`codecloud-dev`**。
- 不提供、不编造客服邮箱、电话、通信地址、公司名、注册号。请勿在公开 Issue 里贴设备令牌、兑换码、订单号或密钥。

---
---

# English

**Repository**: `codecloud-dev/moxsh-proot` (PRoot container engine / the `mox` CLI)
**Effective date**: 2026-11-15
**Controller**: `codecloud-dev` — a personal GitHub account run by an individual developer, **not a company or legal entity**; no company name, registration number, registered address, or support phone.

> Chinese first, English after, in one file; the Chinese text controls in case of conflict.
> Written against the **actual code** in `src/proot/*.rs` (especially `backend.rs` / `device_claim.rs` / `redeem.rs` / `fan.rs` / `distro.rs` / `net.rs`), `go/internal/fetch/` and `recipes/index.toml`. Anything not implemented is marked "not implemented / not collected".
> Written in the spirit of GDPR / PIPL minimisation, purpose limitation and transparency; no statutory text is quoted or copied.

## 1. What it does online

| Action | Endpoint / target | Carries user data? |
| --- | --- | --- |
| Download rootfs / distro packages and checksum manifests | Official: `cdimage.ubuntu.com`, `us.lxd.images.canonical.com`, `kali.download`, `dl-cdn.alpinelinux.org`; mainland-China TUNA mirrors (`mirrors.tuna.tsinghua.edu.cn`); `SHA256SUMS` | Pure file download, no account/device identifier |
| Device-code login (RFC 8628) | `POST /device/code`, `POST /device/token`, via the website `https://mox-site.pages.dev` into `mox-id` | 5-field device claim (§2.2) |
| Device binding | One-shot `127.0.0.1` loopback + website `bind.html` (URL carries `redirect` / `state` / `device_token` / `name` / `platform`) → GitHub login → `POST /devices/register` | Device token and coarse device description |
| Redeem code / Afdian order reconciliation | `POST /products/redeem` (logged in), `GET /entitlement/check?device_token=...` | Redeem code or order number + device token |
| Fan-code online verification | `POST /fan/verify` | HS256-signed fan code |
| Container network probes | Ali DNS `223.5.5.5:443` (TCP connect), IPv6 `[2400:3200::1]:53` (UDP connect to establish a route, **no packet sent**) | None; connectivity only |
| Backend candidate domains | Sole client entry `https://mox-site.pages.dev`; upstream `https://mox-id.moxsh.app`; fallback `https://mox-id.3042980037.workers.dev` (`backend.rs`) | — |

> `backend.rs` notes explicitly: the old `https://mox-id.mox-proot.app` is a **never-deployed placeholder domain**, and `codecloud-dev.github.io/mox-site` is a deprecated Pages address.

## 2. Field-by-field data inventory

### 2.1 rootfs / package downloads (`distro.rs` / `go/internal/fetch/fetch.go`)

- The download URL is resolved by host architecture (`aarch64` / `x86_64`) and region (mainland China prefers TUNA mirrors; elsewhere the official source). `curl_fetch()` uses a curl subprocess (on Android the host Kotlin layer downloads via OkHttp / the system network stack and passes a local path), with **resumable downloads** (`Range`) and a live progress callback.
- `SHA256SUMS` is downloaded for integrity verification; `DistroManager::install()` does "download → verify → extract → initialise".
- Distros: Ubuntu Base 24.04, Debian bookworm (LXD images), Kali NetHunter minimal, Alpine minirootfs 3.20.3.
- **These are pure file GETs** with no account, email, device token, or any identifying information; the source / mirror sites log requests under their own policies.
- **Not implemented**: download-behaviour callbacks, install statistics, telemetry, automatic crash reporting.

### 2.2 Device claim (`device_claim.rs` / contract with `src/device_claim.ts`)

`POST /device/code` must carry `X-Mox-Device-Claim` (with the JSON body as fallback when a proxy strips headers), containing **canonical JSON with a fixed field order**:

| Field | Value | Why it is not private |
| --- | --- | --- |
| `device_id` | Locally random 32-hex, stored in `$MOX_ROOT/device.toml`, deletable/resettable | Hardware-independent; not MAC/IMEI/serial |
| `platform` | Compile-time constant `linux` / `android` / `darwin` / `windows` / `unknown` | Coarse OS category |
| `arch` | Compile-time constant `x86_64` / `aarch64` / `armv7` / `i686` / `riscv64` / `unknown` | CPU architecture |
| `client` | Client name such as `mox-proot` | Software identifier |
| `client_version` | Semantic version (from `CARGO_PKG_VERSION`) | Software version |

The backend's `validateDeviceClaim()` accepts only these 5 keys; any extra/privacy field gets 400. The `device_claims` table stores canonical JSON, a SHA-256 hash, a human-readable `label`, and a timestamp. Token polling carries **the same claim**; the server compares the hash so a device code cannot be intercepted and claimed by another device.

**Explicitly never collected or stored**: hostname, username, MAC/IMEI/serial, exact OS version string, disk path, IP, geolocation, raw browser UA, any file contents. `cf-connecting-ip` is used **only for in-memory login rate counting**.

### 2.3 Device-code QR login (`device_code.rs` / `0022`)

`mox-proot login` → `POST /device/code` returns `device_code` + `user_code` + `verification_uri_complete` (`<publicBase>/activate?code=<user_code>`) → the terminal renders a **half-block QR code** → the phone scans it, signs in with **GitHub / Gitee** at `/activate` and taps "Bind this device" → the worker marks it authorised, registers/reuses a `device` row and issues a `device_token` → the terminal polls `POST /device/token`. States: `pending → authorized → consumed / expired`; it becomes `consumed` after token exchange.

Login identity comes from GitHub / Gitee OAuth (profile fields per the mox-id policy); this engine only stores the final `device_token` locally in `device.toml`.

### 2.4 Device binding (`redeem.rs`, steps 1–2)

`mox redeem login` opens a one-shot loopback listener on `127.0.0.1` and launches the browser at `bind.html` with query parameters: `redirect=<loopback>`, `state=<random>`, `device_token=<local random>`, `name=<device name>`, `platform=<platform>`. In the page, **GitHub login** → "Bind this device" = `POST /devices/register` `{device_token, name, platform}` → the page returns to the loopback with `ok=1&state` (state must match, anti-forgery) and the script receives the confirmation automatically.

`device_token` is a **locally random URL-safe string** with no forgeable material; leaking it is equivalent to "switching to another device", not an identity credential.

### 2.5 Redeem codes / Afdian orders / entitlement cache (`redeem.rs` / `products.ts`)

- In the website account centre or redeem page, a buyer uses either a **redeem code** (form `PROOT-7K3M-9QP2-X4F8`) or an **Afdian order number `out_trade_no`** to grant an entitlement to a chosen device via `POST /products/redeem`. The backend verifies the order is genuinely paid and unused, that `device_token` belongs to the logged-in user, and uses a D1 batch for atomic redemption.
- `mox redeem status` → `GET /entitlement/check?device_token=<hex>` (**login-free, the token is the credential**) fetches all entitlements online; the response is plain text, one line per entitlement `ok <product_id> <tier> <exp_unix_seconds>` (`exp=0` = perpetual), plus a server **Ed25519 signature** (public key embedded in the client, verify-only; the private key never reaches the client).
- **Local cache**: `entitlements.toml` (with `fetched_at`); older than `ENTS_TTL_DAYS` is considered stale (the gate shows a lock and asks for a refresh). **Fixed-term subscriptions** (`exp>0`, e.g. the 30-day `proot-pro` monthly) are judged by `exp` even when the cache is stale, so offline use lasts until the expiry day and stops immediately afterwards.
- Local files: `$MOX_ROOT/device.toml` (device token), `$MOX_ROOT/entitlements.toml` (entitlement cache), `$MOX_ROOT/fan.toml` (fan tier). Deleting `device.toml` = switching device, requiring re-binding.
- **Never received**: card numbers, payment passwords, payment-account credentials, Afdian account passwords.

### 2.6 Fan codes (`fan.rs` / `0011`)

`mox fan` uses server-side **HS256-signed** activation codes (`fan_codes` ledger: `code`, `user_id`, `github_id`, `login`, `tier`, `exp`, `issued_at`, `verified_at`, `verify_count`, `revoked`). Each `POST /fan/verify` verifies online and records `verified_at` / `verify_count` (audit + abuse observation); `exp=0` means permanent but the server can `revoked` it at any time. The phone script only `curl`-forwards and does not parse the JWT.

### 2.7 Container network configuration (`net.rs`)

PRoot **does not isolate the network namespace** — the guest uses the host kernel network stack directly. To make DNS work inside the container, the engine:

- Reads host system DNS (passed via C-ABI, cleaned by `sanitize_dns_csv`) or falls back to public DNS: IPv4 `223.5.5.5` (Ali) / `119.29.29.29` (Tencent), IPv6 `2400:3200::1` / `2402:4e00::`;
- Writes `/etc/resolv.conf` inside the rootfs (with `options timeout:2 attempts:2`) and syncs extra entries from the host `/etc/hosts`;
- Uses `std::net` for IPv6 route probing (UDP connect **establishes a route only, sends no packet**) and egress probing (TCP connect to Ali DNS 443, 2-second timeout).

These are **local network configuration and connectivity probes**; no probe result is uploaded anywhere, and no geolocation or network profile is collected.

## 3. What we do not collect (current version)

- Not uploaded: terminal command contents, container file contents, clipboard, browsing history, contacts, location, photo library, device fingerprint, usage duration.
- Device claims explicitly reject: hostname, username, MAC/IMEI/serial, exact OS version, disk paths, IP, raw UA, file contents (§2.2).
- Not held: card numbers, payment passwords, Afdian account passwords, GitHub client_secret, entitlement-signing private key.
- Not embedded: ad SDKs, analytics/tracking SDKs, automatic crash reporting, telemetry, profiling, cross-site tracking, automated decision-making.
- **Not implemented**: real-name verification, age verification, cloud sync (this engine has no such feature), an account system (identity lives in mox-id), behaviour analysis, sale of data abroad.

## 4. Retention, location, and sharing

| Data | Where | Retention / deletion |
| --- | --- | --- |
| `device.toml` (device token) | On-device `$MOX_ROOT` (default `~/.mox`) | User-controlled; deleting it = switching device, requires re-binding |
| `entitlements.toml` (cache + `fetched_at`) | On-device `$MOX_ROOT` | User-controlled; stale after `ENTS_TTL_DAYS`, refreshed by `mox redeem status`; delete the file to clear |
| `fan.toml` (fan tier) | On-device `$MOX_ROOT` | User-controlled |
| rootfs / package caches | On-device | User-controlled; `mox-proot-distro remove` / delete the directory |
| Device claims, device codes, orders, entitlements, fan-code ledger | `mox-id` Cloudflare D1 | Per the **mox-id privacy policy**: sessions 30 days, visitor reporting 90 days, accounts/orders/entitlements until deletion; financial records anonymised after deletion |
| Redeem codes / order numbers | Submitted to `mox-id` for verification and ledgering | Retained with the order ledger |

**Recipients** (each acts under its own terms):

- **Cloudflare**: `mox-id` runs on Workers / D1; the website's Pages is the sole client entry point.
- **GitHub / Gitee**: OAuth identity on the device-code confirmation page; Gitee is also the mainland-China read-only mirror and engine-binary download source.
- **Afdian**: verifies a paid order by order number; only the order number is sent, never payment credentials.
- **Distro sources and mirrors**: Ubuntu cdimage, Canonical LXD images, kali.download, Alpine CDN, TUNA — pure file downloads; each logs requests under its own policy.
- **mox-id / mox-site**: also under `codecloud-dev`, the actual processors of device binding, redemption and entitlements.

We **do not sell, rent, use for advertising, or provide to data brokers** any data.

## 5. How to view, reset and delete

- **View local state**: `mox redeem status` (online refresh of the entitlement cache), `mox fan` state commands, `mox-proot-distro list`.
- **Reset device identity**: delete `$MOX_ROOT/device.toml` (or the whole `$MOX_ROOT`) and re-run `mox redeem login`; the old device token becomes invalid.
- **Clear entitlement cache / fan state**: delete `entitlements.toml` / `fan.toml`.
- **Delete container data**: `mox-proot-distro remove <id>` or delete the container directory; clearing `~/.mox` removes all engine-side identity and caches.
- **Delete account / server-side data**: `DELETE /me` in the website account centre (hard-delete identity, anonymised financial records, identity tombstone), **irreversible**; or use the mox-id policy channels to request entitlement cleanup by `device_token`.
- **Revoke OAuth grants**: GitHub at `github.com/settings/connections/applications/<client_id>`; Gitee in its account settings.
- **Revoke a device**: manage via website `GET /devices` / the device-binding page; server-side revocation takes effect on the next `redeem status` refresh.
- **Blocking network access**: **no fine-grained network toggle in the current version**; simply not running `login` / `redeem status` / `fan` and not installing a distro lets an existing rootfs run fully offline.

## 6. Children

Not directed at children under 14. The current version **has no age verification** and does not knowingly collect children's data. If a guardian finds a child bound a device or redeemed an entitlement, contact us via GitHub Issues; we will advise on clearing local files and on handling server-side records under the mox-id policy.

## 7. Cross-border transfers

Distro source sites and mainland-China mirrors may be in different countries; `mox-id` / `mox-site` run on Cloudflare's global edge and D1 data may be processed outside your country; GitHub / Gitee / Afdian process the data they receive under their own terms and in their own locations. This engine only performs file downloads and the necessary handshakes; it builds no bespoke cross-border channel and makes no promise about third-party data locations.

## 8. Third-party terms (we do not endorse them)

| Service | Terms / privacy policy |
| --- | --- |
| GitHub | `https://docs.github.com/en/site-policy/github-terms/github-terms-of-service` · `https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement` |
| Gitee | `https://gitee.com/` (see its published terms) |
| Afdian | `https://afdian.com/` (see its published terms; fallback domains `afdian.net` / `ifdian.net`) |
| Cloudflare | `https://www.cloudflare.com/privacypolicy/` (and its published Terms) |
| Ubuntu / Canonical / Kali / Alpine / TUNA mirrors | See each site's published terms and logging policy; we only download files |

The latest published terms govern. We do not interpret them, guarantee availability, or take responsibility for them.

## 9. Changes and contact

- Changes update this repository's `PRIVACY.md` directly and appear in Git history / CHANGELOG. We collect no contact details, so **we do not push notifications**; significant changes are announced in the repository.
- **Only contact channels**: open a **GitHub Issue** at `https://github.com/codecloud-dev/<repo>/issues`, or reach the GitHub account **`codecloud-dev`**.
- No support email, phone, postal address, company name, or registration number is provided or invented here. Please do not post device tokens, redeem codes, order numbers, or secrets in a public Issue.
