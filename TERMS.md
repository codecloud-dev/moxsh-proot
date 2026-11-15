# 用户协议 / 服务条款（Terms of Service）

**适用仓库**：`codecloud-dev/moxsh-proot`（PRoot 容器引擎 / `mox` 命令行工具）
**生效日期 / Effective date**：2026-11-15
**协议主体**：`codecloud-dev` —— 一个 GitHub 个人账号，由个人开发者运营，**不是公司、不是法人实体**；没有公司名称、注册号、注册地址或客服电话。

> 中文在前，英文在后，同一份文件；口径一致，歧义以中文为准。
> 本文件与 `PRIVACY.md` 配套：数据行为以 `PRIVACY.md` 为准，使用条件以本文件为准。
> 本仓库代码按 **GPL-3.0**（`LICENSE`）授权。本条款不缩减该许可授予你的任何权利；冲突时以 `LICENSE` 为准。

---

## 一、接受条款

下载、编译、运行本引擎，或执行 `mox-proot-distro install` / `mox login` / `mox redeem` / `mox fan` 等联网命令，即表示你已阅读并同意本条款与 `PRIVACY.md`。不同意请停止使用。

本引擎的**核心 PRoot 运行能力本身免费、开源（GPL-3.0）**；联网部分涉及：设备码扫码登录、设备绑定、兑换码 / 爱发电订单核销、粉丝码校验、增值配方授权（如 `proot-pro` 月度订阅，`0021_seed_proot_pro.sql` 定价 ¥5.99 / 30 天）、rootfs 下载。

## 二、软件按「现状」提供与免责

- 本软件按 **「现状」（AS IS）** 提供，不附带任何明示或默示担保，包括适销性、特定用途适用性、不侵权、不中断、无缺陷、兼容任意宿主 / 内核 / 架构。
- PRoot **不做 network namespace 隔离**，guest 直接共享宿主内核网络栈；这是设计语义而非缺陷。容器内命令、脚本、包管理器、发行版的行为与后果由你自行承担。
- rootfs 与包来自**第三方上游**（Ubuntu / Canonical / Kali / Alpine / 清华 TUNA 镜像）；我们只做下载、SHA256 校验、解压与初始化，**不担保其内容、安全性、可用性、许可合规性或长期可下载**。
- 在适用法律允许的最大范围内，我们不对任何直接、间接、附带、特殊或后果性损失（含数据丢失、设备损坏、业务中断、利润损失）承担责任，即使已被告知可能发生。
- 当前版本**未实现**：自动崩溃上报、远程擦除、官方技术支持 SLA、数据恢复、商业 SLA / 赔偿。

## 三、可接受使用（AUP）

- 遵守你所在国家/地区的法律，以及 GitHub / Gitee / 爱发电 / Cloudflare / 各发行版源站与镜像的条款。
- 不利用本引擎入侵他人系统、抓取未授权数据、传播恶意软件、挖矿滥用、发起拒绝服务、规避网络管制或实施诈骗。
- 不把本引擎作为绕过其它服务授权、计费、访问控制、兑换码或设备绑定机制的工具。
- 自行保管 `$MOX_ROOT/device.toml` 里的 `device_token`、兑换码、爱发电订单号、`fan.toml` 里的粉丝码；**不要贴到公开 Issue、日志、截图或公开仓库**。
- 兑换码与粉丝码**仅供你本人账号 / 你本人设备使用**；共享、转卖、公开传播导致的封禁、吊销或无法兑换，由你自行承担。
- 增值配方（如 GPU 透传、嵌套容器、systemd 常驻、快照克隆）按授权门控；绕过门控即违反第四节。
- 我们保留在明显滥用时拒绝签发登录码、吊销授权、拒绝兑换的权利，但**无义务监控**，也不为第三方行为背书。

## 四、禁止逆向滥用与绕过授权

