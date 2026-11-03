//! Backend: the game library model and the pipeline phases. No UI code here;
//! both the terminal mode and the UI drive it.

pub mod app;
pub mod bps;
pub mod commands;
pub mod config;
pub mod detector;
pub mod init;
pub mod logging;
pub mod manifest;
pub mod migrations;
pub mod models;
pub mod package;
pub mod patcher;
pub mod paths;
pub mod persister;
pub mod pipeline;
pub mod platform;
pub mod saver;
pub mod scanner;
pub mod sgdb;
pub mod sizer;
pub mod steam;
pub mod steam_self;
pub mod vdf;

#[cfg(test)]
pub mod testutil;
