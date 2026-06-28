//! Bridge layer: data <-> the Cartouche backend. Reads `config.txt` and
//! `.cartouche/game.json` directly for browsing; the pipeline subprocess runner
//! (BRIDGE-3) lands with the Status screen wiring.

pub mod config;
pub mod library;
pub mod models;
