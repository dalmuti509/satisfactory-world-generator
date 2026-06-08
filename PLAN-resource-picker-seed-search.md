# Plan: Resource Picker + Seed Search

## Goal

Add a **resource picker** on the map (replacing the current plot legend) that lets users define per-node resource constraints, then **search for seeds** where those fixed world positions are assigned those resources. Matching seeds are listed in the currently empty sidebar area above the View Options / Statistics tabs.

The **sidebar View Options checkbox table** stays unchanged — it continues to control which resource types and purities are visible on the map.

---

## Current State (baseline)

| Area | Behavior today |
|------|----------------|
| Map legend (top-right overlay) | Toggles entire resource types on/off via `egui_plot::Legend`; synced with `ViewOptions` |
| Sidebar View Options tab | Per-resource / per-purity checkbox table; unchanged by this feature |
| Sidebar empty space | Caused by `ui.take_available_space()` in `stats_panel` — intentional spacer above tabs |
| Seeds | Manual entry, random button, or URL share only; no search |
| Node identity | Each node has a stable `name` string in `default-world.json` (e.g. sorted by name during randomization) |
| Randomization | `apply_randomization_settings(world, seed, mode, purity)` — deterministic per seed |

**Key files:** `src/app/ui.rs`, `src/app/view_options.rs`, `src/app/plot_item.rs`, `src/randomization.rs`, `src/game.rs`

---

## User Workflow (proposed)

```mermaid
flowchart TD
    M{Mode is none?}
    M -->|Yes| N[Picker inactive — node selection disabled]
    M -->|No| A[Select resource type in map-zone picker]
    A --> B[Click node(s) on map]
    B --> C[Constraint added: node X must be resource Y]
    C --> D[Auto-search triggered]
    D --> E[Filter cached matches against new constraints]
    E --> F{Have 10 matches?}
    F -->|No| G[Resume forward scan from next_seed cursor]
    G --> H[Add matches until 10 or space exhausted]
    H --> F
    F -->|Yes| I[Show up to 10 seeds in sidebar list]
    I --> J[Click a seed → load in Randomization Settings]
```

1. User sets **Mode** to any value **other than `none`** (required before node picking works).
2. User picks a **resource type** from the map-zone picker (Iron, Uranium, etc.).
3. User **clicks one or more nodes** on the map to add constraints: *"this node (by stable `name`) must be that resource."* Each node keeps its own assigned resource type independently.
4. Constraints **persist** until changed or deselected: switch resource type + re-click a node to update it; click again (toggle) or use remove in the constraint list to deselect.
5. Selected constraints are shown in the picker UI (per-node, with assigned resource type).
6. Search runs **automatically** when constraints (or mode/purity) change.
7. Up to **10 matching seeds** appear in the sidebar list; clicking a result sets the seed field and regenerates the map.

---

## UI Changes

### 1. Map zone — replace legend with Resource Picker

**Remove:**
- `egui_plot::Legend` integration (`get_hidden_items`, `apply_legend_interaction`)

**Add** a custom overlay panel (top-right of map, similar position to current legend) containing:

| Control | Purpose |
|---------|---------|
| Resource type list / dropdown | Active resource for picking (highlighted row with color swatch) |
| Constraint list | One row per constrained node: `"Iron @ (x, y)"` with its **persisted** resource type; remove button per row |
| Clear all | Reset all constraints |
| Search / status | Trigger or auto-run search; show progress and result count |

**Enabled only when `mode ≠ none`:**
- Node picking, constraints, and seed search are **disabled** when Mode is `none` (resources are not randomized).
- Picker overlay shows inactive state: e.g. *"Select a randomization mode to search by node placement."*
- Switching Mode **to** `none`: clear constraints, stop search, clear cached results.
- Switching Mode **from** `none` to a randomizing mode: enable picker; fresh search cache.

**Map interaction while picker is active (`mode ≠ none`):**
- Nodes matching the **currently selected resource type** (and visible per View Options) render normally.
- Nodes whose resource type is **unchecked** in View Options render as **very faint gray** (still visible and clickable for picking).
- **Click hit-testing** on individual resource nodes and fracking cores (not just hover tooltips):
  - **First click** (unconstrained node): assign currently selected resource type; constraint persists.
  - **Re-click** with a **different** resource type selected: update that node's constraint to the new type.
  - **Re-click** same node with toggle semantics, or **remove** in constraint list: deselect / drop constraint.
