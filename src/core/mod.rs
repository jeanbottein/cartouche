//! Backend: the game library model and the pipeline phases. No UI code here;
//! both the terminal mode and the UI drive it.

pub mod app;
pub mod bps;
pub mod config;
pub mod detector;
pub mod init;
pub mod logging;
pub mod manifest;
pub mod migrations;
pub mod models;
pub mod mods;
pub mod package;
pub mod patcher;
pub mod paths;
pub mod persister;
pub mod pipeline;
pub mod platform;
pub mod proton;
pub mod saver;
pub mod scanner;
pub mod sgdb;
pub mod share;
pub mod sizer;
pub mod steam;
pub mod steam_self;
pub mod vdf;

#[cfg(test)]
pub mod testutil;

/// `items` without repeats, in first-seen order.
pub fn unique<T: PartialEq>(items: impl IntoIterator<Item = T>) -> Vec<T> {
    items.into_iter().fold(Vec::new(), |mut kept, item| {
        if !kept.contains(&item) {
            kept.push(item);
        }
        kept
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn unique_keeps_the_first_of_each_in_order() {
        assert_eq!(super::unique([3, 1, 3, 2, 1]), [3, 1, 2]);
    }
}
