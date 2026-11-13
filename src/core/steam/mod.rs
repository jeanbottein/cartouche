//! Steam integration: non-Steam shortcuts in `userdata/<id>/config/shortcuts.vdf`
//! (pipeline steps 5 and 6), their artwork in the `grid/` folder, and Proton
//! compatibility mappings in `config/config.vdf`.
//!
//! Shortcuts we own carry this cartridge's ownership tag (`cartridge:<id>`),
//! so other non-Steam games, and other cartridges' games, are never touched.
//! The cartridge's label (its folder name) is a second, visible tag.
//!
//! The user picks which games are in Steam, per game (Library, Y);
//! `system/steam.json` remembers the choice, and the pipeline keeps exactly
//! those shortcuts up to date. Steam reads shortcuts at startup: a change
//! shows after Steam restarts (see `is_running`).

use std::path::Path;

use super::models::Game;
use super::paths;

pub const PROTON_PREFIX: &str = "~/.local/share/Steam/steamapps/compatdata/{appid}/pfx/drive_c";

pub fn proton_prefix(appid: u32) -> String {
    PROTON_PREFIX.replace("{appid}", &appid.to_string())
}

/// Stable non-Steam shortcut AppID (what Steam itself computes).
pub fn generate_appid(name: &str, exe: &str) -> u32 {
    crc32fast::hash(format!("{exe}{name}").as_bytes()) | 0x8000_0000
}

/// The AppID of `game`'s shortcut and Proton prefix when it runs `exe`: the
/// one game.json keeps, else the one its title gives now.
pub fn appid_for(game: &Game, exe: &Path) -> u32 {
    game.steam_appid
        .unwrap_or_else(|| generate_appid(&game.title, &paths::lossy(exe)))
}

/// `appid_for` the game's resolved target.
pub fn appid_of(game: &Game) -> Option<u32> {
    game.resolved.as_ref().map(|r| appid_for(game, &r.target))
}

/// `shortcuts.vdf` stores AppIDs as signed 32-bit ints.
pub fn signed32(appid: u32) -> i32 {
    appid as i32
}

mod clean;
mod compat;
mod export;
mod host;
mod selection;
mod shortcuts;
mod users;

pub use clean::*;
pub use compat::*;
pub use export::*;
pub use host::*;
pub use selection::*;
pub use shortcuts::*;
pub use users::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Resolved;

    #[test]
    fn appid_matches_python_generate_appid() {
        assert_eq!(
            generate_appid("Hollow Knight", "/g/Hollow Knight/hollow_knight.exe"),
            3770780903
        );
        assert_eq!(
            generate_appid("Hollow Knight", "/g/HK/hk.x86_64"),
            2664893048
        );
        assert_eq!(signed32(2664893048), -1630074248);
    }

    #[test]
    fn without_a_stored_appid_it_comes_from_title_and_exe() {
        let mut game = Game::skeleton(Path::new("/g/Hollow Knight"));
        game.title = "Hollow Knight".into();
        game.resolved = Some(Resolved {
            target: "/g/Hollow Knight/hollow_knight.exe".into(),
            ..Default::default()
        });
        assert_eq!(appid_of(&game), Some(3770780903));
    }
}
