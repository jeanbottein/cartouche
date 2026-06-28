# cartouche-ui

A retro, game-like frontend for Cartouche — Rust + [macroquad](https://github.com/not-fl3/macroquad).
See [`BACKLOG.md`](./BACKLOG.md) for the full plan.

## Status: M0 (walking skeleton)

- 480p virtual canvas, nearest-neighbor scaling (fit or integer), **pure black** background
- VT323 pixel font, nearest-filtered for crisp retro text
- Aspect modes: **Wide 854×480** (default) and **Square 480×480** (reflows content vertically)
- Unified input layer: gamepad (gilrs) + keyboard + mouse → `Action`s, with d-pad repeat
- 60fps cap (will become configurable in Settings)

This milestone renders a demo grid + cursor as a stand-in for the real Library screen.

### Controls

| Action | Keyboard | Gamepad | Mouse |
|---|---|---|---|
| Move | Arrows / WASD | D-pad / Left stick | Hover |
| Confirm | Enter / Space | A (South) | Left click |
| Back | Esc / Backspace | B (East) | — |
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
