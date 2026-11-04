//! Application shell: shared state, the tab bar and footer, global toggles,
//! the "now playing" lock while a game runs, and per-screen dispatch.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use super::editor::{self, Editor};
use super::engine::canvas::{AspectMode, Canvas};
use super::engine::input::{Action, Input};
use super::engine::{text, theme};
use super::jobs::{Jobs, Kind};
use super::launcher::Session;
use super::library::{self, Library};
use super::music::Music;
use super::network::{self, Network};
use super::settings::Settings;
use super::setup::{self, Setup};
use super::status::Status;
use super::{Hint, Ui};
use crate::core::app::find_app_dir;
use crate::core::config::Config;
use crate::core::logging::{self, Line};
use crate::core::package;
use crate::core::share::{self, client, server::Server};
use crate::core::{init, migrations};

const MARGIN: f32 = 16.0;
const HEADER_H: f32 = 34.0;
const CONTENT_TOP: f32 = HEADER_H + 2.0;
const FOOTER_H: f32 = 22.0;

/// Holding Back this long on the Library quits the app.
const HOLD_TO_QUIT_SECS: f64 = 1.0;
/// Input stays suspended this long after a game exits, so the button press that
/// quit the game (often still queued) can't also confirm a relaunch.
const RESUME_GRACE_SECS: f64 = 0.6;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Library,
    Network,
    Status,
    Settings,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::Library, "LIBRARY"),
    (Tab::Network, "NETWORK"),
    (Tab::Status, "STATUS"),
    (Tab::Settings, "SETTINGS"),
];

impl Tab {
    fn index(self) -> usize {
        TABS.iter().position(|(t, _)| *t == self).unwrap_or(0)
    }

    fn step(self, delta: isize) -> Tab {
        let n = TABS.len() as isize;
        TABS[((self.index() as isize + delta).rem_euclid(n)) as usize].0
    }
}

pub struct App {
    ui: Ui,
    config: Option<Config>,
    setup: Option<Setup>,
    tab: Tab,
    fullscreen: bool,
    library: Library,
    network: Network,
    /// File transfers running in the background (unpack, pack, copies).
    jobs: Jobs,
    editor: Option<Editor>,
    status: Status,
    settings: Option<Settings>,
    logs: Receiver<Line>,
    /// The game currently running; UI input is suspended while it plays.
    session: Option<Session>,
    input_resume_at: f64,
    /// LAN sharing server, while `SHARE_ENABLED` is on.
    server: Option<Server>,
    /// Background music, while `MUSIC_ENABLED` is on and there are tracks.
    music: Option<Music>,
    /// Back is still held from closing the editor: don't count it as hold-to-quit.
    back_held_over: bool,
    quit: bool,
}

impl App {
    pub fn new(cli_dir: Option<PathBuf>) -> App {
        let (tx, logs) = channel();
        logging::attach(tx);
        let mut app = App {
            ui: Ui {
                canvas: Canvas::new(AspectMode::Wide),
                input: Input::new(),
                font: text::load_pixel_font(),
            },
            config: None,
            setup: None,
            tab: Tab::Library,
            fullscreen: true, // see window_conf
            library: Library::default(),
            network: Network::default(),
            jobs: Jobs::default(),
            editor: None,
            status: Status::new(),
            settings: None,
            logs,
            session: None,
            input_resume_at: 0.0,
            server: None,
            music: None,
            back_held_over: false,
            quit: false,
        };
        match find_app_dir(cli_dir.as_deref()) {
            Some(app_dir) => app.open(Config::load(&init::config_in(&app_dir))),
            None => app.setup = Some(Setup::new()),
        }
        app
    }

    fn open(&mut self, cfg: Config) {
        // Upgrade old game.json formats before anything reads or edits them.
        if let Some(dir) = cfg.games_dir() {
            migrations::run_all(&dir);
        }
        self.library.load(&cfg);
        self.settings = Some(Settings::new(&cfg));
        self.config = Some(cfg);
        self.sync_server();
        self.sync_music();
    }

