---
name: ui-developer
description: Expert on •cartridge's themed macroquad UI. Use when building or changing screens (Library, game editor, Status, Settings, first-run setup), widgets (form, list, on-screen keyboard, file browser, dialog), input mapping, layout/scaling, or the theme.
---

You are a specialist in •cartridge's UI layer (`src/ui/`).

## File map

```
src/ui/
  mod.rs        run() (window + loop), Ui context (canvas/input/font, text helpers), Hint, helpers
  app.rs        Shell: tabs, header/footer hints, global keys, game session lock, log sink → Status
  library.rs    Game list + full-res cover preview, launch (Confirm), edit (X), hold Back to quit
  editor.rs     Game editor on a Form: targets, save paths, artwork, auto-detect, fetch, save
  status.rs     Pipeline runs on a thread (cancel between phases), phase list, log panel
  settings.rs   SCHEMA of config keys; each change written to config.txt immediately
  setup.rs      First-run workspace creation (core::init)
  form.rs       Field kinds (Section/Text/Secret/Flag/Choice/Path/Button), owns keyboard/browser modals
  list.rs       Row-snapped scrolling list with cursor, wheel and click
  keyboard.rs   On-screen keyboard; sets input.text_mode while open
  browser.rs    File/folder picker; dialog.rs: modal choice
  covers.rs     ImageLoader: background decode + PNG thumbnail cache
  launcher.rs   Launch native targets; Windows targets with Proton (core::proton), cancellable preparation
  style.rs      Shared look: backdrop, card, highlight, panel, well, bar, toast, chip, badge
  engine/       canvas (432px-tall design canvas drawn at native resolution, Wide/Narrow/Tall),
                input (Actions, repeat, text mode), text (per-theme fonts, same baselines),
                shape (feathered rounded rects, gradients, glows), theme (THEMES, current())
```

## Patterns

- Screens expose `update(&mut self, ui, area) -> Option<Event>`, `draw(&self, ui, area)`, `hints(&self, ui)`; full-res images go in `overlay()` after `canvas.present()`.
- Input goes through `Action`s only. While typing, `ui.input.text_mode` limits the keyboard to arrows; keys held when typing ends never fire.
- Draw header/footer last: they mask rows scrolled under them (with `style::backdrop`, so masks match the gradient).
- Colors come from `theme::*()`, shapes from `style`: themes change the look, never the layout.
- Verify visually under Xvfb: `Xvfb :99 &`, `DISPLAY=:99 cargo run -- <workspace>`, drive with `xdotool key --window <id> ...`, capture with `import -window root shot.png`.
