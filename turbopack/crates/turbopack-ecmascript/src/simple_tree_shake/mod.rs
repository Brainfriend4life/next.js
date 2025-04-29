//! Intermediate tree shaking that uses global information but not good as the full tree shaking.

use anyhow::{bail, Context, Result};
use rustc_hash::{FxHashMap, FxHashSet};
use turbo_rcstr::RcStr;
use turbo_tasks::{ResolvedVc, ValueToString, Vc};
use turbopack_core::{module::Module, module_graph::ModuleGraph, resolve::ExportUsage};

use crate::chunk::EcmascriptChunkPlaceable;

#[turbo_tasks::function]
pub async fn get_module_export_usages(
    graph: ResolvedVc<ModuleGraph>,
    module: ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>,
) -> Result<Vc<ModuleExportUsageInfo>> {
    let export_usage_info = compute_export_usage_info(graph)
        .resolve_strongly_consistent()
        .await?;

    let export_usage_info = export_usage_info.await?;

    let Some(exports) = export_usage_info.used_exports.get(&module) else {
        bail!(
            "module {} not found in export usage info. Something is wrong with the export usage \
             info.",
            module.ident().to_string().await?
        );
    };

    Ok(ModuleExportUsageInfo {
        exports: exports.clone(),
    }
    .cell())
}

#[turbo_tasks::function(operation)]
async fn compute_export_usage_info(graph: ResolvedVc<ModuleGraph>) -> Result<Vc<ExportUsageInfo>> {
    let mut result = ExportUsageInfo::default();

    graph
        .await?
        .traverse_all_edges_unordered(|(_, ref_data), target| {
            if let Some(target_module) =
                ResolvedVc::try_downcast::<Box<dyn EcmascriptChunkPlaceable>>(target.module)
            {
                result
                    .used_exports
                    .entry(target_module)
                    .or_default()
                    .insert(ref_data.export.clone());
            }

            Ok(())
        })
        .await
        .context("failed to traverse module graph")?;

    Ok(result.cell())
}

#[turbo_tasks::value]
#[derive(Default)]
pub struct ExportUsageInfo {
    used_exports: FxHashMap<ResolvedVc<Box<dyn EcmascriptChunkPlaceable>>, FxHashSet<ExportUsage>>,
}

#[turbo_tasks::value]
pub struct ModuleExportUsageInfo {
    exports: FxHashSet<ExportUsage>,
}

impl ModuleExportUsageInfo {
    pub fn is_export_used(&self, export_name: RcStr) -> bool {
        self.exports.contains(&ExportUsage::All)
            || self.exports.contains(&ExportUsage::Named(export_name))
    }
}
