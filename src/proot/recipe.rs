//! 配方系统（0.3.0，docs/roadmap.md §4）：声明式 TOML 索引 + shell 逃生舱。
//!
//! 设计哲学（clean-room，参考 tmoe 但重写并优化）：
//! - 索引 `recipes/index.toml` 用与 settings.rs 同源的手写极简 TOML 解析，
//!   **零外部依赖**（不引 serde/toml），数组表 `[[recipe]]` 驱动。
//! - 每个配方 = 一条元数据 + 一个 `script = "id.sh"` 逃生舱脚本（真正在容器内
//!   跑的安装逻辑，见 recipes/*.sh）。比 tmoe 的 `DEPENDENCY_01/02` 两个硬编码
//!   槽位扩展性强得多——新增配方只改 index.toml + 加一个 .sh。
//! - 应用配方时把脚本（连同公共库 lib.sh）落进容器 rootfs 的 /tmp，再经
//!   `cli::exec_into` 在容器内执行，复用引擎的 PATH/环境/控制台绑定，不重复造轮子。
//! - 比 tmoe 多：执行统一走 `exec_into`（tmoe 在 awk/bash 里各自实现一遍）；
//!   id 走同一套 `valid_id` 白名单防穿越（见 distro.rs）。

use std::path::{Path, PathBuf};

use crate::proot::cli;
use crate::proot::distro::DistroManager;
use crate::proot::{ProotError, ProotResult};

/// 编译期内嵌索引（仓库根 `recipes/index.toml`）。从目录加载失败时的兜底，
/// 保证 `recipe list` 永远有内容；脚本内容则在运行时按目录/路径再解析。
const EMBEDDED_INDEX: &str = include_str!("../../recipes/index.toml");
/// 编译期内嵌公共库（recipes/lib.sh）——落进容器 /tmp，供各配方 source。
const EMBEDDED_LIB: &str = include_str!("../../recipes/lib.sh");

/// 单个配方元数据。
#[derive(Debug, Clone, Default)]
pub struct Recipe {
    pub id: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    /// 逃生舱脚本文件名（相对配方目录）。
    pub script: String,
    /// 粉丝专属门禁：true 时 `apply` 需 `fan::is_fan` 解锁才放行
    /// （index.toml 里 `fan_only = true`；见 fan.rs「脚本↔官网」握手）。
    pub fan_only: bool,
    /// 商品授权门禁：非空时 `apply` 需 `redeem::has_entitlement` 有该商品授权
    /// 才放行（index.toml 里 `requires = "<product_id>"`；与 fan_only 并列，
    /// 付费商品后台可加——见 redeem.rs）。空串 = 无门禁。
    pub requires: String,
}

/// 配方索引（解析结果）。
#[derive(Debug, Clone, Default)]
pub struct RecipeIndex {
    pub recipes: Vec<Recipe>,
    /// 配方脚本所在目录（用于解析 `script` 相对路径；内嵌时为 None）。
    dir: Option<PathBuf>,
}

