use std::time::{Duration, Instant};

use egui::{Align, Layout, RichText};
use egui_extras::{Column, TableBuilder};
use egui_plot::PlotPoint;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::IntoEnumIterator;
use url::Url;

use crate::{
    app::{
        constants::get_resource_color,
        outline::WorldOutline,
        plot_item::{ResourceDisplay, ResourceDisplayContent},
        resource_picker::{self, ResourcePicker},
        view_options::ViewOptions,
    },
    game::{ResourceDescriptor, World},
    randomization::{NodePuritySettings, NodeRandomizationMode, apply_randomization_settings},
    seed_search::SearchCacheKey,
    stats::Stats,
};

#[cfg(not(target_arch = "wasm32"))]
use crate::seed_search::SeedSearchState;
#[cfg(target_arch = "wasm32")]
use crate::app::seed_search_client::RemoteSeedSearch;

#[derive(Serialize, Deserialize)]
struct QueryParams {
    seed: i32,
    mode: NodeRandomizationMode,
    purity: NodePuritySettings,
}

#[derive(PartialEq, Eq, Clone, Copy, strum::EnumIter, strum::Display)]
enum SidePanel {
    #[strum(to_string = "View Options")]
    ViewOptions,
    #[strum(to_string = "Statistics")]
    Stats,
}

pub struct App {
    seed: Option<i32>,
    randomization_mode: NodeRandomizationMode,
    purity_settings: NodePuritySettings,

    side_panel: SidePanel,

    world: Option<World>,
    stats: Stats,
    last_calc_duration: Duration,

    plot_id: egui::Id,
    view_options: ViewOptions,

    outline: WorldOutline,

    resource_picker: ResourcePicker,
    #[cfg(not(target_arch = "wasm32"))]
    seed_search: SeedSearchState,
    #[cfg(target_arch = "wasm32")]
    seed_search: RemoteSeedSearch,
    last_search_key: Option<SearchCacheKey>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            seed: None,
            randomization_mode: NodeRandomizationMode::None,
            purity_settings: NodePuritySettings::NoChange,

            side_panel: SidePanel::ViewOptions,

            world: None,
            stats: Stats::new(),
            last_calc_duration: Duration::ZERO,

            plot_id: egui::Id::new("map_display_plot"),
            view_options: ViewOptions::new(),

            outline: WorldOutline::new(),

            resource_picker: ResourcePicker::new(),
            #[cfg(not(target_arch = "wasm32"))]
            seed_search: SeedSearchState::new(Self::default_world_template()),
            #[cfg(target_arch = "wasm32")]
            seed_search: RemoteSeedSearch::new(),
            last_search_key: None,
        }
    }
}

impl App {
    pub const PUBLIC_URL: Option<&'static str> = option_env!("PUBLIC_URL");

    pub fn new(_cc: &eframe::CreationContext<'_>, startup_url: Option<&str>) -> Self {
        if let Some(params) = startup_url
            .and_then(|url| Url::parse(url).ok())
            .and_then(|url| serde_urlencoded::from_str::<QueryParams>(url.query()?).ok())
        {
            Self {
                seed: Some(params.seed),
                randomization_mode: params.mode,
                purity_settings: params.purity,

                ..Default::default()
            }
        } else {
            Default::default()
        }
    }

    pub const fn supports_share_link() -> bool {
        Self::PUBLIC_URL.is_some()
    }