- Visual distinction for **constrained nodes** (e.g. ring colored by **assigned** resource, not just active picker selection).

**Unchanged:** Sidebar View Options table — still the sole control for show/hide filtering.

### 2. Sidebar — seed results in empty area

**Replace** `ui.take_available_space()` spacer with a **Seed Results** panel:

```
┌─ stats_panel ─────────────────────────────┐
│  Seed Search Results                      │
│  ┌─────────────────────────────────────┐  │
│  │ 5526                                │  │  ← clickable rows
│  │ 10482                               │  │
│  │ 33001                               │  │
│  │ ...                                 │  │
│  └─────────────────────────────────────┘  │
│  "3 / 10 found — searching from seed 88421…" │
│  ─────────────────────────────────────────│
│  [View Options | Statistics] tabs         │
│  ...existing content...                   │
└───────────────────────────────────────────┘
```

- Scrollable list of matching seed numbers.
- Click row → set `App.seed`, invalidate `world`, map updates.
- Show `N / 10 found` and seeds scanned since resume when searching.
- Show constraint summary when idle; empty state if 0 matches after exhaustion.
- When `mode == none`: show inactive placeholder (no results, no search).
- Optional cancel to stop early (keeps partial cached results).

### 3. Optional third tab?

Alternative: add a **"Seed Search"** tab alongside View Options / Statistics instead of always showing results above tabs. **Default proposal:** always show results in the empty area (matches screenshot intent); can revisit if crowded.

---

## Backend: Seed Search Engine

### Search goal (decided)

- Return **up to 10 matching seeds** per query.
- Scan **as many seeds as necessary** to reach 10 — no fixed upper bound (full `i32` space if needed).
- **Cache** prior results and resume from the cached scan cursor rather than restarting from scratch.

### Constraint model

```rust
enum PickableNodeKind {
    ResourceNode,
    FrackingCore,
}

struct NodeConstraint {
    node_name: String,           // stable ID from default-world.json; unique key (one constraint per node)
    node_kind: PickableNodeKind,
    required_resource: ResourceDescriptor,  // set at click time; persists until changed or deselected
}
```

Constraints are stored as a **map keyed by `node_name`** (at most one entry per node). Multiple nodes may require **different** resource types in the same query (AND logic across nodes).

**Pickable targets:** anything whose **resource type** is randomized — **resource nodes** and **fracking cores**. Geysers and fracking satellites are excluded (geysers have no resource assignment; satellite purities are randomized but constraints are type-only).

Search uses **current** `randomization_mode` and `purity_settings` from the sidebar — only the seed varies.

**Gating:** If `randomization_mode == NodeRandomizationMode::None`, do not run search, do not accept new constraints, and clear active picker state.

### Cache model

```rust
struct SearchCacheKey {
    constraints: Vec<NodeConstraint>,  // order-normalized for stable hashing
    mode: NodeRandomizationMode,
    purity: NodePuritySettings,
}

struct SeedSearchCache {
    key: SearchCacheKey,
    matches: Vec<i32>,       // 0..=10 verified matches, newest search appends here
    next_seed: i32,          // next seed to evaluate on forward scan
    seeds_scanned: u64,      // total seeds evaluated for this key (for status UI)
    exhausted: bool,         // true if full i32 space scanned without reaching 10
}
```

**Cache invalidation:** Replace cache when `SearchCacheKey` changes (constraints added/removed/changed, or mode/purity changed). Do **not** discard the prior `matches` list immediately — reuse it as the starting set (see algorithm below).

### Algorithm (incremental, cached)

When constraints or settings change:

1. **Build new `SearchCacheKey`.**
2. **Seed from prior cache:** If there was a previous cache entry, take its `matches` list and **re-verify** each seed against the new constraints (fast batch step). Keep those that still pass — this is the **prior found set**.
3. **Resume scan:** Set `next_seed` from the old cache's cursor (or `0` on first search). Continue evaluating `next_seed`, `next_seed+1`, … until:
   - `matches.len() == 10`, or
   - full `i32` space exhausted (`exhausted = true`), or
   - user cancels.
