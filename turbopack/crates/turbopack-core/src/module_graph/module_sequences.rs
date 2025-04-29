use anyhow::Result;
use rustc_hash::{FxHashMap, FxHashSet};
use turbo_tasks::{FxIndexMap, ResolvedVc, TryJoinIterExt, ValueToString, Vc};

use crate::{
    module::{Module, Modules},
    module_graph::{GraphTraversalAction, ModuleGraph},
};

// TODO maybe a smallvec once we know the size distribution?
#[turbo_tasks::value]
pub struct ModuleSequences {
    groups: FxHashMap<ResolvedVc<Box<dyn Module>>, ResolvedVc<Modules>>,
    included: FxHashSet<ResolvedVc<Box<dyn Module>>>,
}

#[turbo_tasks::value_impl]
impl ModuleSequences {
    #[turbo_tasks::function]
    pub fn additional_modules_for_entry(&self, module: ResolvedVc<Box<dyn Module>>) -> Vc<Modules> {
        self.groups
            .get(&module)
            .map(|v| **v)
            .unwrap_or_else(Modules::empty)
    }

    #[turbo_tasks::function]
    pub fn should_create_chunk_item_for(&self, module: ResolvedVc<Box<dyn Module>>) -> Vc<bool> {
        Vc::cell(!self.included.contains(&module))
    }
}

pub async fn compute_module_sequences(
    module_graph: Vc<ModuleGraph>,
) -> Result<Vc<ModuleSequences>> {
    let chunk_groups = module_graph.chunk_group_info().await?;
    let module_graph = module_graph.await?;

    let mut groups: FxIndexMap<ResolvedVc<Box<dyn Module>>, Vec<ResolvedVc<Box<dyn Module>>>> =
        Default::default();
    let mut included: FxHashSet<ResolvedVc<Box<dyn Module>>> = FxHashSet::default();

    // let parents = FxHashMap::default();

    for chunk_group in &chunk_groups.chunk_groups {
        // struct State {
        //     first: Option<ResolvedVc<Box<dyn Module>>>,
        // }
        // let mut state = State { first: None };
        let entries = FxHashSet::from_iter(chunk_group.entries());
        let mut first = None;
        module_graph
            .traverse_edges_from_entries_topological(
                chunk_group.entries(),
                &mut (),
                |parent_info, _node, _| {
                    if parent_info.is_none_or(|p| p.1.is_parallel()) {
                        Ok(GraphTraversalAction::Continue)
                    } else {
                        Ok(GraphTraversalAction::Exclude)
                    }
                },
                |_parent_info, node, _| {
                    let module = node.module;
                    let is_exposed = entries.contains(&module);
                    if is_exposed {
                        groups.entry(module).or_default();
                    } else if let Some(first) = first {
                        if module != first {
                            included.insert(module);
                            groups.entry(first).or_default().push(module);
                        }
                    } else {
                        first = Some(module);
                        groups.entry(module).or_default();
                    }
                },
            )
            .await?;
    }

    println!(
        "included {:#?}",
        included
            .iter()
            .map(|m| m.ident().to_string())
            .try_join()
            .await?
    );
    println!(
        "groups {:#?}",
        groups
            .iter()
            .map(async |(k, v)| Ok((
                k.ident().to_string().await?,
                v.iter().map(|m| m.ident().to_string()).try_join().await?
            )))
            .try_join()
            .await?
    );

    Ok(ModuleSequences {
        groups: groups
            .into_iter()
            .map(|(k, v)| (k, ResolvedVc::cell(v)))
            .collect(),
        included,
    }
    .cell())
}
