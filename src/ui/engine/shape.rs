//! Rounded rectangles (filled or outlined), vertical gradients and soft
//! glows, drawn as meshes. Curved edges get a one-screen-pixel feather, so
//! they stay smooth without multisampling (the canvas is a render target).
//! A radius of 0 draws plain, crisp rectangles.

use std::cell::Cell;
use std::f32::consts::{FRAC_PI_2, PI};

use macroquad::models::{draw_mesh, Mesh, Vertex};
use macroquad::prelude::*;

thread_local! {
    /// One screen pixel, in design pixels.
    static PIXEL: Cell<f32> = const { Cell::new(1.0) };
}

const CORNER_SEGMENTS: usize = 8;

/// Tell the shapes how many screen pixels make a design pixel (the canvas
/// does, each frame), for the width of their feathered edges.
pub fn set_scale(scale: f32) {
    PIXEL.with(|p| p.set(1.0 / scale.max(1.0)));
}

fn pixel() -> f32 {
    PIXEL.with(Cell::get)
}

fn clear(color: Color) -> Color {
    Color { a: 0.0, ..color }
}

/// `r` grown by `d` on every side (shrunk when negative).
fn grow(r: Rect, d: f32) -> Rect {
    Rect::new(
        r.x - d,
        r.y - d,
        (r.w + 2.0 * d).max(0.0),
        (r.h + 2.0 * d).max(0.0),
    )
}

/// Points around `r` with `radius` corners, clockwise from the left end of
/// the top-left corner. Always the same count, so outlines pair up in rings.
fn outline(r: Rect, radius: f32) -> Vec<Vec2> {
    let radius = radius.clamp(0.0, r.w.min(r.h) / 2.0);
    let corners = [
        (vec2(r.x + radius, r.y + radius), PI),
        (vec2(r.right() - radius, r.y + radius), PI + FRAC_PI_2),
        (vec2(r.right() - radius, r.bottom() - radius), 0.0),
        (vec2(r.x + radius, r.bottom() - radius), FRAC_PI_2),
    ];
    corners
        .iter()
        .flat_map(|&(center, start)| {
            (0..=CORNER_SEGMENTS).map(move |i| {
                let angle = start + FRAC_PI_2 * i as f32 / CORNER_SEGMENTS as f32;
                center + vec2(angle.cos(), angle.sin()) * radius
            })
        })
        .collect()
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

impl MeshBuilder {
    fn vertex(&mut self, p: Vec2, color: Color) -> u16 {
        self.vertices
            .push(Vertex::new(p.x, p.y, 0.0, 0.0, 0.0, color));
        (self.vertices.len() - 1) as u16
    }

    /// Triangles from `center` to every edge of the closed `points`.
    fn fan(&mut self, center: Vec2, points: &[Vec2], color: Color) {
        let c = self.vertex(center, color);
        let first = self.vertices.len() as u16;
        points.iter().for_each(|&p| {
            self.vertex(p, color);
        });
        let n = points.len() as u16;
        for i in 0..n {
            self.indices.extend([c, first + i, first + (i + 1) % n]);
        }
    }

    /// A band between two closed outlines of the same length, colors blending across it.
    fn ring(&mut self, outer: &[Vec2], outer_color: Color, inner: &[Vec2], inner_color: Color) {
        let o = self.vertices.len() as u16;
        outer.iter().for_each(|&p| {
            self.vertex(p, outer_color);
        });
        let i0 = self.vertices.len() as u16;
        inner.iter().for_each(|&p| {
            self.vertex(p, inner_color);
        });
        let n = outer.len() as u16;
        for k in 0..n {
            let next = (k + 1) % n;
            self.indices
                .extend([o + k, o + next, i0 + k, i0 + k, o + next, i0 + next]);
        }
    }

    fn draw(self) {
        draw_mesh(&Mesh {
            vertices: self.vertices,
            indices: self.indices,
            texture: None,
        });
    }
}

/// A filled rectangle with rounded corners.
pub fn fill_rounded(r: Rect, radius: f32, color: Color) {
    if radius <= 0.0 {
        draw_rectangle(r.x, r.y, r.w, r.h, color);
        return;
    }
    let half = pixel() / 2.0;
    let inner = outline(grow(r, -half), radius - half);
    let mut mesh = MeshBuilder::default();
    mesh.fan(r.center(), &inner, color);
    mesh.ring(
        &outline(grow(r, half), radius + half),
        clear(color),
        &inner,
        color,
    );
    mesh.draw();
}

/// A `width`-thick outline inside the edge of `r`, with rounded corners.
pub fn stroke_rounded(r: Rect, radius: f32, width: f32, color: Color) {
    if radius <= 0.0 {
        stroke_rect(r, width, color);
        return;
    }
    let half = pixel() / 2.0;
    let edge = |d: f32| outline(grow(r, d), radius + d);
    let (outer, inner) = (edge(-half), edge(half - width));
    let mut mesh = MeshBuilder::default();
    mesh.ring(&edge(half), clear(color), &outer, color);
    mesh.ring(&outer, color, &inner, color);
    mesh.ring(&inner, color, &edge(-half - width), clear(color));
    mesh.draw();
}

/// A crisp `width`-thick square border as four filled rects.
pub fn stroke_rect(r: Rect, width: f32, color: Color) {
    draw_rectangle(r.x, r.y, r.w, width, color);
    draw_rectangle(r.x, r.y + r.h - width, r.w, width, color);
    draw_rectangle(r.x, r.y, width, r.h, color);
    draw_rectangle(r.x + r.w - width, r.y, width, r.h, color);
}

/// A halo around `r` fading out over `spread` (glows, shadows). Leaves the
/// inside of `r` untouched.
pub fn glow(r: Rect, radius: f32, spread: f32, color: Color) {
    let mut mesh = MeshBuilder::default();
    mesh.ring(
        &outline(grow(r, spread), radius + spread),
        clear(color),
        &outline(r, radius),
        color,
    );
    mesh.draw();
}

/// `r` filled with a vertical gradient.
pub fn gradient(r: Rect, top: Color, bottom: Color) {
    let mut mesh = MeshBuilder::default();
    let corners = [
        (vec2(r.x, r.y), top),
        (vec2(r.right(), r.y), top),
        (vec2(r.right(), r.bottom()), bottom),
        (vec2(r.x, r.bottom()), bottom),
    ];
    corners.iter().for_each(|&(p, c)| {
        mesh.vertex(p, c);
    });
    mesh.indices.extend([0, 1, 2, 0, 2, 3]);
    mesh.draw();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outlines_have_the_same_length_whatever_the_radius() {
        let r = Rect::new(0.0, 0.0, 40.0, 20.0);
        assert_eq!(outline(r, 0.0).len(), outline(r, 6.0).len());
    }

    #[test]
    fn outline_radius_is_capped_at_half_the_short_side() {
        let points = outline(Rect::new(0.0, 0.0, 40.0, 10.0), 50.0);
        assert!(points.iter().all(|p| p.y >= -1e-4 && p.y <= 10.0 + 1e-4));
        assert!(points.iter().all(|p| p.x >= -1e-4 && p.x <= 40.0 + 1e-4));
    }

    #[test]
    fn outline_starts_at_the_left_edge_and_turns_clockwise() {
        let points = outline(Rect::new(0.0, 0.0, 40.0, 20.0), 5.0);
        assert!((points[0] - vec2(0.0, 5.0)).length() < 1e-4);
        // End of the top-left corner: on the top edge.
        assert!((points[CORNER_SEGMENTS] - vec2(5.0, 0.0)).length() < 1e-4);
    }
}