    pub fn create_share_link(&self) -> Option<String> {
        let params = QueryParams {
            seed: self.seed.unwrap_or(0),
            mode: self.randomization_mode,
            purity: self.purity_settings,
        };
        let query_str = serde_urlencoded::to_string(params).ok()?;

        let mut url = Url::parse(Self::PUBLIC_URL?).ok()?;
        url.set_query(Some(&query_str));

        Some(url.to_string())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get_time() -> Instant {
        Instant::now()
    }

    #[cfg(target_arch = "wasm32")]
    fn get_time() -> f64 {
        web_sys::window()
            .expect("no window")
            .performance()
            .expect("no performance")
            .now()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get_elapsed_duration(start_time: Instant) -> Duration {
        start_time.elapsed()
    }

    #[cfg(target_arch = "wasm32")]
    fn get_elapsed_duration(start_time: f64) -> Duration {
        Duration::from_secs_f64((Self::get_time() - start_time) / 1000.0)
    }

    fn default_world_template() -> World {
        serde_json::from_str(include_str!("../default-world.json")).unwrap()
    }

    fn picker_enabled(&self) -> bool {
        ResourcePicker::is_enabled(self.randomization_mode)
    }

    fn sync_seed_search(&mut self, ctx: &egui::Context) {
        if !self.picker_enabled() {
            self.resource_picker.clear();
            self.seed_search.clear();
            self.last_search_key = None;
            return;
        }

        let key = SearchCacheKey::new(
            &self.resource_picker.constraints_vec(),
            self.randomization_mode,
            self.purity_settings,
        );

        if self.last_search_key.as_ref() == Some(&key) {
            return;
        }

        let prior_matches = self.seed_search.matches.clone();
        #[cfg(not(target_arch = "wasm32"))]
        self.seed_search.on_settings_changed(key.clone(), &prior_matches);
        #[cfg(target_arch = "wasm32")]
        {
            let constraints = self.resource_picker.constraints_vec();
            self.seed_search
                .on_settings_changed(key.clone(), &prior_matches, &constraints, ctx);
        }
        self.last_search_key = Some(key);
    }

    fn seed_results_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Seed Search Results");

        if !self.picker_enabled() {
            ui.label(self.seed_search.status_text());
            return;
        }

        if self.resource_picker.constraints_vec().is_empty() {
            ui.label(self.seed_search.status_text());
            return;
        }

        ui.label(self.seed_search.status_text());

        let matches = self.seed_search.matches.clone();
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 24.0)
            .show(ui, |ui| {
                for seed in matches {
                    if ui.selectable_label(false, seed.to_string()).clicked() {
                        self.seed = Some(seed);
                        self.world = None;
                    }
                }
            });
    }

    fn stats_ui(&self, ui: &mut egui::Ui) {
        let available_height = ui.available_height();
        let table = TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::remainder())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .column(Column::auto())
            .min_scrolled_height(0.0)
            .max_scroll_height(available_height);