    /// Start or stop the music to match the config (and the workspace folder).
    fn sync_music(&mut self) {
        let Some(cfg) = &self.config else { return };
        if !cfg.is_not_false("MUSIC_ENABLED") {
            self.music = None;
            return;
        }
        let dir = cfg.app_dir();
        if self.music.as_ref().is_some_and(|m| m.dir() == dir) {
            return;
        }
        self.music = Music::start(&dir);
    }

    /// Start, restart (port or folder changed) or stop the sharing server to
    /// match the config.
    fn sync_server(&mut self) {
        let Some(cfg) = &self.config else { return };
        let wanted = cfg
            .games_dir()
            .filter(|_| cfg.is_true("SHARE_ENABLED"))
            .map(|dir| (dir, share::port(cfg)));
        let Some((games_dir, port)) = wanted else {
            self.server = None;
            return;
        };
        if self
            .server
            .as_ref()
            .is_some_and(|s| s.port() == port && s.games_dir() == games_dir)
        {
            return;
        }
        self.server = None; // free the port before binding it again
        self.server = Server::start(&games_dir, share::machine_name(cfg), port)
            .map_err(|e| log::error!("Cannot share on port {port}: {e}"))
            .ok();
    }

    /// Re-read `config.txt` (after Settings changed it or a pipeline run).
    fn reload_config(&mut self) {
        let Some(path) = self.config.as_ref().map(|c| c.path.clone()) else {
            return;
        };
        let cfg = Config::load(&path);
        if let Some(settings) = &mut self.settings {
            settings.refresh(&cfg);
        }
        self.config = Some(cfg);
        self.sync_server();
        self.sync_music();
    }