4. For each candidate seed `s`:
   - Clone default world (parsed once, held in memory).
   - Call `apply_randomization_settings(&mut world, s, mode, purity)`.
   - Check all constraints by node `name` (`node.resource == required_resource`; purity not compared).
   - If pass, push `s` onto `matches` (stop adding once len == 10).
5. Update `next_seed` to last evaluated + 1 and persist cache.

**Constraint tightening** (added constraints): Prior matches are filtered in step 2; likely fewer remain; forward scan fills back up to 10.

**Constraint loosening** (removed constraints): Prior matches remain valid; forward scan adds more if still under 10.

**Mode/purity change:** New key → prior matches re-verified under new settings (most will drop); scan resumes from saved `next_seed` under new key (reset `next_seed` to `0` when key changes entirely, but still attempt to salvage verified matches from old cache first).

**Node lookup:** Build a `HashMap<String, ResourceDescriptor>` once per seed check, or index constraints by name for O(constraints) verification.

### Scan order

- Forward scan from `next_seed`, wrapping `i32::MAX → i32::MIN` if needed until exhausted.
- Fresh search (no prior cache): start at seed `0`, `matches = []`.

### Performance strategy

| Challenge | Mitigation |
|-----------|------------|
| Unbounded scan to find 10 | Stop immediately at 10; batched per-frame stepping |
| WASM single-thread | `step_search()` processes N seeds per frame via `egui` |
| Re-parsing JSON each seed | Load default world once at app start; clone struct per check |
| Repeated constraint changes | Cache cursor avoids re-scanning already-rejected seeds |
| UI freeze | Stream matches into list as found; show `N/10` progress |

**Estimated cost:** ~577 nodes per seed check. Profiling will set batch size (target 1,000–10,000 seeds/frame).

### New module

`src/seed_search.rs` (or `src/app/seed_search.rs`):
- `NodeConstraint`, `SearchCacheKey`, `SeedSearchCache`
- `filter_matches(cache, default_world)` — re-verify prior set
- `step_search(cache, default_world, batch_size) -> SearchStepResult` — `{ done, cancelled, matches_len, next_seed }`

Wire into `App` in `ui.rs`; trigger `step_search` each frame while `matches.len() < 10 && !exhausted`.

---

## Map Rendering Changes (`plot_item.rs`)

### Faint gray for hidden resource types

When resource picker mode is active (always on once legend is replaced):

- If `!view_options.is_target_visible(ResourceWithPurity(...))` for a node, draw with low-alpha gray instead of skipping (`continue`).
- Ensures unchecked resources remain pickable.

### Click detection

`egui_plot` items support `on_hover` today. Extend `ResourceDisplay` (or add wrapper) with:

- `on_click` or manual pointer handling via `plot_ui.response()` + transform screen coords → plot coords → nearest node within hit radius.
- Return clicked node `name` + location for constraint UI.

**Hit radius:** Scale with zoom (same `base_size` logic as markers).

### Constrained node highlight

Store `HashSet<String>` of constrained node names in new `ResourcePicker` state; pass to `ResourceDisplay` for outline/ring rendering.

---

## State Additions (`App` / new struct)

```rust
struct ResourcePicker {
    active_resource: ResourceDescriptor,
    constraints: Vec<NodeConstraint>,
}

struct SeedSearchState {
    cache: Option<SeedSearchCache>,
    searching: bool,
    cancelled: bool,
}
```

Remove legend-sync fields/methods from `ViewOptions` that are only used by the plot legend (`get_hidden_items`, `apply_legend_interaction`).

---

## Implementation Phases

### Phase 1 — Resource Picker UI (map zone)
- [ ] Add `ResourcePicker` struct and map overlay UI
- [ ] Remove plot legend; keep View Options filtering working
- [ ] Faint-gray rendering for hidden resource types
- [ ] Node click → add constraint
- [ ] Constraint list with remove / clear