- 允许：阅读、修改、再分发本仓库代码 —— 这些是 **GPL-3.0 明确授予的权利**，本条款不限制；为互操作性而进行的必要分析亦不受限。
- 禁止的是**以规避授权与安全机制为目的的滥用**，包括但不限于：
  - 伪造 / 篡改 / 重放 `device_token`、`device_code`、`user_code`、OAuth 会话，冒领他人设备码或冒充他人登录；
  - 伪造或篡改 `entitlements.toml` 缓存、`exp` 到期时间、`sig` 签名行，或反编译客户端以绕过 **Ed25519 授权签名校验**（私钥只在服务端）；
  - 修改 `device_claim` 的 5 字段白名单、伪造设备声明以冒领登录码，或绕过 `X-Mox-Device-Claim` 校验；
  - 伪造兑换码、批量爆破兑换码、复用已核销订单号、绕过 `/products/redeem` 的登录态与设备归属校验；
  - 伪造 / 篡改 HS256 粉丝码或绕过 `/fan/verify` 在线校验、规避 `verify_count` 审计与 `revoked` 吊销；
  - 修改客户端使订阅型授权（`exp>0`）在到期后仍继续生效、绕过 7 天缓存 TTL 与服务端吊销刷新；
  - 以任何方式规避本系列（moxsh-terminal / mox-id / Fenestra / mox-site）已有的商业授权或付费门禁。
- 上述禁止不扩张为「禁止一切逆向」：GPL-3.0 授予的研究、修改、再分发权利完整保留；被禁止的是**用于绕过授权、侵犯他人或规避付费**的具体滥用行为。

## 五、兑换码、设备绑定与授权条款

- **设备绑定**：`device_token` 是本地随机串，绑定在你的账号下；删除 `$MOX_ROOT/device.toml` 等于换设备，需重新 `mox redeem login`。一个用户对一个商品只有一条授权（`product_entitlements` 的 `UNIQUE(user_id, product_id)`）；**换绑每月限 1 次**（`rebinds` / `rebind_month`），防共享传播。
- **兑换**：兑换码或爱发电订单号二选一，必须「订单真实已付 + 未被核销 + 设备属于登录用户」才生效；重复兑换返回 `already_redeemed`，已吊销返回 `code_revoked`，未查到订单返回 `order_not_found`。我们**不接收银行卡号、支付密码或爱发电账号密码**。
- **订阅与到期**：订阅型商品（如 `proot-pro`，`duration_days=30`）自兑换时刻起算，**到期自动失效**；客户端即便离线也按 `exp` 判定，到期当天过后立即失效（`redeem.rs` 尊重有限 `exp`）。永久授权（`exp=0`）靠缓存 TTL 与服务端吊销维持可撤销性。
- **授权可撤销**：服务端可 `revoke` 授权 / 粉丝码；`mox redeem status` 联网刷新时即时生效。缓存陈旧时门控显示锁定并提示刷新。
- **退款 / 账务**：当前版本**未实现**在线退款、自动续费、发票开具与价格争议仲裁；涉及付费的问题请通过第九节的 GitHub Issues 沟通，并遵循爱发电等平台自身的规则。
- 本条不因 GPL-3.0 对代码的授权而免除：代码可自由使用，**商品授权与兑换权益是独立于代码许可的服务层门禁**。

## 六、专项：插件版本安全策略（与终端同口径，用户义务）

> 该机制由本项目另一条开发线实现；本条款**只规定用户义务，不描述、不承诺其具体实现方式与当前进度，也不虚构任何实现细节**；实现行为以届时发布的客户端与可信目录声明为准。moxsh-proot 的 `.mox` 增值配方 / 插件同样受此约束。

1. **禁止回滚到存在已知漏洞的旧版本**：被标记为存在已知安全漏洞的插件 / 配方版本，你不得降级、回滚、侧载、强制安装或伪装成新版本。
2. **不得绕过该机制**：不得禁用、篡改、拦截或伪造版本安全校验，不得用非官方渠道包绕过校验。
3. **后果**：客户端可拒绝安装 / 禁用被标记版本；由此导致的不可用按第二节「按现状提供」由你自行承担。合规路径：升级到修复版本、卸载、或从官方源重装。

## 七、第三方服务（链接指向，不为其背书）

| 服务 | 用途 | 条款 / 隐私政策入口 |
| --- | --- | --- |
| GitHub | 设备码确认页 OAuth 登录、主仓与 Issues | `https://docs.github.com/en/site-policy/github-terms/github-terms-of-service` · `https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement` |
| Gitee | 平行 OAuth 入口、国内只读镜像、引擎二进制下载 | `https://gitee.com/`（以其站内公布条款为准） |
| 爱发电 | 赞助、订单号查证与核销 | `https://afdian.com/`（以其站内公布条款为准；备用域 `afdian.net` / `ifdian.net`） |
| Cloudflare | `mox-id` Workers / D1、官网 Pages | `https://www.cloudflare.com/privacypolicy/`（及其站内公布的 Terms） |
| 发行版源站 / 镜像 | rootfs 与 `SHA256SUMS` 下载 | Ubuntu cdimage、Canonical LXD images、kali.download、Alpine CDN、清华 TUNA —— 见各站公布条款 |

