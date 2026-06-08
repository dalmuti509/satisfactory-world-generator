use std::collections::HashMap;

use crate::{
    game::{ResourceDescriptor, World},
    random_stream::RandomStream,
    randomization::{
        NodeRandomizationMode, ResourceNodeInfo, get_purity_override, modify_node_distribution,
        shuffle,
    },
    seed_search::{PickableNodeKind, SearchCacheKey},
};

#[derive(Clone, Debug)]
pub struct WorldSearchTemplate {
    resource_nodes: Vec<(String, ResourceNodeInfo)>,
    fracking_cores: Vec<(String, ResourceNodeInfo)>,
    default_resources: HashMap<String, ResourceDescriptor>,
    default_fracking_resources: HashMap<String, ResourceDescriptor>,
}

#[derive(Clone, Debug)]
struct SearchPlan {
    resource_checks: HashMap<String, ResourceDescriptor>,
    fracking_checks: HashMap<String, ResourceDescriptor>,
    last_resource_index: Option<usize>,
    needs_full_resource_loop: bool,
}

impl WorldSearchTemplate {
    pub fn from_world(world: &World) -> Self {
        let mut resource_nodes = world
            .resource_nodes
            .iter()
            .map(|node| (node.name.clone(), ResourceNodeInfo::from(node)))
            .collect::<Vec<_>>();
        resource_nodes.sort_by(|a, b| a.0.cmp(&b.0));

        let mut fracking_cores = world
            .fracking_cores
            .iter()
            .map(|core| (core.name.clone(), ResourceNodeInfo::from(core)))
            .collect::<Vec<_>>();
        fracking_cores.sort_by(|a, b| a.0.cmp(&b.0));

        let default_resources = world
            .resource_nodes
            .iter()
            .map(|node| (node.name.clone(), node.resource))
            .collect();
        let default_fracking_resources = world
            .fracking_cores
            .iter()
            .map(|core| (core.name.clone(), core.resource))
            .collect();

        Self {
            resource_nodes,
            fracking_cores,
            default_resources,
            default_fracking_resources,
        }
    }

    pub fn seed_matches(&self, seed: i32, key: &SearchCacheKey) -> bool {
        if key.mode == NodeRandomizationMode::None {
            return key.constraint_entries().iter().all(|(name, kind, required)| {
                let actual = match kind {
                    PickableNodeKind::ResourceNode => self.default_resources.get(name),
                    PickableNodeKind::FrackingCore => self.default_fracking_resources.get(name),
                };
                actual == Some(required)
            });
        }

        let plan = self.plan_for(key);
        let mut rng = RandomStream::new(seed);

        let mut node_pool = self
            .resource_nodes
            .iter()
            .map(|(_, info)| info.clone())
            .collect::<Vec<_>>();
        node_pool.sort();

        match key.mode {
            NodeRandomizationMode::BasicRich => {
                modify_node_distribution(
                    &mut rng,
                    &mut node_pool,
                    crate::game::GameplayTag::Basic,
                    1.1,
                );
            }
            NodeRandomizationMode::AdvancedRich => {
                modify_node_distribution(
                    &mut rng,
                    &mut node_pool,
                    crate::game::GameplayTag::Advanced,
                    3.0,
                );
            }
            NodeRandomizationMode::FossilFuelRich => {
                modify_node_distribution(
                    &mut rng,
                    &mut node_pool,
                    crate::game::GameplayTag::FossilFuel,
                    2.0,
                );
            }
            NodeRandomizationMode::None | NodeRandomizationMode::Strict => (),
        }

        let resource_limit = if plan.needs_full_resource_loop {
            self.resource_nodes.len()
        } else {
            plan.last_resource_index.map_or(0, |index| index + 1)
        };

        for (name, _) in self.resource_nodes.iter().take(resource_limit) {
            let pool_index = rng.frand_range(0.0..node_pool.len() as f32) as usize;
            let node_info = node_pool.remove(pool_index);
            let resource = node_info.resource;

            let _ = get_purity_override(&mut rng, node_info.purity, key.purity);

            if let Some(required) = plan.resource_checks.get(name) {
                if resource != *required {
                    return false;
                }
            }
        }

        if plan.fracking_checks.is_empty() {
            return true;
        }

        let mut fracking_pool = self
            .fracking_cores
            .iter()
            .map(|(_, info)| info.clone())
            .collect::<Vec<_>>();
        fracking_pool.sort();
        shuffle(&mut rng, &mut fracking_pool);

        for (name, _) in &self.fracking_cores {
            let pool_index = rng.frand_range(0.0..fracking_pool.len() as f32) as usize;
            let node_info = fracking_pool.remove(pool_index);
            let resource = node_info.resource;

            if let Some(required) = plan.fracking_checks.get(name) {
                if resource != *required {
                    return false;
                }
            }
        }

        true
    }