### Phase 2 — Seed search backend
- [ ] `seed_search.rs` with batched incremental search
- [ ] Cache: prior matches + `next_seed` resume cursor
- [ ] Stop at 10 matches or exhaustion; cancel support

### Phase 3 — Results UI (sidebar)
- [ ] Replace `take_available_space()` with results panel
- [ ] Progress indicator during search
- [ ] Click seed → apply to Randomization Settings

### Phase 4 — Polish
- [ ] Persist constraints in URL query params (optional)
- [ ] Keyboard shortcuts (Esc clear, etc.)
- [ ] Empty states ("No constraints", "No matches found", "Only N matches exist")
- [ ] Performance tuning + WASM testing

---

## Decided Requirements

| Topic | Decision |
|-------|----------|
| Result count | **10 matches** maximum displayed |
| Scan scope | **As many seeds as needed** to reach 10; no fixed range cap |
| Caching | **Prior found set** re-verified on constraint change; forward scan resumes from `next_seed` cursor |
| Search trigger | **Auto-search** on constraint or mode/purity change |
| Constraint match | **Resource type only** — purity is ignored when verifying constraints |
| Pickable nodes | **All randomized resource assignments** — regular resource nodes and fracking cores |
| Mode gating | Node picking + search **only when mode ≠ `none`**; disabled and cleared otherwise |
| Per-node constraints | **Independent** — each node stores the resource type selected at click time; persists until changed or deselected |

## Open Questions

None — requirements captured. Fast search randomization approved 2026-06-06.

---

## Risks & Limitations

- **Rare constraint combos** may require scanning many seeds before finding 10; progress UI (`N/10`, seeds scanned) is essential.
- **Fewer than 10 matches may exist** for a given constraint set across the full seed space; UI must handle `exhausted` with partial results.
- **Mode `none`** disables the picker entirely (resources fixed; only purity varies per seed).
- **Legend removal** means resource-type bulk toggle moves exclusively to View Options (acceptable per requirements).
- **No precomputed index** — each seed requires full simulation; no known closed-form inverse of the game's RNG assignment.

---

## Out of Scope (v1)

- Searching across multiple modes/purities in one query
- Purity constraints
- Geyser / fracking-satellite picking (not resource-randomized at pickable granularity)
- Exporting results to file
- Server-side / precomputed seed database

---

## Planned: Fast Seed Search Randomization (2026-06-06)

### Problem

`seed_matches()` currently clones the full default `World` (~577 resource nodes + fracking + geysers) and calls `apply_randomization_settings()` for **every** candidate seed. That function:

- Sorts all node lists
- Builds and shuffles full resource + fracking pools
- Assigns resources to **every** node
- Applies purity overrides (even though constraints ignore purity)

The **map viewer** should keep using `apply_randomization_settings()` unchanged so the displayed world stays identical to today.

The **seed search path** (server parallel search + optional native fallback) should use a new function that produces the same resource assignments for constrained nodes, without building a full `World`.

### Why we cannot skip the entire map

Resource assignment is **sequential** and **order-dependent**:

1. All resource nodes are sorted by stable `name`.
2. A shared `node_pool` is built from default resources, optionally modified by mode (`BasicRich`, etc.), then shuffled.
3. For each node in sort order: draw a random pool index → assign that resource → remove from pool.

So the resource at node **N** depends on every pool draw **before** N in sorted order, not only on N itself. We **cannot** randomize constraint nodes in isolation without breaking correctness.

Purity settings can also consume RNG between pool draws (e.g. `AllRandom`), which affects later pool indices even though constraints match **resource type only**.

### Proposed fast path: `resolve_constrained_resources_for_search`

New function in `src/randomization.rs` (name TBD), used **only** by seed search:

```rust
/// Returns resource type for each constrained node name, or checks constraints in one pass.
pub fn seed_matches_fast(
    template: &WorldTemplate,  // precomputed, read-only
    seed: i32,
    mode: NodeRandomizationMode,
    purity: NodePuritySettings,
    constraints: &[(node_name, PickableNodeKind, ResourceDescriptor)],
) -> bool
```

**Same logic as today for correctness:**

