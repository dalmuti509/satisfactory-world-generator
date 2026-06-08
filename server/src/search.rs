use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use rayon::prelude::*;
use satisfactory_world_generator::{
    randomization::NodeRandomizationMode,
    search_template::WorldSearchTemplate,
    seed_search::{SearchCacheKey, TARGET_MATCH_COUNT, seed_matches},
};

pub const PARALLEL_BATCH_SIZE: i32 = 4_000;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchRequest {
    pub constraints: Vec<SearchConstraintEntry>,
    pub mode: satisfactory_world_generator::randomization::NodeRandomizationMode,
    pub purity: satisfactory_world_generator::randomization::NodePuritySettings,
    pub prior_matches: Vec<i32>,
    pub next_seed: i32,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchConstraintEntry {
    pub node_name: String,
    pub node_kind: satisfactory_world_generator::seed_search::PickableNodeKind,
    pub required_resource: satisfactory_world_generator::game::ResourceDescriptor,
}

impl SearchRequest {
    pub fn cache_key(&self) -> SearchCacheKey {
        use satisfactory_world_generator::seed_search::{NodeConstraint, SearchCacheKey};

        let constraints: Vec<NodeConstraint> = self
            .constraints
            .iter()
            .map(|c| NodeConstraint {
                node_name: c.node_name.clone(),
                node_kind: c.node_kind,
                required_resource: c.required_resource,
                location: [0.0; 3],
            })
            .collect();
        SearchCacheKey::new(&constraints, self.mode, self.purity)
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SearchEvent {
    Match { seed: i32 },
    Progress {
        matches: usize,
        next_seed: i32,
        searching: bool,
    },
    Done {
        matches: Vec<i32>,
        next_seed: i32,
        exhausted: bool,
    },
}

pub fn run_parallel_search(
    search_template: &WorldSearchTemplate,
    request: &SearchRequest,
    cancel: Arc<AtomicBool>,
    mut on_event: impl FnMut(SearchEvent) + Send,
) {
    let key = request.cache_key();

    if key.is_empty() || key.mode == NodeRandomizationMode::None {
        on_event(SearchEvent::Done {
            matches: Vec::new(),
            next_seed: 0,
            exhausted: true,
        });
        return;
    }

    let mut matches: Vec<i32> = request
        .prior_matches
        .iter()
        .copied()
        .filter(|&seed| seed_matches(search_template, seed, &key))
        .collect();

    for &seed in &matches {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        on_event(SearchEvent::Match { seed });
    }

    if matches.len() >= TARGET_MATCH_COUNT {
        on_event(SearchEvent::Done {
            matches: matches.clone(),
            next_seed: request.next_seed,
            exhausted: false,
        });
        return;
    }

    let mut next_seed = request.next_seed;
    let scan_origin = next_seed;
    let mut wrapped = false;

    while matches.len() < TARGET_MATCH_COUNT {
        if cancel.load(Ordering::Relaxed) {
            return;
        }

        let seeds: Vec<i32> = (0..PARALLEL_BATCH_SIZE)
            .map(|offset| next_seed.wrapping_add(offset))
            .collect();

        let found: Vec<i32> = seeds
            .par_iter()
            .filter_map(|&seed| {
                if seed_matches(search_template, seed, &key) {
                    Some(seed)
                } else {
                    None
                }
            })
            .collect();

        for seed in found {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            if matches.len() >= TARGET_MATCH_COUNT {
                break;
            }
            if !matches.contains(&seed) {
                matches.push(seed);
                on_event(SearchEvent::Match { seed });
            }
        }

        next_seed = next_seed.wrapping_add(PARALLEL_BATCH_SIZE);

        on_event(SearchEvent::Progress {
            matches: matches.len(),
            next_seed,
            searching: matches.len() < TARGET_MATCH_COUNT,
        });

        if next_seed == scan_origin {
            wrapped = true;
            break;
        }
    }

    on_event(SearchEvent::Done {
        exhausted: wrapped && matches.len() < TARGET_MATCH_COUNT,
        matches,
        next_seed,
    });
}
