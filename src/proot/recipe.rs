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
                    recipes.push(r);
                }
                cur = Some(Recipe::default());
                continue;
            }
            if let Some((k, v)) = line.split_once('=') {
                if let Some(r) = cur.as_mut() {
                    let key = k.trim();
                    let val = unquote(v.trim()).to_string();
                    match key {
                        "id" => r.id = val,
                        "name" => r.name = val,
                        "description" => r.description = val,
                        "script" => r.script = val,
                        "tags" => {
                            r.tags = val
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect()
                        }
                        "fan_only" => r.fan_only = val == "true",
                        _ => {}
                    }
                }
            }
        }
        if let Some(r) = cur.take() {
            recipes.push(r);
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
    pub fn load() -> RecipeIndex {
        if let Ok(d) = std::env::var("MOXSH_RECIPE_DIR") {
            if let Ok(idx) = RecipeIndex::from_dir(Path::new(&d)) {
                return idx;
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(bin) = exe.parent() {
                // <bin>/../share/moxsh-proot/recipes（安装布局）
                let candidate = bin.join("../share/moxsh-proot/recipes");
                if let Ok(idx) = RecipeIndex::from_dir(&candidate) {
                    return idx;
                }
            }
        }
        RecipeIndex::parse(EMBEDDED_INDEX)
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
        if let Some(dir) = &self.dir {
            let p = dir.join(&recipe.script);
            if p.exists() {
                return std::fs::read_to_string(&p).map_err(ProotError::Io);
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(bin) = exe.parent() {
                let p = bin
                    .join("../share/moxsh-proot/recipes")
                    .join(&recipe.script);
                if p.exists() {
                    return std::fs::read_to_string(&p).map_err(ProotError::Io);
                }
            }
        }
        Err(ProotError::RootfsMissing(format!("配方脚本 {}", recipe.script)))
    }

    /// 应用配方：把公共库 + 脚本落进目标容器的 /tmp，再在容器内执行。
    ///
    /// - `root`：moxsh 数据根（`MOXSH_ROOT` 解析后）。
    /// - `recipe_id`：配方 id。
    /// - `distro_id`：已安装发行版 id（未装报错）。
    ///
    /// 落盘位置 `rootfs/tmp/*`：宿主视角写 = 容器内 `/tmp/*` 视角读，
    /// 执行后由引擎 env 决定的 HOME 无关，幂等可重跑。
    pub fn apply(&self, root: &Path, recipe_id: &str, distro_id: &str) -> ProotResult<i32> {
        let recipe = self.find(recipe_id)?;
        // 粉丝专属门禁（fan_only）：未解锁直接拒，不碰引擎、不落盘。
        if recipe.fan_only && !crate::proot::fan::is_fan(root) {
            return Err(ProotError::InvalidArg(format!(
                "配方 {} 为粉丝专属，需先解锁：mox fan login 领兑换码 → mox fan redeem <code>",
                recipe_id
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
        // 公共库先落，配方脚本首行 `source /tmp/moxsh-lib.sh` 即生效。
        std::fs::write(tmp.join("moxsh-lib.sh"), EMBEDDED_LIB).map_err(ProotError::Io)?;
        let script_name = format!("moxsh-recipe-{}.sh", recipe.id);
        std::fs::write(tmp.join(&script_name), &script).map_err(ProotError::Io)?;
        // 经 exec_into 在容器内执行（复用引擎 PATH/环境/控制台绑定）。
        let tail = vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            format!(
                "sh /tmp/{} && echo 'moxsh: 配方 {} 完成' || echo 'moxsh: 配方 {} 失败'",
                script_name, recipe.id, recipe.id
            ),
        ];
        cli::exec_into(root, distro_id, &tail)
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
        let dir = std::env::temp_dir().join("moxsh-recipe-test");
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
        let dir = std::env::temp_dir().join("moxsh-recipe-apply-test");
        let _ = std::fs::remove_dir_all(&dir);
        let idx = RecipeIndex::parse(SAMPLE);
        let root = dir.join("moxsh-root");
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
        let root = std::env::temp_dir().join("moxsh-recipe-fan-gate");
        let _ = std::fs::remove_dir_all(&root);
        let err = idx.apply(&root, "fb", "ubuntu").unwrap_err();
        assert!(
            matches!(err, ProotError::InvalidArg(_)),
            "未解锁的 fan_only 配方应被门禁挡下：{:?}",
            err
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