        table
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Resource");
                });

                for mk in Stats::MINER_MK_RANGE {
                    for speed in Stats::CLOCK_SPEEDS {
                        header.col(|ui| {
                            ui.strong(format!("Mk. {}\n{} %", mk, speed));
                        });
                    }
                }
            })
            .body(|mut body| {
                for resource in ResourceDescriptor::iter() {
                    body.row(18.0, |mut row| {
                        row.col(|ui| {
                            ui.label(
                                RichText::new("\u{23FA}")
                                    .color(get_resource_color(resource, ui.visuals().dark_mode)),
                            );
                            ui.label(resource.to_string());
                        });

                        for mk in Stats::MINER_MK_RANGE {
                            for speed in Stats::CLOCK_SPEEDS {
                                let amount = self.stats.get(speed, mk, resource);

                                row.col(|ui| {
                                    ui.label(format!("{}", amount));
                                });
                            }
                        }
                    });
                }
            });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.global_style_mut(|style| style.interaction.selectable_labels = false);

        egui::Panel::top("top_bar")
            .frame(egui::Frame::side_top_panel(ui.style()).inner_margin(4))
            .show_inside(ui, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    egui::widgets::global_theme_preference_buttons(ui);
                });
            });

        let mut view_options_highlight = None;

        egui::Panel::right("settings_panel")
            .resizable(true)
            .min_size(400.0)
            .show_inside(ui, |ui| {
                egui::Panel::bottom("stats_panel")
                    .resizable(true)
                    .min_size(200.0)
                    .default_size(380.0)
                    .show_inside(ui, |ui| {
                        self.seed_results_ui(ui);
                        ui.add_space(5.0);
                        ui.separator();

                        ui.horizontal(|ui| {
                            SidePanel::iter().for_each(|v| {
                                ui.selectable_value(&mut self.side_panel, v, v.to_string());
                            })
                        });
                        ui.separator();

                        match self.side_panel {
                            SidePanel::ViewOptions => {
                                self.view_options.ui(ui, &mut view_options_highlight);
                            }

                            SidePanel::Stats => {
                                self.stats_ui(ui);
                            }
                        }
                    });

                egui::CentralPanel::default().show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Randomization Settings");
                    });
                    ui.add_space(5.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        egui::Grid::new("settings_grid")
                            .num_columns(2)
                            .spacing([40.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                ui.label("Seed");

                                ui.with_layout(
                                    Layout::right_to_left(egui::Align::Center)
                                        .with_cross_justify(true),
                                    |ui| {
                                        let randomize_seed =
                                            ui.button("\u{1F3B2} random").clicked();
                                        let mut seed_text = self
                                            .seed
                                            .map(|seed| seed.to_string())
                                            .unwrap_or_default();
                                        if ui
                                            .add(
                                                egui::TextEdit::singleline(&mut seed_text)
                                                    .hint_text("0"),
                                            )
                                            .changed()
                                        {
                                            self.world = None;
                                        }

                                        if randomize_seed {
                                            self.seed = Some(rand::random());
                                            self.world = None;
                                        } else if seed_text.is_empty() {
                                            self.seed = None;
                                        } else if let Ok(seed) = seed_text.trim().parse::<i32>() {
                                            self.seed = Some(seed);
                                        }
                                    },
                                );

                                ui.end_row();

                                ui.label("Mode");
                                egui::ComboBox::from_id_salt("mode_setting")
                                    .selected_text(self.randomization_mode.to_string())
                                    .show_ui(ui, |ui| {
                                        NodeRandomizationMode::iter().for_each(|m| {
                                            if ui
                                                .selectable_value(
                                                    &mut self.randomization_mode,
                                                    m,
                                                    m.to_string(),
                                                )
                                                .changed()
                                            {
                                                self.world = None;
                                                self.last_search_key = None;
                                                if m == NodeRandomizationMode::None {
                                                    self.resource_picker.clear();
                                                    self.seed_search.clear();
                                                }
                                            }
                                        });
                                    });
                                ui.end_row();

                                ui.label("Purity");
                                egui::ComboBox::from_id_salt("purity_setting")
                                    .selected_text(self.purity_settings.to_string())
                                    .show_ui(ui, |ui| {
                                        NodePuritySettings::iter().for_each(|p| {
                                            if ui
                                                .selectable_value(
                                                    &mut self.purity_settings,
                                                    p,
                                                    p.to_string(),
                                                )
                                                .changed()
                                            {
                                                self.world = None;
                                                self.last_search_key = None;
                                            }
                                        });
                                    });
                                ui.end_row();
                            });
                    });

                    if Self::supports_share_link() {
                        ui.add_space(15.0);

                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            if ui.button("\u{1F4CB} copy share url").clicked()
                                && let Some(link) = self.create_share_link()
                            {
                                ui.copy_text(link);
                            }
                        });
                    }
                });
            });

        // Seed-search state is independent of the generated world, so we update it
        // before borrowing `self.world` below.
        self.sync_seed_search(ui.ctx());
        #[cfg(target_arch = "wasm32")]
        {
            self.seed_search.poll(ui.ctx());
            if self.seed_search.searching {
                ui.ctx().request_repaint();
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.seed_search.searching {
            self.seed_search.step();
        }

        let picker_enabled = self.picker_enabled();
        // Clone to avoid borrow conflicts: the UI closure may mutate `self.resource_picker`.
        let constrained_nodes = self.resource_picker.constraint_resources().clone();

        let world = self.world.get_or_insert_with(|| {
            let start_time = Self::get_time();

            let mut world: World =
                serde_json::from_str(include_str!("../default-world.json")).unwrap();

            apply_randomization_settings(
                &mut world,
                self.seed.unwrap_or_default(),
                self.randomization_mode,
                self.purity_settings,
            );
            self.stats.compute(&world);
            self.view_options.get_existing_nodes(&world);

            self.last_calc_duration = Self::get_elapsed_duration(start_time);
            world
        });

        egui::Panel::bottom("status_panel").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                if !self.last_calc_duration.is_zero() {
                    ui.label(format!(
                        "calculation took {:.2} ms",
                        self.last_calc_duration.as_secs_f64() * 1000.0
                    ));
                }

                ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(world.game_version.clone());
                });
            })
        });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let plot = egui_plot::Plot::new("main_display_plot")
                .show_axes(true)
                .show_grid(true)
                .data_aspect(1.0)
                .invert_y(true)
                .id(self.plot_id);

            let is_dark_mode = ui.visuals().dark_mode;

            let plot_response = plot.show(ui, |plot_ui| {
                plot_ui.add(self.outline.plot_item());

                let test_rect = plot_ui
                    .transform()
                    .rect_from_values(&PlotPoint::new(0.0, 0.0), &PlotPoint::new(1.0, 1.0));
                let scale = (test_rect.width() + test_rect.height()) / 2.0;
                let base_size = (5000.0 * scale).clamp(5.0, 20.0);

                // resource nodes
                for (resource, nodes) in &world.resource_nodes.iter().chunk_by(|n| n.resource) {
                    plot_ui.add(ResourceDisplay::new(
                        base_size,
                        ResourceDisplayContent::ResourceNodes(resource, nodes.collect()),
                        &self.view_options,
                        view_options_highlight,
                        is_dark_mode,
                        picker_enabled,
                        &constrained_nodes,
                    ));
                }

                // fracking nodes
                for (resource, cores) in &world.fracking_cores.iter().chunk_by(|c| c.resource) {
                    plot_ui.add(ResourceDisplay::new(
                        base_size,
                        ResourceDisplayContent::FrackingNodes(resource, cores.collect()),
                        &self.view_options,
                        view_options_highlight,
                        is_dark_mode,
                        picker_enabled,
                        &constrained_nodes,
                    ));
                }

                // geysers
                plot_ui.add(ResourceDisplay::new(
                    base_size,
                    ResourceDisplayContent::Geysers(world.geysers.iter().by_ref().collect()),
                    &self.view_options,
                    view_options_highlight,
                    is_dark_mode,
                    picker_enabled,
                    &constrained_nodes,
                ));
            });

            let mut picker_changed = self
                .resource_picker
                .map_overlay_ui(ui, picker_enabled);

            if picker_enabled && plot_response.response.clicked() {
                if let Some(pointer) = plot_response.response.interact_pointer_pos() {
                    let test_rect = plot_response.transform.rect_from_values(
                        &PlotPoint::new(0.0, 0.0),
                        &PlotPoint::new(1.0, 1.0),
                    );
                    let scale = (test_rect.width() + test_rect.height()) / 2.0;
                    let hit_base_size = (5000.0 * scale).clamp(5.0, 20.0);

                    let pickable_hits =
                        resource_picker::collect_pickable_hits(world, hit_base_size);
                    if let Some(hit) = resource_picker::find_pickable_hit(
                        &pickable_hits,
                        pointer,
                        &plot_response.transform,
                    ) {
                        self.resource_picker.toggle_node(
                            hit.name.clone(),
                            hit.kind,
                            hit.location,
                        );
                        picker_changed = true;
                    }
                }
            }

            if picker_changed {
                self.last_search_key = None;
            }
        });
    }
}
