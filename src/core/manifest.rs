//! Pipeline step 7: `manifests.json` for Steam ROM Manager.

use std::path::Path;

use log::{error, info};
use serde_json::{json, Value};

use super::models::{to_pretty_json, Game};
use super::paths;

fn entry(game: &Game) -> Option<Value> {
    let resolved = game.resolved.as_ref()?;
    let mut entry = json!({
        "title": game.title,
        "target": paths::lossy(&resolved.target),
        "startIn": paths::lossy(&resolved.start_in),
        "launchOptions": resolved.launch_options,
        "savePath": game.resolved_save_paths.first().map(|p| paths::lossy(p)).unwrap_or_default(),
    });
    if let Some(id) = game.steamgriddb_id {
        entry["steamgriddb_id"] = json!(id);
    }
    Some(entry)
}

/// Write `output`, unless it already holds this list.
pub fn write(games: &[Game], output: &Path) {
    let entries: Vec<Value> = games.iter().filter_map(entry).collect();
    if entries.is_empty() {
        info!("No games with targets to write to manifests.json");
        return;
    }
    let text = to_pretty_json(&entries);
    if std::fs::read_to_string(output).is_ok_and(|current| current == text) {
        return;
    }
    let result = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| paths::write_atomic(output, &text));
    match result {
        Ok(()) => info!(
            "Wrote manifests.json ({} games) to {}",
            entries.len(),
            output.display()
        ),
        Err(e) => error!("Failed to write manifests.json: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;
    use crate::core::testutil::TempDir;

    #[test]
    fn writes_only_games_with_targets() {
        let dir = TempDir::new();
        let mut game = Game {
            title: "Celeste".into(),
            steamgriddb_id: Some(9),
            ..Default::default()
        };
        game.resolved = Some(Resolved {
            target: "/g/c".into(),
            start_in: "/g".into(),
            ..Default::default()
        });
        game.resolved_save_paths = vec!["/s/1".into(), "/s/2".into()];
        let games = vec![
            game,
            Game {
                title: "No target".into(),
                ..Default::default()
            },
        ];
        let out = dir.path().join("sub/manifests.json");
        write(&games, &out);

        let written: Value = serde_json::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
        assert_eq!(
            written,
            json!([{"title": "Celeste", "target": "/g/c", "startIn": "/g", "launchOptions": "", "savePath": "/s/1", "steamgriddb_id": 9}])
        );
    }

    #[test]
    fn an_unchanged_list_is_not_written_again() {
        let dir = TempDir::new();
        let game = Game {
            resolved: Some(Resolved::default()),
            ..Default::default()
        };
        let out = dir.path().join("manifests.json");
        write(std::slice::from_ref(&game), &out);
        let modified = || std::fs::metadata(&out).unwrap().modified().unwrap();
        let first = modified();
        std::thread::sleep(std::time::Duration::from_millis(10));
        write(&[game], &out);
        assert_eq!(modified(), first);
    }
}
