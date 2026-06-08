use std::cell::RefCell;

use egui::Context;
use wasm_bindgen::JsValue;
use wasm_bindgen::{JsCast, prelude::Closure};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{
    Headers, ReadableStreamDefaultReader, Request, RequestInit, RequestMode, Response,
};

use crate::{
    randomization::{NodePuritySettings, NodeRandomizationMode},
    search_template::WorldSearchTemplate,
    seed_search::{NodeConstraint, SearchCacheKey, TARGET_MATCH_COUNT, seed_matches},
};

#[derive(Clone, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SearchEvent {
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

#[derive(serde::Serialize)]
struct SearchConstraintEntry {
    node_name: String,
    node_kind: crate::seed_search::PickableNodeKind,
    required_resource: crate::game::ResourceDescriptor,
}

#[derive(serde::Serialize)]
struct SearchRequest {
    constraints: Vec<SearchConstraintEntry>,
    mode: NodeRandomizationMode,
    purity: NodePuritySettings,
    prior_matches: Vec<i32>,
    next_seed: i32,
}

thread_local! {
    static INBOX: RefCell<Vec<(u64, SearchEvent)>> = RefCell::new(Vec::new());
}

pub struct RemoteSeedSearch {
    pub matches: Vec<i32>,
    next_seed: i32,
    pub exhausted: bool,
    pub searching: bool,
    cache_key: Option<SearchCacheKey>,
    active_generation: u64,
    search_template: WorldSearchTemplate,
}

impl RemoteSeedSearch {
    pub fn new() -> Self {
        Self {
            matches: Vec::new(),
            next_seed: 0,
            exhausted: false,
            searching: false,
            cache_key: None,
            active_generation: 0,
            search_template: WorldSearchTemplate::from_world(
                &serde_json::from_str(include_str!("../default-world.json")).unwrap(),
            ),
        }
    }

    pub fn clear(&mut self) {
        self.cache_key = None;
        self.matches.clear();
        self.next_seed = 0;
        self.exhausted = false;
        self.searching = false;
        self.active_generation = self.active_generation.wrapping_add(1);
        INBOX.with(|inbox| inbox.borrow_mut().clear());
    }

    pub fn on_settings_changed(
        &mut self,
        key: SearchCacheKey,
        prior_matches: &[i32],
        constraints: &[NodeConstraint],
        ctx: &Context,
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
            self.exhausted = false;
        } else {
            self.matches = prior_matches
                .iter()
                .copied()
                .filter(|&seed| seed_matches(&self.search_template, seed, &key))
                .collect();
        }

