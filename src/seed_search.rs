use serde::{Deserialize, Serialize};

use crate::{
    game::{ResourceDescriptor, World},
    randomization::{NodePuritySettings, NodeRandomizationMode},
    search_template::WorldSearchTemplate,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PickableNodeKind {
    ResourceNode,
    FrackingCore,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeConstraint {
    pub node_name: String,
    pub node_kind: PickableNodeKind,
    pub required_resource: ResourceDescriptor,
    pub location: [f32; 3],
}

#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SearchCacheKey {
    constraints: Vec<(String, PickableNodeKind, ResourceDescriptor)>,
    pub mode: NodeRandomizationMode,
    pub purity: NodePuritySettings,
}

impl SearchCacheKey {
    pub fn new(
        constraints: &[NodeConstraint],
        mode: NodeRandomizationMode,
        purity: NodePuritySettings,
    ) -> Self {
        let mut entries: Vec<_> = constraints
            .iter()
            .map(|c| {
                (
                    c.node_name.clone(),
                    c.node_kind,
                    c.required_resource,
                )
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));

        Self {
            constraints: entries,
            mode,
            purity,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.constraints.is_empty()
    }

    pub fn constraint_entries(&self) -> &[(String, PickableNodeKind, ResourceDescriptor)] {
        &self.constraints
    }
}

pub const TARGET_MATCH_COUNT: usize = 10;
const BATCH_SIZE: usize = 2_000;

pub struct SeedSearchState {
    cache_key: Option<SearchCacheKey>,
    pub matches: Vec<i32>,
    next_seed: i32,
    scan_origin: i32,
    seeds_scanned: u64,
    pub exhausted: bool,
    pub searching: bool,
    search_template: WorldSearchTemplate,
}

impl SeedSearchState {
    pub fn new(default_world: World) -> Self {
        let search_template = WorldSearchTemplate::from_world(&default_world);
        Self {
            cache_key: None,
            matches: Vec::new(),
            next_seed: 0,
            scan_origin: 0,
            seeds_scanned: 0,
            exhausted: false,
            searching: false,
            search_template,
        }
    }

    pub fn clear(&mut self) {
        self.cache_key = None;
        self.matches.clear();
        self.next_seed = 0;
        self.scan_origin = 0;
        self.seeds_scanned = 0;
        self.exhausted = false;
        self.searching = false;
    }

    pub fn on_settings_changed(
        &mut self,
        key: SearchCacheKey,
        prior_matches: &[i32],
    ) {
        if key.is_empty() || key.mode == NodeRandomizationMode::None {
            self.clear();
            return;
        }

        let mode_or_purity_changed = self
            .cache_key
            .as_ref()
            .is_some_and(|old| old.mode != key.mode || old.purity != key.purity);

        if mode_or_purity_changed {
            self.matches.clear();
            self.next_seed = 0;
            self.seeds_scanned = 0;
            self.exhausted = false;
        } else {
            self.matches = prior_matches
                .iter()
                .copied()
                .filter(|&seed| seed_matches(&self.search_template, seed, &key))
                .collect();
        }

        self.cache_key = Some(key);
        self.scan_origin = self.next_seed;
        self.exhausted = false;
        self.searching = self.matches.len() < TARGET_MATCH_COUNT;
    }

    pub fn step(&mut self) {
        let Some(key) = self.cache_key.clone() else {
            self.searching = false;
            return;
        };

        if !self.searching || self.exhausted || self.matches.len() >= TARGET_MATCH_COUNT {
            self.searching = false;
            return;
        }

        let mut evaluated_this_step = 0u64;

        for _ in 0..BATCH_SIZE {
            if self.matches.len() >= TARGET_MATCH_COUNT {
                break;
            }

            let seed = self.next_seed;

            if evaluated_this_step > 0 && seed == self.scan_origin {
                self.exhausted = true;
                break;
            }

            if seed_matches(&self.search_template, seed, &key) && !self.matches.contains(&seed) {
                self.matches.push(seed);
            }

            self.next_seed = seed.wrapping_add(1);
            self.seeds_scanned += 1;
            evaluated_this_step += 1;
        }

        if self.matches.len() >= TARGET_MATCH_COUNT || self.exhausted {
            self.searching = false;
        }
    }

    pub fn status_text(&self) -> String {
        if self.cache_key.as_ref().is_none_or(|k| k.is_empty()) {
            return "Add node constraints to search for seeds.".to_owned();
        }

        if self.cache_key.as_ref().is_some_and(|k| k.mode == NodeRandomizationMode::None) {
            return "Select a randomization mode to search by node placement.".to_owned();
        }

        if self.searching {
            return format!(
                "{}/{} found — searching from seed {}…",
                self.matches.len(),
                TARGET_MATCH_COUNT,
                self.next_seed
            );
        }

        if self.exhausted && self.matches.is_empty() {
            return "No matching seeds found.".to_owned();
        }

        if self.exhausted {
            return format!(
                "{}/{} found — no more matches exist.",
                self.matches.len(),
                TARGET_MATCH_COUNT
            );
        }

        format!("{}/{} found.", self.matches.len(), TARGET_MATCH_COUNT)
    }
}

pub fn seed_matches(template: &WorldSearchTemplate, seed: i32, key: &SearchCacheKey) -> bool {
    template.seed_matches(seed, key)
}