以各站最新公布条款为准；我们**不代替它们解释、不担保可用性、不对其行为负责、不为其背书**。第三方中断、改版、限流、域名切换（如爱发电主域备案变化）或条款变化，我们不承担责任。

## 八、知识产权与许可

- 本仓库代码按 **GNU GPL v3.0**（`LICENSE`）授权：可阅读、修改、再分发；分发派生版本须遵守 GPL-3.0 同等许可与源码提供义务。**本条款不叠加额外限制、不收费、不改变 GPL-3.0**。
- 兑换码、粉丝码、商品授权、`proot-pro` 等增值权益的**服务层规则独立于代码许可**，见第五节；GPL-3.0 授予的是代码权利，不是免费获得付费权益或伪造授权的许可。
- `mox` / `moxsh-proot` 名称、品牌标识与文档版权归 `codecloud-dev`；GPL-3.0 对代码的授权不等于商标授权。
- 你提交的 Issue / PR 视为同意按 `CONTRIBUTING.md` 与 GPL-3.0 公开处理。

## 九、变更、终止与争议

- 变更直接更新本仓库 `TERMS.md`，留痕于 Git 提交与 CHANGELOG；不收集联系方式，故**不主动推送**，重大变更在仓库公告。继续使用视为接受。
- 你可随时终止：停止使用、删除 `$MOX_ROOT`（含 `device.toml` / `entitlements.toml` / `fan.toml`）、`mox-proot-distro remove <id>` 删容器、在官网 `DELETE /me` 注销账号（**不可恢复**）。终止不自动删除第三方（爱发电订单、GitHub 授权、D1 匿名化对账流水）侧记录。
- 因不可抗力、上游源站失效、镜像同步失败、Cloudflare / GitHub / Gitee / 爱发电服务终止、平台下架导致的功能不可用，我们不承担责任。
- 争议应先友好协商；协商不成，提交**版权人所在地有管辖权的法院**处理。本条款以中文书写，英文仅为阅读便利，歧义以中文为准。部分条款无效不影响其余条款效力。

## 十、联系方式

- **唯一联系方式**：在对应仓库开 **GitHub Issues**（`https://github.com/codecloud-dev/<repo>/issues`），或联系 GitHub 账号 **`codecloud-dev`**。
- 不提供、不编造客服邮箱、电话、通信地址、公司名、注册号。请勿在公开 Issue 里贴 `device_token`、兑换码、订单号、粉丝码或密钥。

---
---

# English — Terms of Service

**Repository**: `codecloud-dev/moxsh-proot` (PRoot container engine / the `mox` CLI)
**Effective date**: 2026-11-15
**Party**: `codecloud-dev` — a personal GitHub account run by an individual developer, **not a company or legal entity**; no company name, registration number, registered address, or support phone.

> Chinese first, English after, in one file; the Chinese text controls in case of conflict.
> Pairs with `PRIVACY.md`: data practices are governed by `PRIVACY.md`, terms of use by this document.
> Licensed under **GPL-3.0** (`LICENSE`). These terms do not reduce any right that licence grants you; where this document conflicts with `LICENSE`, `LICENSE` controls.

## 1. Acceptance

Downloading, compiling, running the engine, or executing online commands such as `mox-proot-distro install` / `mox login` / `mox redeem` / `mox fan` means you have read and agree to these terms and to `PRIVACY.md`. If you do not agree, stop using it.

The **core PRoot runtime capability itself is free and open source (GPL-3.0)**. Online parts involve: device-code QR login, device binding, redeem-code / Afdian-order reconciliation, fan-code verification, paid-recipe entitlements (e.g. the `proot-pro` monthly subscription, priced ¥5.99 / 30 days in `0021_seed_proot_pro.sql`), and rootfs downloads.

## 2. "As is" and disclaimer

- Provided **"AS IS"**, without any express or implied warranty of merchantability, fitness for a particular purpose, non-infringement, uninterrupted operation, defect-free operation, or compatibility with any host / kernel / architecture.
- PRoot **does not isolate the network namespace** — the guest shares the host kernel network stack directly; this is by design, not a defect. You bear the behaviour and consequences of commands, scripts, package managers, and distros inside containers.
- rootfs and packages come from **third-party upstreams** (Ubuntu / Canonical / Kali / Alpine / TUNA mirrors); we only download, SHA256-verify, extract and initialise, and **do not warrant their content, safety, availability, licence compliance, or long-term downloadability**.
- To the maximum extent permitted by law, we are not liable for any direct, indirect, incidental, special, or consequential loss (including data loss, device damage, business interruption, lost profit), even if advised of the possibility.
- **Not implemented**: automatic crash reporting, remote wipe, an official support SLA, data recovery, or a commercial SLA / indemnity.