- Same `RandomStream`, same pool construction, same `modify_node_distribution`, same shuffle, same per-node pool draws.
- Same `get_purity_override` calls when purity settings consume RNG (discard result; constraints ignore purity).
- Same fracking pool + assignment when **any** constraint targets a fracking core.

**Optimizations (safe):**

| Skip | When |
|------|------|
| `World` clone | Always — operate on pre-sorted template slices + indices |
| Geyser sorting / mutation | Always — geysers not in search |
| Fracking block | No fracking-core constraints |
| Satellite purity pass | Always for search (constraints are resource-only; fracking cores use `distribute_throughput`, not satellite purity, for `core.resource`) |
| Resource loop after last constrained node | Once all **resource-node** constraints are assigned — stop early, check failures |
| Writing purity / locations / full structs | Always — only compare `ResourceDescriptor` for constrained names |

**Precomputed once per `SearchCacheKey`** (cached alongside search state):

- Sorted resource node names + index into template
- Sorted fracking core names + index
- Default `ResourceNodeInfo` pools (from template, not cloned `World`)
- Set of constrained names + max sort index for early exit
- Flags: `needs_fracking_phase`, `needs_purity_rng` (any purity setting that calls `frand` during resource loop)

### Call sites

| Location | Before | After |
|----------|--------|-------|
| `src/seed_search.rs` → `seed_matches` | clone + `apply_randomization_settings` | `seed_matches_fast` + shared template |
| `server/src/search.rs` | `seed_matches` | same fast path |
| `src/app/ui.rs` / world display | `apply_randomization_settings` | **unchanged** |

### Expected speedup

- **Large:** no per-seed `World` clone/allocation.
- **Medium:** early exit when constraints cover few nodes (stop after last constrained index in name order, skip fracking if unused).
- **Small:** no geysers, no purity writes, no full struct updates.

Still **O(all resource nodes)** worst case when a constrained node sorts last by name. Typical picks (few nodes, scattered) should be much faster.

### Correctness requirement

Add tests (or dev-only assertion) that for random seeds + constraint sets:

```text
seed_matches_fast(...) == seed_matches(...)  // legacy full-world path
```

Keep legacy `seed_matches` internally for tests; route production search through fast path only after parity verified.

### Implementation phases

1. [x] **`WorldSearchTemplate`** — build once from default world: sorted names, default pool entries, constraint index maps.
2. [x] **`WorldSearchTemplate::seed_matches`** — fast RNG path with early exit.
3. [x] **Wire `seed_matches`** to use template + fast path; legacy path kept in unit tests for parity.
4. [x] **Server:** holds `Arc<WorldSearchTemplate>` instead of cloning `World` per seed check.
5. [ ] **Benchmark:** seeds/sec before vs after on representative 1 / 3 / 10 constraint queries.

### Decided (2026-06-06)

| Topic | Decision |
|-------|----------|
| Viewer randomization | Keep `apply_randomization_settings` unchanged |
| Search randomization | New fast path with early exit after last constrained node |
| Purity in search | Still advance RNG when settings require it; never compare purity in constraints |
| Fracking | Full fracking phase only if a fracking core is constrained |
| Early exit | **Yes** — stop after last constrained resource node in name order; skip fracking if unused |

### Open question

None for fast search — ready to implement.

---

## Test Plan

- [ ] Mode `none`: node clicks ignored, constraints cleared, results panel inactive
- [ ] Switch mode to `random`: picker enables; pick Iron + node → up to 10 seeds, each with Iron at that node
- [ ] Add second constraint; prior matches filtered, search resumes until 10 total
- [ ] Remove constraint; cached valid seeds kept, search fills toward 10 if needed
- [ ] Fracking core clickable same as resource node; constraint verified on `core.resource`
- [ ] Hidden resource (unchecked in View Options) appears faint gray and is still clickable
- [ ] View Options table unchanged; legend no longer present
- [ ] Multiple constraints with different resource types (AND logic) narrow results correctly
- [ ] Constraint persists when switching active resource type in picker; re-click updates type; deselect removes
- [ ] Cancel mid-search stops cleanly
- [ ] Click result seed updates map and share URL
- [ ] WASM build: search stops at 10; progress UI remains responsive

---

*Draft for review — created 2026-06-04*