impl RecipeIndex {
    /// 解析索引文本（手写 TOML 子集：`[[recipe]]` 数组表 + 简单键值）。
    ///
    /// **安全约束（渗透测试 P0-2）**：`id` 会拼进容器内落盘文件名与执行
    /// 命令串，`script` 会用于 `dir.join(script)` 读文件——二者都来自用户
    /// 可写的 index.toml，必须过 [`DistroManager::valid_id`] 白名单：
    /// - `id` 含 shell 元字符（`;`/`#`/空格）或 `..` 等：整条配方丢弃，
    ///   杜绝 `id = "zz;echo PWNED;#"` 注入任意命令 / `../` 路径穿越；
    /// - `script` 含 `/`、`..` 或元字符：置空作废，`read_script` 会拒绝，
    ///   杜绝 `script = "../../etc/passwd"` 读任意文件。
    ///
    /// 与 distro.rs 的 id 白名单同源，避免两套规则产生绕过缝隙。
    pub fn parse(text: &str) -> RecipeIndex {
        let mut recipes = Vec::new();
        let mut cur: Option<Recipe> = None;
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if line.is_empty() {
                continue;
            }
            if line == "[[recipe]]" {
                if let Some(r) = cur.take() {
                    push_valid(&mut recipes, r);
                }
                cur = Some(Recipe::default());
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if let Some(r) = cur.as_mut() {
                    let key = k.trim();
                    let val = unquote(v.trim()).to_string();
                    match key {
                        // 非法 id 直接作废（置空），push_valid 会据此丢弃整条。
                        "id" => r.id = if DistroManager::valid_id(&val) { val } else { String::new() },
                        "name" => r.name = val,
                        "description" => r.description = val,
                        // 非法脚本名作废（置空），read_script 会拒绝读取。
                        "script" => {
                            r.script = if DistroManager::valid_id(&val) { val } else { String::new() }
                        }
                        "tags" => {
                            r.tags = val
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect()
                        }
                        "fan_only" => r.fan_only = val == "true",
                        "requires" => r.requires = val,
                        _ => {}
                    }
                }
            }
        }
        if let Some(r) = cur.take() {
            push_valid(&mut recipes, r);
        }
        RecipeIndex { recipes, dir: None }
    }

    /// 从目录加载：读 `<dir>/index.toml`，记录脚本目录。
    pub fn from_dir(dir: &Path) -> ProotResult<RecipeIndex> {
        let text = std::fs::read_to_string(dir.join("index.toml")).map_err(ProotError::Io)?;
        let mut idx = RecipeIndex::parse(&text);
        idx.dir = Some(dir.to_path_buf());
        Ok(idx)
    }

    /// 运行时加载：按环境变量与可执行文件同目录探测配方目录，
    /// 找不到则回退到内嵌索引（脚本内容以 `script` 文件名在候选路径查找）。
    ///
    /// **安全约束（防门禁绕过）**：用户可写目录 / `MOX_RECIPE_DIR` 只能*增*配方，
    /// 不能*降级或移除*内嵌付费/专属配方的门禁。任何与内嵌同 `id` 的配方，
    /// 其 `requires` / `fan_only` 强制取内嵌值——即便攻击者在磁盘索引里
    /// `sed '/requires/d'` 删掉那一行，门禁依旧生效（见渗透测试报告·路径②）。
    pub fn load() -> RecipeIndex {
        let mut disk = None;
        if let Ok(d) = std::env::var("MOX_RECIPE_DIR") {
            if let Ok(idx) = RecipeIndex::from_dir(Path::new(&d)) {
                disk = Some(idx);
            }
        }
        if disk.is_none() {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(bin) = exe.parent() {
                    // <bin>/../share/mox-proot/recipes（安装布局）
                    let candidate = bin.join("../share/mox-proot/recipes");
                    if let Ok(idx) = RecipeIndex::from_dir(&candidate) {
                        disk = Some(idx);
                    }
                }
            }
        }
        let mut idx = match disk {
            Some(d) => d,
            None => RecipeIndex::parse(EMBEDDED_INDEX),
        };
        // 内嵌索引的门禁表（id → (requires, fan_only)）为权威来源。
        let emb = RecipeIndex::parse(EMBEDDED_INDEX);
        let mut gate: std::collections::HashMap<&str, (&str, bool)> =
            std::collections::HashMap::with_capacity(emb.recipes.len());
        for r in &emb.recipes {
            gate.insert(r.id.as_str(), (r.requires.as_str(), r.fan_only));
        }
        for r in &mut idx.recipes {
            if let Some((req, fo)) = gate.get(r.id.as_str()) {
                r.requires = (*req).to_string();
                r.fan_only = *fo;
            }
        }
        idx
    }

    /// 所有配方。
    pub fn all(&self) -> &[Recipe] {
        &self.recipes
    }

    /// 按 id 查找配方。
    pub fn find(&self, id: &str) -> ProotResult<&Recipe> {
        self.recipes
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| ProotError::DistroUnknown(format!("配方 {}", id)))
    }

    /// 读取某配方的逃生舱脚本内容（供 `show` 与 `apply` 共用）。
    pub fn script_content(&self, id: &str) -> ProotResult<String> {
        let recipe = self.find(id)?;
        self.read_script(recipe)
    }

    /// 读取脚本：优先配方目录，其次可执行文件同目录 share 路径。
    fn read_script(&self, recipe: &Recipe) -> ProotResult<String> {
        // 付费配方（requires 非空）：脚本内嵌混淆存储，不在磁盘落明文，
        // 仅在门禁（requires 判定）通过后由本函数解码使用——杜绝直接
        // `sh recipes/pro-*.sh` 绕过门禁（渗透测试报告·路径③）。
        if !recipe.requires.is_empty() {
            return crate::proot::recipe_paid::decode_paid(&recipe.id).ok_or_else(|| {
                ProotError::RootfsMissing(format!("付费配方脚本 {}", recipe.id))
            });
        }
        // 【P0-2】脚本名二次白名单：即便有人绕过 parse 直接构造 Recipe，
        // 也阻止 `dir.join(script)` 借 `../` 逃出配方目录读任意文件。
        // 付费配方走上面的内嵌解码分支，不依赖 script 字段。
        if !DistroManager::valid_id(&recipe.script) {
            return Err(ProotError::InvalidArg(format!(
                "配方脚本名非法（仅允许字母数字 ._-）：{}",
                recipe.id
            )));
        }
        if let Some(dir) = &self.dir {
            let p = dir.join(&recipe.script);
            if p.is_file() {
                return std::fs::read_to_string(&p).map_err(ProotError::Io);
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(bin) = exe.parent() {
                let p = bin
                    .join("../share/mox-proot/recipes")
                    .join(&recipe.script);
                if p.is_file() {
                    return std::fs::read_to_string(&p).map_err(ProotError::Io);
                }
            }
        }
        Err(ProotError::RootfsMissing(format!("配方脚本 {}", recipe.script)))
    }

    /// 应用配方：把公共库 + 脚本落进目标容器的 /tmp，再在容器内执行。
    ///
    /// - `root`：mox-proot 数据根（`MOX_ROOT` 解析后）。
    /// - `recipe_id`：配方 id。
    /// - `distro_id`：已安装发行版 id（未装报错）。
    ///
    /// 落盘位置 `rootfs/tmp/*`：宿主视角写 = 容器内 `/tmp/*` 视角读，
    /// 执行后由引擎 env 决定的 HOME 无关，幂等可重跑。
    pub fn apply(&self, root: &Path, recipe_id: &str, distro_id: &str) -> ProotResult<i32> {
        let recipe = self.find(recipe_id)?;
        // 统一门禁（粉丝专属 + 付费商品授权融合为单一裁决点，见 `access`）：
        // 未解锁直接拒，不碰引擎、不落盘。失败原因分两类给出可读提示。
        if !crate::proot::access::recipe_unlocked(root, recipe) {
            if recipe.fan_only && !crate::proot::fan::is_fan(root) {
                return Err(ProotError::InvalidArg(format!(
                    "配方 {} 为专属内容，需先解锁（mox fan redeem <code>）",
                    recipe_id
                )));
            }
            return Err(ProotError::InvalidArg(format!(
                "配方 {} 为付费商品内容（{}），需先获得授权：\n\
                 \x20 mox redeem login    # 绑定本机设备（首次）\n\
                 \x20 mox redeem status   # 联网刷新授权缓存\n\
                 \x20 购买/兑换见官网 redeem 页",
                recipe_id, recipe.requires
            )));
        }
        let mgr = DistroManager::new(root);
        let rootfs = mgr.distro_rootfs(distro_id);
        if !rootfs.is_dir() {
            return Err(ProotError::DistroNotInstalled(distro_id.to_string()));
        }
        let script = self.read_script(recipe)?;
        let tmp = rootfs.join("tmp");
        std::fs::create_dir_all(&tmp).map_err(ProotError::Io)?;
        // 公共库先落，配方脚本首行 `source /tmp/mox-proot-lib.sh` 即生效。
        std::fs::write(tmp.join("mox-proot-lib.sh"), EMBEDDED_LIB).map_err(ProotError::Io)?;
        let script_name = format!("mox-proot-recipe-{}.sh", recipe.id);
        std::fs::write(tmp.join(&script_name), &script).map_err(ProotError::Io)?;
        // 【渗透测试 P0-2 修复】旧实现把 `recipe.id` 拼进 `sh -c` 命令串：
        //   `sh /tmp/mox-proot-recipe-{id}.sh && echo '配方 {id} 完成' ...`
        // 复测实测 `id = "zz;echo RECIPE_ID_INJECTION_PROOF;#"` 会在容器内执行
        // 注入命令。现在改为**直接 argv** 把脚本交给容器内 `/bin/sh`，任何
        // id 字符都只是独立参数/文件名，绝不进 shell 语法树；再叠加 parse /
        // read_script 的 `valid_id` 白名单，双重封死命令注入与路径穿越。
        let tail = vec!["/bin/sh".to_string(), format!("/tmp/{}", script_name)];
        let code = cli::exec_into(root, distro_id, &tail)?;
        // 成功/失败提示改由 Rust 侧按退出码给出（不再借 shell 字符串拼接），
        // 且脚本真实退出码得以保留——旧 `sh -c "…|| echo 失败"` 恒返回 0。
        if code == 0 {
            eprintln!("mox-proot: 配方 {} 完成", recipe.id);
        } else {
            eprintln!("mox-proot: 配方 {} 失败（退出码 {}）", recipe.id, code);
        }
        Ok(code)
    }
}

