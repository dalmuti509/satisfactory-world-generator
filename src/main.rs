#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crate::app::run_app;

mod app;

pub use satisfactory_world_generator::{
    game, random_stream, randomization, search_template, seed_search, stats,
};

fn main() {
    run_app().unwrap();
}
