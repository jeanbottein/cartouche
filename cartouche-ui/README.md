# cartouche-ui

A retro, game-like frontend for Cartouche — Rust + [macroquad](https://github.com/not-fl3/macroquad).
See [`BACKLOG.md`](./BACKLOG.md) for the full plan.

## Status: M0 (walking skeleton)

- Fullscreen; draws to a fixed **432 px tall** canvas whose width follows the screen shape
  (4:3 → 576×432, 16:9 → 768×432), upscaled nearest-neighbor — the UI looks the same at any
  resolution (exact ×5 at 4K). Screens under 432 px tall draw 1:1. Tune `DESIGN_HEIGHT` in
  `src/engine/canvas.rs` to make the whole UI bigger (lower) or smaller (higher).
  **Pure black** background
- VT323 pixel font rendered only at its native 25 px grid (×1 body, ×2 headings), hard-edged
- Footer bar with button hints for the current screen, shown for the last-used device
  (gamepad A/B/LB RB or keyboard Enter/Esc/Q E)
- Aspect modes: **Wide** (fills the screen, default) and **Square** (centered, reflows content vertically)
- Unified input layer: gamepad (gilrs) + keyboard + mouse → `Action`s, with d-pad repeat
- 60fps cap (will become configurable in Settings)

This milestone renders a demo grid + cursor as a stand-in for the real Library screen.

### Controls

| Action | Keyboard | Gamepad | Mouse |
|---|---|---|---|
| Move | Arrows / WASD | D-pad / Left stick | Hover |
| Confirm (Library: launch game) | Enter / Space | A (South) | Left click (click the focused row to launch) |
| Back (Library: **hold 1 s to quit**) | Esc / Backspace | B (East) | — |
| Toggle aspect | Tab | Start / Select | — |

## Build & run

Requires the Rust toolchain (`rustup`).

```bash
cd cartouche-ui
cargo run            # debug
cargo run --release  # optimized
```

**Linux build deps:** `gilrs` needs `libudev` and macroquad needs X11/OpenGL at
runtime. On Debian/Ubuntu/CI: `sudo apt-get install -y libudev-dev libx11-dev libgl1-mesa-dev`.
On SteamOS these are typically already present at runtime.

**Steam Game Mode:** gamescope gives the app the resolution set in the shortcut's
*Properties → Game Resolution*. Set it to **Native**, otherwise Steam renders at a lower
resolution and upscales it with its own filter, softening the pixels.

## Fonts

`assets/VT323-Regular.ttf` — VT323 by The VT323 Project Authors, licensed under the
SIL Open Font License v1.1 (see `assets/VT323-OFL.txt`). Embedded into the binary.

## Layout

```
src/
  main.rs            entry + M0 demo screen
  engine/
    canvas.rs        virtual canvas, scaling, aspect modes (ENGINE-1/2)
    input.rs         unified gamepad/keyboard/mouse action layer (ENGINE-3)
    theme.rs         retro palette, pure-black background (ENGINE-6)
```