/// 仅接纳 id 合法的配方（P0-2）：非法 id 在 parse 里被置空，这里整条丢弃。
/// 这样 `find`/`apply` 永远拿不到带 shell 元字符或 `..` 的 id。
fn push_valid(recipes: &mut Vec<Recipe>, r: Recipe) {
    if DistroManager::valid_id(&r.id) {
        recipes.push(r);
    }
}

// ---- 复用 settings.rs 的同款手写 TOML 小工具（引号内 # 不切） ----
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# 配方索引
[[recipe]]
id = "xfce"
name = "XFCE 桌面"
description = "轻量 GTK 桌面"
tags = "desktop, gui"
script = "xfce.sh"

[[recipe]]
id = "vnc"
name = "VNC 服务"
description = "tigervnc"
tags = "gui,remote"
script = "vnc.sh"
"#;

    #[test]
    fn parse_index_and_metadata() {
        let idx = RecipeIndex::parse(SAMPLE);
        assert_eq!(idx.recipes.len(), 2);
        let xfce = idx.find("xfce").unwrap();
        assert_eq!(xfce.name, "XFCE 桌面");
        assert_eq!(xfce.tags, vec!["desktop", "gui"]);
        assert_eq!(xfce.script, "xfce.sh");
        assert_eq!(idx.find("vnc").unwrap().description, "tigervnc");
        // 未知 id 报错
        assert!(idx.find("nope").is_err());
    }

    #[test]
    fn from_dir_resolves_script() {
        let dir = std::env::temp_dir().join("mox-proot-recipe-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.toml"), SAMPLE).unwrap();
        std::fs::write(dir.join("xfce.sh"), "#!/bin/sh\necho xfce\n").unwrap();
        std::fs::write(dir.join("vnc.sh"), "#!/bin/sh\necho vnc\n").unwrap();

        let idx = RecipeIndex::from_dir(&dir).unwrap();
        assert_eq!(idx.dir.as_ref().unwrap(), &dir);
        let content = idx.script_content("xfce").unwrap();
        assert!(content.contains("echo xfce"));
        assert!(idx.script_content("nope").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_missing_rootfs_errors_without_running_engine() {
        // 容器未安装 → DistroNotInstalled，不触碰 proot 引擎（CI 无 proot 二进制）。
        let dir = std::env::temp_dir().join("mox-proot-recipe-apply-test");
        let _ = std::fs::remove_dir_all(&dir);
        let idx = RecipeIndex::parse(SAMPLE);
        let root = dir.join("mox-proot-root");
        let _ = std::fs::remove_dir_all(&root);
        let err = idx.apply(&root, "xfce", "ubuntu").unwrap_err();
        assert!(matches!(err, ProotError::DistroNotInstalled(_)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_fan_only_flag() {
        let on = RecipeIndex::parse(
            "[[recipe]]\nid=\"fb\"\nname=\"x\"\nscript=\"fb.sh\"\nfan_only = true\n",
        );
        assert!(on.find("fb").unwrap().fan_only);
        // 未标注 → 默认 false（普通配方不受影响）。
        let off = RecipeIndex::parse("[[recipe]]\nid=\"a\"\nname=\"x\"\nscript=\"a.sh\"\n");
        assert!(!off.find("a").unwrap().fan_only);
    }

    #[test]
    fn apply_fan_only_locked_without_unlock() {
        // fan_only 且未解锁 → 粉丝门禁先于 rootfs 检查触发（InvalidArg），不碰引擎。
        let idx = RecipeIndex::parse(
            "[[recipe]]\nid=\"fb\"\nname=\"x\"\nscript=\"fb.sh\"\nfan_only = true\n",
        );
        let root = std::env::temp_dir().join("mox-proot-recipe-fan-gate");
        let _ = std::fs::remove_dir_all(&root);
        let err = idx.apply(&root, "fb", "ubuntu").unwrap_err();
        assert!(
            matches!(err, ProotError::InvalidArg(_)),
            "未解锁的 fan_only 配方应被门禁挡下：{:?}",
            err
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paid_scripts_embedded_not_plaintext_on_disk() {
        // 渗透测试路径③：付费脚本内嵌混淆，磁盘 pro-*.sh 是占位桩，
        // decode_paid 还原原文；直接 `sh` 磁盘文件无害。
        let s = crate::proot::recipe_paid::decode_paid("pro-gpu").expect("应有内嵌付费脚本");
        assert!(s.contains("WebGPU") || s.contains("GPU"), "解码后应含原脚本内容");
        assert!(!s.contains("占位桩"), "不应是占位桩");
        // 磁盘占位桩确实只回显提示、不执行真实逻辑
        let stub = std::fs::read_to_string(
            std::env::current_dir().unwrap().join("recipes/pro-gpu.sh"),
        )
        .or_else(|_| {
            std::fs::read_to_string(
                std::env::current_exe()
                    .unwrap()
                    .parent()
                    .unwrap()
                    .join("../share/mox-proot/recipes/pro-gpu.sh"),
            )
        });
        if let Ok(stub) = stub {
            assert!(stub.contains("占位桩"), "磁盘 pro-*.sh 应为无害占位桩");
        }
    }

    #[test]
    fn parse_requires_flag() {
        let on = RecipeIndex::parse(
            "[[recipe]]\nid=\"pb\"\nname=\"x\"\nscript=\"pb.sh\"\nrequires = \"proot-pro\"\n",
        );
        assert_eq!(on.find("pb").unwrap().requires, "proot-pro");
        // 未标注 → 默认空串（普通配方不受影响）。
        let off = RecipeIndex::parse("[[recipe]]\nid=\"a\"\nname=\"x\"\nscript=\"a.sh\"\n");
        assert!(off.find("a").unwrap().requires.is_empty());
    }

    #[test]
    fn apply_requires_locked_without_entitlement() {
        // requires 且无授权缓存 → 商品门禁先于 rootfs 检查触发（InvalidArg）。
        let idx = RecipeIndex::parse(
            "[[recipe]]\nid=\"pb\"\nname=\"x\"\nscript=\"pb.sh\"\nrequires = \"proot-pro\"\n",
        );
        let root =
            std::env::temp_dir().join(format!("mox-proot-recipe-req-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let err = idx.apply(&root, "pb", "ubuntu").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("proot-pro") && msg.contains("mox redeem"),
            "应提示商品授权与刷新命令：{}",
            msg
        );
        assert!(matches!(err, ProotError::InvalidArg(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn load_overlays_embedded_gating_even_if_disk_strips_requires() {
        // 渗透测试路径②：攻击者在 MOX_RECIPE_DIR 的 index.toml 里删掉
        // `requires = "proot-pro"`，load() 仍必须报告该付费配方处于门禁后。
        let dir = std::env::temp_dir().join(format!("mox-proot-recipe-strip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // 故意写一份"被删掉门禁行"的索引，但保留付费配方条目
        std::fs::write(
            dir.join("index.toml"),
            "[[recipe]]\nid=\"pro-gpu\"\nname=\"X\"\nscript=\"pro-gpu.sh\"\n\n\
             [[recipe]]\nid=\"community\"\nname=\"免费社区\"\nscript=\"community.sh\"\n",
        )
        .unwrap();
        std::env::set_var("MOX_RECIPE_DIR", &dir);
        let idx = RecipeIndex::load();
        std::env::remove_var("MOX_RECIPE_DIR");
        let paid = idx.find("pro-gpu").expect("付费配方应仍在列表");
        assert_eq!(
            paid.requires, "proot-pro",
            "即使磁盘索引删了 requires 行，内嵌门禁必须兜底生效"
        );
        let comm = idx.find("community").expect("社区配方应可见");
        assert!(comm.requires.is_empty(), "社区配方自身无门禁不受影响");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn apply_requires_passes_after_gate_when_entitled() {
        // 有新鲜授权缓存时，商品门禁放行 → 错误应来自 rootfs 未安装（DistroNotInstalled），
        // 证明门禁判定本身不拦。
        let idx = RecipeIndex::parse(
            "[[recipe]]\nid=\"pb\"\nname=\"x\"\nscript=\"pb.sh\"\nrequires = \"proot-pro\"\n",
        );
        let root =
            std::env::temp_dir().join(format!("mox-proot-recipe-req-ok-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // P0-1 后门禁 fail-closed：先用「设备身份 + 服务端签名」落合法缓存，
        // 否则 has_entitlement 一律拒绝，测试就走不到容器检查那一步。
        crate::proot::redeem::testkit::seed_legit_cache(
            &root,
            &[crate::proot::redeem::Entitlement {
                product_id: "proot-pro".into(),
                tier: "pro".into(),
                exp: 0,
            }],
            crate::proot::redeem::testkit::SIG_PROOT_PRO,
        );
        let err = idx.apply(&root, "pb", "ubuntu").unwrap_err();
        assert!(
            matches!(err, ProotError::DistroNotInstalled(_)),
            "有授权应过门禁、走到容器检查：{:?}",
            err
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- 渗透测试 P0-2 回归：id 命令注入 / 路径穿越 ----

    /// 复测报告的真机载荷：`id = "zz;echo RECIPE_ID_INJECTION_PROOF;#"`。
    /// 旧实现会把它拼进 `sh -c` 执行注入命令；现在 parse 必须整条丢弃。
    #[test]
    fn parse_drops_recipe_id_with_shell_metacharacters() {
        let evil = "[[recipe]]\n\
                    id = \"zz;echo RECIPE_ID_INJECTION_PROOF;#\"\n\
                    name = \"evil\"\n\
                    script = \"evil.sh\"\n";
        let idx = RecipeIndex::parse(evil);
        assert!(
            idx.all().is_empty(),
            "含 ; / # / 空格的 id 必须整条丢弃，不能进入索引"
        );
        assert!(idx.find("zz;echo RECIPE_ID_INJECTION_PROOF;#").is_err());
    }

    #[test]
    fn parse_drops_ids_that_escape_paths() {
        // 注意 `-rf` 是**合法** id（valid_id 允许 `-` 开头），不能当非法样本；
        // 真正要挡的是路径分隔符 / shell 元字符 / 空格 / 以 . 开头。
        for bad in ["../etc", "..", "a/b", "a\\b", "x y", "id`whoami`", "a;b", "a#b"] {
            let text = format!(
                "[[recipe]]\nid = \"{}\"\nname = \"x\"\nscript = \"x.sh\"\n",
                bad
            );
            assert!(
                RecipeIndex::parse(&text).all().is_empty(),
                "非法 id {:?} 不应被接纳",
                bad
            );
        }
        // 合法 id 仍正常保留（别把白名单写成一刀切）。
        assert_eq!(
            RecipeIndex::parse(SAMPLE).all().len(),
            2,
            "正常社区配方不应被误杀"
        );
    }

    /// `script = "../../etc/passwd"`：旧实现会 `dir.join(script)` 读任意文件。
    /// 现在非法脚本名置空，read_script 明确拒绝。
    #[test]
    fn traversing_script_name_is_rejected() {
        let dir = std::env::temp_dir().join("mox-proot-recipe-traversal");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.toml"),
            "[[recipe]]\nid = \"evil\"\nname = \"x\"\nscript = \"../../etc/passwd\"\n").unwrap();
        let idx = RecipeIndex::from_dir(&dir).unwrap();
        let r = idx.find("evil").expect("id 合法，配方本身应保留");
        assert!(r.script.is_empty(), "穿越型脚本名必须作废");
        assert!(
            idx.script_content("evil").is_err(),
            "作废的脚本名不得触发文件读取"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 即便绕过 parse 手工构造 Recipe，read_script 的白名单也要兜住。
    #[test]
    fn read_script_rejects_forged_recipe_with_bad_script() {
        let idx = RecipeIndex {
            recipes: vec![Recipe {
                id: "good".into(),
                script: "../secret".into(),
                ..Recipe::default()
            }],
            dir: None,
        };
        assert!(idx.script_content("good").is_err());
    }

    /// apply 的执行参数必须是 argv（`/bin/sh` + 脚本路径），不能出现
    /// `sh -c` 拼接出的命令串。这里通过源码级不变量间接钉死：注入 id
    /// 在 parse 阶段就被丢弃，apply 找不到配方直接报 DistroUnknown，
    /// 根本走不到引擎执行。
    #[test]
    fn injected_id_never_reaches_apply() {
        let idx = RecipeIndex::parse(
            "[[recipe]]\nid = \"zz;echo PWNED;#\"\nname = \"e\"\nscript = \"e.sh\"\n",
        );
        let root = std::env::temp_dir().join("mox-proot-recipe-inject");
        assert!(
            matches!(
                idx.apply(&root, "zz;echo PWNED;#", "ubuntu"),
                Err(ProotError::DistroUnknown(_))
            ),
            "注入 id 必须在索引层就被丢弃"
        );
    }
}