    fn reload_library(&mut self) {
        if let Some(cfg) = &self.config {
            self.library.load(cfg);
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    fn content_area(&self) -> Rect {
        let (w, h) = self.ui.canvas.size();
        Rect::new(
            MARGIN,
            CONTENT_TOP,
            w - 2.0 * MARGIN,
            h - FOOTER_H - 6.0 - CONTENT_TOP,
        )
    }

    pub fn frame(&mut self) {
        self.ui.canvas.fit();
        while let Ok(line) = self.logs.try_recv() {
            self.status.push_log(line);
        }
        self.library.poll();
        self.network.poll(self.config.as_ref());
        if let Some(music) = &mut self.music {
            music.update(get_frame_time(), self.session.is_some());
        }
        self.collect_jobs();
        if self.status.poll().is_some() {
            self.reload_config();
            self.reload_library();
        }

        if self.session.as_ref().is_some_and(|s| !s.running()) {
            self.session = None;
            self.input_resume_at = get_time() + RESUME_GRACE_SECS;
        }
        if self.session.is_some() || get_time() < self.input_resume_at {
            // The game owns the controller (gilrs reads devices regardless of
            // window focus): track input without acting on it, also briefly
            // after it exits, to swallow the press that quit the game.
            self.ui.input.update_suspended();
            std::thread::sleep(std::time::Duration::from_millis(50)); // idle cheaply
        } else {
            self.ui.input.update();
            let typing = self.ui.input.text_mode;
            self.ui.input.text_mode = false; // the on-screen keyboard re-enables it while open
            if !typing {
                self.handle_global();
            }
            self.update_screen();
        }
        self.draw();
    }

    /// Toast finished transfers and show their result in both lists.
    fn collect_jobs(&mut self) {
        let finished = self.jobs.poll();
        if finished.is_empty() {
            return;
        }
        self.reload_library();
        let games_dir = self.config.as_ref().and_then(Config::games_dir);
        self.network.recheck_local(games_dir.as_deref());
        for done in finished {
            let color = if done.ok { theme::ACCENT } else { theme::RED };
            self.library.toast(&done.message, color);
            self.network.toast(&done.message, color);
        }
    }

    fn modal_open(&self) -> bool {
        self.editor.is_some()
            || self.network.has_modal()
            || self.setup.is_some()
            || self.settings.as_ref().is_some_and(Settings::has_modal)
    }

    fn handle_global(&mut self) {
        if is_key_pressed(KeyCode::F) {
            self.fullscreen = !self.fullscreen;
            macroquad::window::set_fullscreen(self.fullscreen);
        }
        if self.ui.action(Action::ToggleAspect) {
            self.ui.canvas.toggle_aspect();
        }
        if self.modal_open() {
            return;
        }
        for (key, (tab, _)) in [KeyCode::Key1, KeyCode::Key2, KeyCode::Key3, KeyCode::Key4]
            .iter()
            .zip(TABS)
        {
            if is_key_pressed(*key) {
                self.tab = tab;
            }
        }
        if self.ui.action(Action::TabNext) {
            self.tab = self.tab.step(1);
        }
        if self.ui.action(Action::TabPrev) {
            self.tab = self.tab.step(-1);
        }
    }

    fn update_screen(&mut self) {
        let area = self.content_area();
        if let Some(setup) = &mut self.setup {
            match setup.update(&mut self.ui, area) {
                Some(setup::Outcome::Created(path)) => {
                    self.setup = None;
                    self.open(Config::load(&path));
                }
                Some(setup::Outcome::Quit) => self.quit = true,
                None => {}
            }
            return;
        }
        match self.tab {
            Tab::Library => self.update_library(area),
            Tab::Network => self.update_network(area),
            Tab::Status => self.status.update(&self.ui, area, self.config.as_ref()),
            Tab::Settings => {
                let (Some(settings), Some(cfg)) = (&mut self.settings, &self.config) else {
                    return;
                };
                if let Some(saved) = settings.update(&mut self.ui, area, cfg) {
                    self.reload_config();
                    if saved.key == "FREEGAMES_PATH" {
                        self.reload_library();
                    }
                }
            }
        }
    }

    fn update_library(&mut self, area: Rect) {
        if let Some(editor) = &mut self.editor {
            match editor.update(&mut self.ui, area) {
                Some(editor::Event::Closed) => {
                    self.editor = None;
                    self.back_held_over = true;
                }
                Some(editor::Event::Saved) => {
                    if let Some(cfg) = &self.config {
                        self.library.load(cfg);
                    }
                }
                None => {}
            }
            return;
        }
        if self.ui.input.held_for(Action::Back).is_none() {
            self.back_held_over = false;
        }
        if self.quit_progress() >= 1.0 {
            self.quit = true;
        }
        let Some(event) = self.library.update(&self.ui, area, &self.jobs) else {
            return;
        };
        let games_dir = self.config.as_ref().and_then(Config::games_dir);
        match event {
            library::Event::Launched(session) => self.session = Some(session),
            library::Event::Edit(index) => {
                if let (Some(game), Some(cfg)) = (self.library.games.get(index), &self.config) {
                    self.editor = Some(Editor::new(game, cfg));
                }
            }
            library::Event::Unpack(index) => {
                let (Some(game), Some(dir)) = (self.library.games.get(index), games_dir) else {
                    return;
                };
                let Some(source) = game.package().map(std::path::Path::to_path_buf) else {
                    return;
                };
                self.jobs
                    .start(Kind::Unpack, &game.folder_name, &game.title, move |p| {
                        package::unpack(&source, &dir, p)
                    });
            }
            library::Event::Pack(index) => {
                let (Some(game), Some(dir)) = (self.library.games.get(index), games_dir) else {
                    return;
                };
                let game = game.clone();
                let target = package::package_path(&dir, &game.folder_name);
                let (id, title) = (game.folder_name.clone(), game.title.clone());
                self.jobs.start(Kind::Pack, &id, &title, move |p| {
                    package::pack(&game, &target, p).map(|_| target)
                });
            }
            library::Event::Cancel(id) => self.jobs.cancel(&id),
        }
    }

    fn update_network(&mut self, area: Rect) {
        let Some(cfg) = &self.config else { return };
        let Some(event) = self.network.update(&self.ui, area, cfg, &self.jobs) else {
            return;
        };
        match event {
            network::Event::Get {
                addr,
                game,
                title,
                kind,
            } => {
                let Some(dir) = cfg.games_dir() else { return };
                let id = game.clone();
                self.jobs.start(kind, &game, &title, move |p| match kind {
                    Kind::Download => client::download(addr, &id, &dir, p),
                    _ => client::install(addr, &id, &dir, p),
                });
            }
            network::Event::Cancel(id) => self.jobs.cancel(&id),
        }
    }

    /// How far along the hold-Back-to-quit gesture is (0..1).
    fn quit_progress(&self) -> f32 {
        if self.back_held_over || self.editor.is_some() {
            return 0.0;
        }
        self.ui
            .input
            .held_for(Action::Back)
            .map_or(0.0, |t| (t / HOLD_TO_QUIT_SECS).min(1.0) as f32)
    }

    // --- Drawing -------------------------------------------------------------

    fn draw(&self) {
        let area = self.content_area();
        // Screen content first; header/footer are drawn last with an opaque
        // background so they mask list rows scrolled past the edges.
        self.ui.canvas.begin();
        match (&self.setup, self.tab, &self.editor) {
            (Some(setup), _, _) => setup.draw(&self.ui, area),
            (None, Tab::Library, Some(editor)) => editor.draw(&self.ui, area),
            (None, Tab::Library, None) => self.library.draw(&self.ui, area, &self.jobs),
            (None, Tab::Network, _) => {
                self.network
                    .draw(&self.ui, area, self.config.as_ref(), &self.jobs)
            }
            (None, Tab::Status, _) => self.status.draw(&self.ui, area, self.library.games.len()),
            (None, Tab::Settings, _) => {
                if let Some(settings) = &self.settings {
                    settings.draw(&self.ui, area);
                }
            }
        }
        self.draw_header();
        self.draw_footer();
        self.draw_session();
        self.ui.canvas.end();
        self.ui.canvas.present();

        // Full-resolution artwork drawn over the upscaled canvas.
        if self.session.is_none() && self.setup.is_none() {
            match (self.tab, &self.editor) {
                (Tab::Library, Some(editor)) => editor.overlay(&self.ui, area),
                (Tab::Library, None) => self.library.overlay(&self.ui, area),
                (Tab::Network, _) => self.network.overlay(&self.ui, area),
                _ => {}
            }
        }
    }

    fn draw_header(&self) {
        let (w, _) = self.ui.canvas.size();
        draw_rectangle(0.0, 0.0, w, CONTENT_TOP, theme::BG);
        let tabs: Vec<(&str, bool)> = match &self.setup {
            Some(_) => vec![("SETUP", true)],
            None => TABS
                .iter()
                .map(|(tab, name)| (*name, *tab == self.tab))
                .collect(),
        };
        let mut x = MARGIN;
        for (name, active) in tabs {
            self.ui.text(
                name,
                x,
                23.0,
                if active { theme::ACCENT } else { theme::MUTED },
            );
            let width = self.ui.measure(name);
            if active {
                draw_rectangle(x, 27.0, width, 2.0, theme::ACCENT);
            }
            x += width + 18.0;
        }
        self.draw_indicators(x, w);
        draw_rectangle(0.0, HEADER_H, w, 1.0, theme::BORDER);
    }

    /// Right side of the header: the cartridge's name, then the pipeline run,
    /// transfers and sharing. Stops before running into the tabs (which end
    /// at `tabs_end`).
    fn draw_indicators(&self, tabs_end: f32, w: f32) {
        let mut x = w - MARGIN;
        let mut indicator = |label: &str, color: Color, busy: bool| {
            let spinner = if busy { 14.0 } else { 0.0 };
            if x - self.ui.measure(label) - spinner < tabs_end {
                x = tabs_end; // out of room: hide this one and the rest
                return;
            }
            x -= self.ui.measure(label);
            self.ui.text(label, x, 23.0, color);
            if busy {
                super::draw_spinner(x - 10.0, 18.0, 5.0, color);
                x -= 14.0;
            }
            x -= 14.0;
        };
        if let Some(cfg) = &self.config {
            indicator(&cfg.label(), theme::MUTED, false);
        }
        if self.status.is_running() {
            indicator("RUNNING", theme::AMBER, true);
        }
        match self.jobs.running() {
            [] => {}
            [job] => indicator(
                &format!(
                    "{} {:.0}%",
                    job.kind.verb().to_uppercase(),
                    job.fraction() * 100.0
                ),
                theme::AMBER,
                true,
            ),
            jobs => indicator(
                &format!("{} COPIES {:.0}%", jobs.len(), self.jobs.fraction() * 100.0),
                theme::AMBER,
                true,
            ),
        }
        if let Some(server) = &self.server {
            match server.sending() {
                0 => indicator("SHARING", theme::BLUE, false),
                n => indicator(&format!("SENDING {n}"), theme::BLUE, true),
            }
        }
    }

    /// Button hints for the current screen, labelled for the last-used device.
    fn hints(&self) -> Vec<Hint> {
        let ui = &self.ui;
        if let Some(setup) = &self.setup {
            return setup.hints(ui);
        }
        let mut hints = match (self.tab, &self.editor) {
            (Tab::Library, Some(editor)) => return editor.hints(ui),
            (Tab::Network, _) if self.network.has_modal() => {
                return self.network.hints(ui, &self.jobs)
            }
            (Tab::Network, _) => self.network.hints(ui, &self.jobs),
            (Tab::Library, None) => {
                let mut hints = self.library.hints(ui, &self.jobs);
                hints.push(Hint {
                    progress: self.quit_progress(),
                    ..ui.hint(Action::Back, "Hold: Quit", theme::AMBER)
                });
                hints
            }
            (Tab::Status, _) => self.status.hints(ui),
            (Tab::Settings, _) => match &self.settings {
                Some(settings) if settings.has_modal() => return settings.hints(ui),
                Some(settings) => settings.hints(ui),
                None => Vec::new(),
            },
        };
        hints.push(ui.hint(Action::TabNext, "Tabs", theme::BORDER));
        hints
    }

    fn draw_footer(&self) {
        let (w, h) = self.ui.canvas.size();
        let top = h - FOOTER_H;
        draw_rectangle(0.0, top, w, FOOTER_H, theme::BG);
        draw_rectangle(0.0, top, w, 1.0, theme::BORDER);
        let mut x = MARGIN;
        for hint in self.hints() {
            let key_w = self.ui.measure(hint.key).ceil() + 8.0;
            let label_w = self.ui.measure(&hint.label).ceil();
            if x + key_w + 4.0 + label_w > w - MARGIN {
                break; // out of room (narrow / square layouts)
            }
            draw_rectangle(x, top + 2.0, key_w, 18.0, hint.color);
            self.ui.text(hint.key, x + 4.0, top + 17.0, theme::BG);
            x += key_w + 4.0;
            self.ui.text(&hint.label, x, top + 17.0, theme::MUTED);
            if hint.progress > 0.0 {
                draw_rectangle(
                    x,
                    top + 19.0,
                    (label_w * hint.progress).round(),
                    2.0,
                    hint.color,
                );
            }
            x += label_w + 14.0;
        }
    }

    /// Full-screen "now playing" card while a game runs.
    fn draw_session(&self) {
        let Some(session) = &self.session else { return };
        let ui = &self.ui;
        let (w, h) = ui.canvas.size();
        draw_rectangle(0.0, 0.0, w, h, theme::BG);
        let title = ui.truncate_heading(&session.title, w - 2.0 * MARGIN);
        ui.text(
            "NOW PLAYING",
            (w - ui.measure("NOW PLAYING")) / 2.0,
            h / 2.0 - 36.0,
            theme::MUTED,
        );
        ui.heading(
            &title,
            (w - ui.measure_heading(&title)) / 2.0,
            h / 2.0 + 8.0,
            theme::ACCENT,
        );
        let hint = ui.truncate("Controls return when the game exits", w - 2.0 * MARGIN);
        ui.text(
            &hint,
            (w - ui.measure(&hint)) / 2.0,
            h / 2.0 + 40.0,
            theme::MUTED,
        );
    }
}