        self.cache_key = Some(key);
        self.searching = self.matches.len() < TARGET_MATCH_COUNT;
        self.exhausted = false;
        self.start_request(constraints, ctx);
    }

    pub fn poll(&mut self, ctx: &Context) {
        let events: Vec<(u64, SearchEvent)> =
            INBOX.with(|inbox| inbox.borrow_mut().drain(..).collect());

        for (generation, event) in events {
            if generation != self.active_generation {
                continue;
            }

            match event {
                SearchEvent::Match { seed } => {
                    if !self.matches.contains(&seed) {
                        self.matches.push(seed);
                    }
                    ctx.request_repaint();
                }
                SearchEvent::Progress {
                    matches,
                    next_seed,
                    searching,
                } => {
                    self.next_seed = next_seed;
                    self.searching = searching && self.matches.len() < TARGET_MATCH_COUNT;
                    if self.matches.len() < matches {
                        ctx.request_repaint();
                    }
                }
                SearchEvent::Done {
                    matches,
                    next_seed,
                    exhausted,
                } => {
                    self.matches = matches;
                    self.next_seed = next_seed;
                    self.exhausted = exhausted;
                    self.searching = false;
                    ctx.request_repaint();
                }
            }
        }
    }

    pub fn status_text(&self) -> String {
        if self.cache_key.as_ref().is_none_or(|k| k.is_empty()) {
            return "Add node constraints to search for seeds.".to_owned();
        }

        if self
            .cache_key
            .as_ref()
            .is_some_and(|k| k.mode == NodeRandomizationMode::None)
        {
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

    fn start_request(&mut self, constraints: &[NodeConstraint], ctx: &Context) {
        self.active_generation = self.active_generation.wrapping_add(1);
        let generation = self.active_generation;

        let Some(key) = self.cache_key.clone() else {
            return;
        };

        let request_body = SearchRequest {
            constraints: constraints
                .iter()
                .map(|c| SearchConstraintEntry {
                    node_name: c.node_name.clone(),
                    node_kind: c.node_kind,
                    required_resource: c.required_resource,
                })
                .collect(),
            mode: key.mode,
            purity: key.purity,
            prior_matches: self.matches.clone(),
            next_seed: self.next_seed,
        };

        let ctx = ctx.clone();

        spawn_local(async move {
            if let Err(err) = run_search_stream(request_body, generation).await {
                log::warn!("seed search request failed: {err:?}");
                INBOX.with(|inbox| {
                    inbox.borrow_mut().push((
                        generation,
                        SearchEvent::Done {
                            matches: Vec::new(),
                            next_seed: 0,
                            exhausted: true,
                        },
                    ));
                });
                ctx.request_repaint();
            }
        });
    }
}

async fn run_search_stream(request: SearchRequest, generation: u64) -> Result<(), JsValue> {
    let body = serde_json::to_string(&request).map_err(|err| JsValue::from_str(&err.to_string()))?;

    let headers = Headers::new()?;
    headers.set("Content-Type", "application/json")?;

    let opts = RequestInit::new();
    opts.set_method("POST");
    opts.set_mode(RequestMode::Cors);
    opts.set_body(&JsValue::from_str(&body));
    opts.set_headers(&headers);

    let url = js_sys::Reflect::get(&web_sys::window().unwrap(), &JsValue::from_str("location"))
        .ok()
        .and_then(|loc| loc.dyn_into::<web_sys::Location>().ok())
        .map(|loc| format!("{}/api/search", loc.origin().unwrap_or_default()))
        .unwrap_or_else(|| "/api/search".to_owned());

    let request = Request::new_with_str_and_init(&url, &opts)?;
    let window = web_sys::window().unwrap();
    let resp_value = JsFuture::from(window.fetch_with_request(&request)).await?;
    let resp: Response = resp_value.dyn_into()?;

    if !resp.ok() {
        return Err(JsValue::from_str(&format!("HTTP {}", resp.status())));
    }

    let reader: ReadableStreamDefaultReader = resp
        .body()
        .ok_or_else(|| JsValue::from_str("missing response body"))?
        .get_reader()
        .dyn_into()?;

    let mut buffer = String::new();

    loop {
        let read_promise = reader.read();
        let chunk = JsFuture::from(read_promise).await?;
        let done = js_sys::Reflect::get(&chunk, &JsValue::from_str("done"))?
            .as_bool()
            .unwrap_or(true);

        if done {
            break;
        }

        let value = js_sys::Reflect::get(&chunk, &JsValue::from_str("value"))?;
        if value.is_null() || value.is_undefined() {
            continue;
        }

        let uint8_array = js_sys::Uint8Array::new(&value);
        let mut bytes = vec![0u8; uint8_array.length() as usize];
        uint8_array.copy_to(&mut bytes);
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(split_at) = buffer.find("\n\n") {
            let frame = buffer.drain(..split_at + 2).collect::<String>();
            if let Some(event) = parse_sse_frame(&frame) {
                INBOX.with(|inbox| inbox.borrow_mut().push((generation, event)));
            }
        }
    }

    let _ = generation;
    Ok(())
}

fn parse_sse_frame(frame: &str) -> Option<SearchEvent> {
    for line in frame.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            return serde_json::from_str(data).ok();
        }
        if let Some(data) = line.strip_prefix("data:") {
            return serde_json::from_str(data.trim()).ok();
        }
    }
    None
}

#[allow(dead_code)]
fn _noop(_: Closure<dyn FnMut()>) {}
