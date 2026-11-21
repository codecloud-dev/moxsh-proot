//! 统一访问层：把两类权益融为单一门禁面，配方只调 [`recipe_unlocked`]。
//!
//! 未解锁的权益默认不在状态里暴露细节；[`sync`] 用于刷新，[`admin_revoke_local`] 用于本地清理。

use std::path::Path;

use crate::proot::fan;
use crate::proot::redeem;
use crate::proot::recipe::Recipe;
use crate::proot::ProotResult;

/// 访问状态（UI / 诊断用；粉丝专属默认隐藏）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessStatus {
    /// 灰度是否开启（决定粉丝专属是否可见/可唤起）。
    pub fan_enabled: bool,
    /// 粉丝档位（None = 未解锁；灰度关时恒为 None —— 粉丝专属默认隐藏）。
    pub fan_tier: Option<String>,
    /// 已持有商品授权 `(product_id, tier)`。
    pub entitlements: Vec<(String, String)>,
}

/// 配方是否解锁（融合单一裁决点）。
///
/// 规则：
/// - `fan_only` 配方：需 `fan::is_fan`（且灰度开启）；否则挡。
/// - `requires` 商品授权：需 `redeem::has_entitlement`；否则挡。
/// - 两者可并存（AND）。
pub fn recipe_unlocked(root: &Path, recipe: &Recipe) -> bool {
    if recipe.fan_only && !fan::is_fan(root) {
        return false;
    }
    if !recipe.requires.is_empty() && !redeem::has_entitlement(root, &recipe.requires) {
        return false;
    }
    true
}

/// 统一状态视图（粉丝专属默认隐藏）。
pub fn status(root: &Path) -> AccessStatus {
    let fan_enabled = fan::fan_enabled();
    let fan_tier = if fan_enabled {
        fan::load_state(root).map(|s| s.tier)
    } else {
        None
    };
    let mut entitlements = Vec::new();
    if let Some(cache) = redeem::load_ents(root) {
        for e in &cache.ents {
            entitlements.push((e.product_id.clone(), e.tier.clone()));
        }
    }
    AccessStatus { fan_enabled, fan_tier, entitlements }
}

/// 管理后台统一刷新：授权重拉（服务端吊销后本次即失活）+ 粉丝状态重读。
///
/// 粉丝已解锁态不主动清本地（避免误伤正常用户）；服务端吊销后由
/// [`admin_revoke_local`] 显式清，或下次 [`status`] 读到过期自然失活。
pub fn sync(root: &Path) -> ProotResult<AccessStatus> {
    // 授权：联网拉取并落盘（服务端吊销后本次即失活）。
    redeem::refresh_ents(root)?;
    Ok(status(root))
}

/// 管理端在设备侧清除本地访问状态（配合服务端吊销）。
pub fn admin_revoke_local(root: &Path) -> ProotResult<()> {
    let _ = std::fs::remove_file(fan::fan_file_path(root));
    let _ = std::fs::remove_file(redeem::ents_file_path(root));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recipe_unlocked_open_recipe_passes() {
        // 既无 fan_only 也无 requires 的配方，任何 root 都应解锁。
        let r = Recipe {
            id: "open".into(),
            name: "开放".into(),
            description: String::new(),
            tags: Vec::new(),
            script: String::new(),
            fan_only: false,
            requires: String::new(),
        };
        assert!(recipe_unlocked(Path::new("/nonexistent"), &r));
    }

    #[test]
    fn recipe_unlocked_fan_only_blocks_without_token() {
        let r = Recipe {
            id: "fb".into(),
            name: "粉丝".into(),
            description: String::new(),
            tags: Vec::new(),
            script: String::new(),
            fan_only: true,
            requires: String::new(),
        };
        // 灰度关 + 无令牌文件 → 挡下；不碰引擎。
        assert!(!recipe_unlocked(Path::new("/nonexistent"), &r));
    }
}