    fn plan_for(&self, key: &SearchCacheKey) -> SearchPlan {
        let mut resource_checks = HashMap::new();
        let mut fracking_checks = HashMap::new();
        let mut last_resource_index = None;

        for (name, kind, required) in key.constraint_entries() {
            match kind {
                PickableNodeKind::ResourceNode => {
                    resource_checks.insert(name.clone(), *required);
                    if let Some(index) = self
                        .resource_nodes
                        .iter()
                        .position(|(node_name, _)| node_name == name)
                    {
                        last_resource_index = Some(
                            last_resource_index.map_or(index, |current: usize| current.max(index)),
                        );
                    }
                }
                PickableNodeKind::FrackingCore => {
                    fracking_checks.insert(name.clone(), *required);
                }
            }
        }

        SearchPlan {
            needs_full_resource_loop: !fracking_checks.is_empty(),
            last_resource_index,
            resource_checks,
            fracking_checks,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        randomization::{NodePuritySettings, NodeRandomizationMode, apply_randomization_settings},
        seed_search::{NodeConstraint, SearchCacheKey},
    };

    fn test_world() -> World {
        serde_json::from_str(include_str!("default-world.json")).unwrap()
    }

    fn legacy_seed_matches(default_world: &World, seed: i32, key: &SearchCacheKey) -> bool {
        let mut world = default_world.clone();
        apply_randomization_settings(&mut world, seed, key.mode, key.purity);

        key.constraint_entries().iter().all(|(name, kind, required)| {
            let actual = match kind {
                PickableNodeKind::ResourceNode => world
                    .resource_nodes
                    .iter()
                    .find(|node| node.name == *name)
                    .map(|node| node.resource),
                PickableNodeKind::FrackingCore => world
                    .fracking_cores
                    .iter()
                    .find(|core| core.name == *name)
                    .map(|core| core.resource),
            };
            actual == Some(*required)
        })
    }

    #[test]
    fn fast_path_matches_legacy_for_sample_seeds() {
        let world = test_world();
        let template = WorldSearchTemplate::from_world(&world);

        let constraints = vec![
            NodeConstraint {
                node_name: "BP_ResourceNode620".to_owned(),
                node_kind: PickableNodeKind::ResourceNode,
                required_resource: ResourceDescriptor::Coal,
                location: [0.0; 3],
            },
            NodeConstraint {
                node_name: "BP_FrackingCore5".to_owned(),
                node_kind: PickableNodeKind::FrackingCore,
                required_resource: ResourceDescriptor::NitrogenGas,
                location: [0.0; 3],
            },
        ];

        for mode in [
            NodeRandomizationMode::Strict,
            NodeRandomizationMode::BasicRich,
            NodeRandomizationMode::AdvancedRich,
            NodeRandomizationMode::FossilFuelRich,
        ] {
            for purity in [
                NodePuritySettings::NoChange,
                NodePuritySettings::AllNormal,
                NodePuritySettings::AllRandom,
            ] {
                let key = SearchCacheKey::new(&constraints, mode, purity);
                for seed in [0, 1, 42, 999, 123_456, -1, i32::MAX] {
                    let fast = template.seed_matches(seed, &key);
                    let legacy = legacy_seed_matches(&world, seed, &key);
                    assert_eq!(
                        fast, legacy,
                        "mismatch for seed {seed} mode {mode:?} purity {purity:?}"
                    );
                }
            }
        }
    }
}
