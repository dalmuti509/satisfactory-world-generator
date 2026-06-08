use std::collections::HashMap;

use egui::{RichText, Ui};
use strum::IntoEnumIterator;

use crate::{
    app::constants::get_resource_color,
    game::{ResourceDescriptor, World},
    randomization::NodeRandomizationMode,
    seed_search::{NodeConstraint, PickableNodeKind},
};

pub struct ResourcePicker {
    active_resource: ResourceDescriptor,
    constraints: HashMap<String, NodeConstraint>,
    constraint_resources_map: HashMap<String, ResourceDescriptor>,
}

impl ResourcePicker {
    pub fn new() -> Self {
        Self {
            active_resource: ResourceDescriptor::OreIron,
            constraints: HashMap::new(),
            constraint_resources_map: HashMap::new(),
        }
    }

    pub fn is_enabled(mode: NodeRandomizationMode) -> bool {
        mode != NodeRandomizationMode::None
    }

    pub fn clear(&mut self) {
        self.constraints.clear();
        self.constraint_resources_map.clear();
    }

    pub fn constraints_vec(&self) -> Vec<NodeConstraint> {
        self.constraints.values().cloned().collect()
    }

    pub fn constraint_resources(&self) -> &HashMap<String, ResourceDescriptor> {
        &self.constraint_resources_map
    }

    pub fn toggle_node(
        &mut self,
        node_name: String,
        node_kind: PickableNodeKind,
        location: [f32; 3],
    ) {
        if let Some(existing) = self.constraints.get(&node_name) {
            if existing.required_resource == self.active_resource {
                self.constraints.remove(&node_name);
                self.constraint_resources_map.remove(&node_name);
            } else {
                let entry = self.constraints.get_mut(&node_name).unwrap();
                entry.required_resource = self.active_resource;
                self.constraint_resources_map
                    .insert(node_name, self.active_resource);
            }
        } else {
            self.constraints.insert(
                node_name.clone(),
                NodeConstraint {
                    node_name: node_name.clone(),
                    node_kind,
                    required_resource: self.active_resource,
                    location,
                },
            );
            self.constraint_resources_map
                .insert(node_name, self.active_resource);
        }
    }

    pub fn remove_constraint(&mut self, node_name: &str) {
        self.constraints.remove(node_name);
        self.constraint_resources_map.remove(node_name);
    }

    pub fn map_overlay_ui(&mut self, ui: &mut Ui, enabled: bool) -> bool {
        let mut changed = false;

        egui::Area::new(egui::Id::new("resource_picker_overlay"))
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 8.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(180.0);
                    ui.heading("Resource Picker");

                    if !enabled {
                        ui.label("Select a randomization mode to search by node placement.");
                        return;
                    }

                    ui.label("Select resource, then click nodes on the map.");
                    ui.separator();

                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .show(ui, |ui| {
                            for resource in ResourceDescriptor::iter() {
                                let selected = self.active_resource == resource;
                                let color =
                                    get_resource_color(resource, ui.visuals().dark_mode);

                                if ui
                                    .selectable_label(
                                        selected,
                                        RichText::new(format!("\u{23FA} {}", resource))
                                            .color(color),
                                    )
                                    .clicked()
                                {
                                    self.active_resource = resource;
                                }
                            }
                        });

                    if !self.constraints.is_empty() {
                        ui.separator();
                        ui.label("Constraints:");

                        let mut to_remove = None;
                        let is_dark = ui.visuals().dark_mode;

                        let mut entries: Vec<_> = self.constraints.values().collect();
                        entries.sort_by(|a, b| a.node_name.cmp(&b.node_name));

                        for constraint in entries {
                            ui.horizontal(|ui| {
                                let color =
                                    get_resource_color(constraint.required_resource, is_dark);
                                ui.label(
                                    RichText::new(format!(
                                        "{} @ {:.0}, {:.0}",
                                        constraint.required_resource,
                                        constraint.location[0],
                                        constraint.location[1],
                                    ))
                                    .color(color),
                                );
                                if ui.small_button("\u{2715}").clicked() {
                                    to_remove = Some(constraint.node_name.clone());
                                }
                            });
                        }

                        if let Some(name) = to_remove {
                            self.remove_constraint(&name);
                            changed = true;
                        }

                        if ui.button("clear all").clicked() {
                            self.clear();
                            changed = true;
                        }
                    }
                });
            });

        changed
    }
}

impl Default for ResourcePicker {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PickableHit {
    pub name: String,
    pub kind: PickableNodeKind,
    pub location: [f32; 3],
    pub plot_x: f64,
    pub plot_y: f64,
    pub hit_radius_screen: f32,
}

pub fn collect_pickable_hits(world: &World, base_size: f32) -> Vec<PickableHit> {
    let mut hits =
        Vec::with_capacity(world.resource_nodes.len() + world.fracking_cores.len());

    for node in &world.resource_nodes {
        hits.push(PickableHit {
            name: node.name.clone(),
            kind: PickableNodeKind::ResourceNode,
            location: node.location,
            plot_x: node.location[0] as f64,
            plot_y: node.location[1] as f64,
            hit_radius_screen: base_size,
        });
    }

    for core in &world.fracking_cores {
        hits.push(PickableHit {
            name: core.name.clone(),
            kind: PickableNodeKind::FrackingCore,
            location: core.location,
            plot_x: core.location[0] as f64,
            plot_y: core.location[1] as f64,
            hit_radius_screen: base_size * 1.5,
        });
    }

    hits
}

pub fn find_pickable_hit<'a>(
    hits: &'a [PickableHit],
    pointer: egui::Pos2,
    transform: &egui_plot::PlotTransform,
) -> Option<&'a PickableHit> {
    hits.iter()
        .filter_map(|hit| {
            let center = transform.position_from_point(&egui_plot::PlotPoint::new(
                hit.plot_x, hit.plot_y,
            ));
            let dist = pointer.distance(center);
            if dist <= hit.hit_radius_screen {
                Some((hit, dist))
            } else {
                None
            }
        })
        .min_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(hit, _)| hit)
}