## 3. Acceptable use (AUP)

- Comply with the law of your country/region and with the terms of GitHub / Gitee / Afdian / Cloudflare / distro sources and mirrors.
- Do not use the engine to intrude into others' systems, scrape unauthorised data, spread malware, abuse mining, launch denial-of-service, evade network controls, or commit fraud.
- Do not use it as a tool to bypass another service's authorisation, billing, access control, redeem-code, or device-binding mechanisms.
- Keep your `device_token` in `$MOX_ROOT/device.toml`, redeem codes, Afdian order numbers, and fan codes in `fan.toml` to yourself; **do not paste them into public Issues, logs, screenshots, or public repositories**.
- Redeem codes and fan codes are **for your own account / your own device only**; sharing, reselling, or publishing them, and any resulting bans, revocation, or failed redemption, are your own responsibility.
- Paid recipes (e.g. GPU passthrough, nested containers, systemd residency, snapshot cloning) are gated by entitlements; bypassing the gate violates §4.
- We reserve the right to refuse issuing login codes, revoke entitlements, or refuse redemption in cases of obvious abuse, but have **no duty to monitor** and do not endorse third-party conduct.

## 4. No abusive reverse engineering / no bypassing authorisation

- Allowed: reading, modifying and redistributing this repository's code — **rights expressly granted by GPL-3.0**, unrestricted by these terms; analysis necessary for interoperability is likewise unrestricted.
- Prohibited is **abuse aimed at circumventing authorisation and security mechanisms**, including but not limited to:
  - Forging / tampering with / replaying `device_token`, `device_code`, `user_code`, or OAuth sessions to claim someone else's device code or impersonate a login;
  - Forging or tampering with the `entitlements.toml` cache, `exp` expiry, or `sig` signature line, or decompiling the client to bypass **Ed25519 entitlement signature verification** (private key server-side only);
  - Altering the 5-field `device_claim` whitelist, forging a device claim to obtain a login code, or bypassing `X-Mox-Device-Claim` validation;
  - Forging redeem codes, brute-forcing them, reusing an already-reconciled order number, or bypassing `/products/redeem`'s login state and device-ownership checks;
  - Forging / tampering with HS256 fan codes, bypassing `/fan/verify` online verification, or evading `verify_count` auditing and `revoked` revocation;
  - Modifying the client so a subscription entitlement (`exp>0`) remains effective after expiry, or bypassing the 7-day cache TTL and server-side revocation refresh;
  - In any way evading existing commercial-licensing or paywall gates of this series (moxsh-terminal / mox-id / Fenestra / mox-site).
- This does **not** expand into "all reverse engineering is forbidden": the research, modification and redistribution rights granted by GPL-3.0 remain intact; what is prohibited is the specific abusive conduct of bypassing authorisation, infringing others, or evading payment.

## 5. Redeem codes, device binding and entitlements

- **Device binding**: `device_token` is a locally random string bound to your account; deleting `$MOX_ROOT/device.toml` equals switching device and requires re-running `mox redeem login`. A user has only one entitlement per product (`UNIQUE(user_id, product_id)` in `product_entitlements`); **rebind is limited to once per month** (`rebinds` / `rebind_month`) to prevent sharing.
- **Redemption**: either a redeem code or an Afdian order number, valid only if "the order is genuinely paid + unused + the device belongs to the logged-in user". Duplicate redemption returns `already_redeemed`, a revoked code `code_revoked`, an unknown order `order_not_found`. We **never receive card numbers, payment passwords, or Afdian account passwords**.
- **Subscription and expiry**: subscription products (e.g. `proot-pro`, `duration_days=30`) run from the moment of redemption and **expire automatically**; the client judges by `exp` even offline and stops immediately after the expiry day (`redeem.rs` honours finite `exp`). Perpetual entitlements (`exp=0`) rely on cache TTL and server-side revocation to remain revocable.
- **Revocable**: the server may `revoke` entitlements / fan codes; this takes effect on the next online `mox redeem status` refresh. A stale cache shows a locked gate and prompts for a refresh.
- **Refunds / billing**: the current version **does not implement** online refunds, auto-renewal, invoicing, or price-dispute arbitration; for payment issues, contact us via §10 GitHub Issues and follow the rules of Afdian and other platforms.
- This clause is not waived by GPL-3.0's grant over the code: the code may be used freely, but **product entitlements and redemption rights are service-layer gates independent of the code licence**.

## 6. Special clause: plugin version security policy (same as the terminal, user obligations)

> The mechanism is implemented by another workstream of this project; these terms **state only user obligations and do not describe, promise, or invent any implementation detail or current status**. Actual behaviour follows the client released at that time and the trusted catalogue declarations. `.mox` paid recipes / plugins of moxsh-proot are equally bound.

1. **No rollback to versions with known vulnerabilities**: for a plugin / recipe version flagged as having a known security vulnerability, you must not downgrade, roll back, sideload, force-install, or disguise it as a newer version.
2. **No bypassing the mechanism**: you must not disable, tamper with, intercept, or forge the version security check, nor use non-official packages to bypass it.
3. **Consequences**: the client may refuse to install / disable a flagged version; any resulting unavailability is borne by you under §2 ("as is"). Compliant path: upgrade to a fixed version, uninstall, or reinstall from the official source.

## 7. Third-party services (links only, no endorsement)

| Service | Purpose | Terms / privacy policy |
| --- | --- | --- |
| GitHub | OAuth login on the device-code confirmation page, canonical repo and Issues | `https://docs.github.com/en/site-policy/github-terms/github-terms-of-service` · `https://docs.github.com/en/site-policy/privacy-policies/github-privacy-statement` |
| Gitee | Parallel OAuth entry, mainland-China read-only mirror, engine binary download | `https://gitee.com/` (see its published terms) |
| Afdian | Sponsorship, order-number verification and reconciliation | `https://afdian.com/` (see its published terms; fallback domains `afdian.net` / `ifdian.net`) |
| Cloudflare | `mox-id` Workers / D1, website Pages | `https://www.cloudflare.com/privacypolicy/` (and its published Terms) |
| Distro sources / mirrors | rootfs and `SHA256SUMS` downloads | Ubuntu cdimage, Canonical LXD images, kali.download, Alpine CDN, TUNA — see each site's published terms |

The latest published terms govern; we **do not interpret them, guarantee availability, take responsibility for them, or endorse them**. We are not liable for third-party outages, changes, rate limits, domain switches (e.g. Afdian's main-domain filing changes), or term changes.

## 8. Intellectual property and licence

- Repository code is licensed under **GNU GPL v3.0** (`LICENSE`): you may read, modify and redistribute it; distributing derivative versions must comply with GPL-3.0's same-licence and source-provision duties. **These terms add no extra restrictions, charge no fee, and do not change GPL-3.0**.
- The **service-layer rules** for redeem codes, fan codes, product entitlements and `proot-pro` value-added rights are independent of the code licence (§5); GPL-3.0 grants rights over code, not a licence to obtain paid rights for free or to forge entitlements.
- The `mox` / `moxsh-proot` names, branding and documentation are copyright `codecloud-dev`; a GPL-3.0 grant over code is not a trademark grant.
- Content you submit as Issues / PRs is treated as agreed to be handled publicly under `CONTRIBUTING.md` and GPL-3.0.

## 9. Changes, termination and disputes

- Changes update this repository's `TERMS.md` directly and appear in Git history / CHANGELOG. We collect no contact details, so **we do not push notifications**; significant changes are announced in the repository. Continued use constitutes acceptance.
- You may terminate at any time: stop using it, delete `$MOX_ROOT` (including `device.toml` / `entitlements.toml` / `fan.toml`), `mox-proot-distro remove <id>` to delete containers, and `DELETE /me` on the website to delete your account (**irreversible**). Termination does not automatically delete records on the third-party side (Afdian orders, GitHub grants, anonymised reconciliation records in D1).
- We are not liable for unavailability caused by force majeure, upstream source failure, mirror sync failure, termination of Cloudflare / GitHub / Gitee / Afdian services, or platform takedown.
- Disputes should first be resolved amicably; failing that, they are submitted to the **court of competent jurisdiction at the copyright holder's location**. These terms are written in Chinese; the English text is for convenience only and the Chinese controls. Invalidity of one clause does not affect the rest.

## 10. Contact

- **Only contact channels**: open a **GitHub Issue** at `https://github.com/codecloud-dev/<repo>/issues`, or reach the GitHub account **`codecloud-dev`**.
- No support email, phone, postal address, company name, or registration number is provided or invented here. Please do not post `device_token`, redeem codes, order numbers, fan codes, or secrets in a public Issue.
