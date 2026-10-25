//! Rectangles blurred by a Gaussian, the way CSS `box-shadow` and Chromium draw them.
//!
//! A [`crate::RectShape`] with a `blur_width` and nothing but a fill (a shadow or a glow)
//! is drawn as a rounded rectangle convolved with a Gaussian of standard deviation
//! `blur_width / 2`, as CSS turns a blur radius into a standard deviation. The edge fades
//! over about three standard deviations on either side of the rectangle's outline, with
//! half the colour on the outline itself.
//!
//! The mesh is a set of rings that follow the rounded rectangle, each vertex coloured with
//! the blurred coverage at its position (computed as in Evan Wallace's "Fast Rounded
//! Rectangle Shadows": the exact blur across, integrated numerically along), so corners
//! and boxes smaller than the blur fade as Chromium's do.

use std::sync::Arc;

use emath::{Rect, Vec2, vec2};

use crate::{Color32, CornerRadiusF32, Mesh, Vertex, WHITE_UV};

/// A blurred rectangle's mesh around the rectangle's centre: where each vertex goes and
/// how much of the colour it carries.
#[derive(Clone, Debug, Default)]
pub struct BlurredRectMesh {
    positions: Vec<Vec2>,
    coverage: Vec<f32>,
    indices: Vec<u32>,
}

impl BlurredRectMesh {
    /// The mesh of a rectangle of `size` with `corner_radius`, blurred with standard
    /// deviation `sigma` (all in points).
    pub fn new(size: Vec2, corner_radius: CornerRadiusF32, sigma: f32) -> Self {
        let half = (size * 0.5).max(Vec2::ZERO);
        let sigma = sigma.max(0.01);
        let max_radius = half.min_elem();
        let radii = [
            corner_radius.nw,
            corner_radius.ne,
            corner_radius.se,
            corner_radius.sw,
        ]
        .map(|r| r.clamp(0.0, max_radius));

        // Rings at these distances (in standard deviations) from the outline: closer
        // together where the fade curves most, none deeper inside than the rectangle goes.
        let deepest = -(3.0 * sigma).min(max_radius_inset(half));
        let mut offsets: Vec<f32> = [-3.0, -2.0, -1.5, -1.0, -0.5, 0.0, 0.5, 1.0, 1.5, 2.0, 3.0]
            .iter()
            .map(|k| k * sigma)
            .filter(|d| *d > deepest + 0.01)
            .collect();
        offsets.insert(0, deepest);

        // Points per corner arc: enough for the widest ring's corner.
        let widest = radii.iter().fold(0.0f32, |a, r| a.max(*r)) + 3.0 * sigma;
        let segments = ((widest / 6.0).ceil() as usize).clamp(2, 6);
        // Along each straight side, points this far from either end: the blur changes along
        // a side within a few standard deviations of a corner, and not beyond.
        let along = [0.5 * sigma, 1.25 * sigma, 2.25 * sigma];
        let per_ring = 4 * (segments + 1 + 2 * along.len());

        let mut mesh = Self {
            positions: Vec::with_capacity(offsets.len() * per_ring + 1),
            coverage: Vec::new(),
            indices: Vec::with_capacity((offsets.len() * per_ring) * 6),
        };
        // The corners, clockwise from the top left, as (sign x, sign y, start angle).
        let corners = [
            (-1.0, -1.0, core::f32::consts::PI),
            (1.0, -1.0, 1.5 * core::f32::consts::PI),
            (1.0, 1.0, 0.0),
            (-1.0, 1.0, 0.5 * core::f32::consts::PI),
        ];
        let mut arcs: [Vec<Vec2>; 4] = Default::default();
        for &d in &offsets {
            for (corner, &(sx, sy, start)) in corners.iter().enumerate() {
                let r = radii[corner];
                let rho = (r + d).max(0.0);
                // The arc's centre; a ring inside a sharp-enough corner is sharp itself.
                let center = vec2(sx * (half.x + d - rho), sy * (half.y + d - rho));
                arcs[corner].clear();
                arcs[corner].extend((0..=segments).map(|i| {
                    let angle = start + 0.5 * core::f32::consts::PI * i as f32 / segments as f32;
                    center + rho * vec2(angle.cos(), angle.sin())
                }));
            }
            for corner in 0..4 {
                mesh.positions.extend_from_slice(&arcs[corner]);
                // The side to the next corner.
                let from = *arcs[corner].last().unwrap_or(&Vec2::ZERO);
                let to = *arcs[(corner + 1) % 4].first().unwrap_or(&Vec2::ZERO);
                let length = (to - from).length();
                let at = |distance: f32| {
                    if length > 0.0 {
                        from + (to - from) * (distance / length)
                    } else {
                        from
                    }
                };
                let near = |s: f32| s.min(0.5 * length);
                mesh.positions.extend(along.iter().map(|&s| at(near(s))));
                mesh.positions
                    .extend(along.iter().rev().map(|&s| at(length - near(s))));
            }
        }
        // The innermost ring is filled from the centre.
        let center_index = mesh.positions.len() as u32;
        mesh.positions.push(Vec2::ZERO);

        let per_ring = per_ring as u32;
        for ring in 0..offsets.len() as u32 - 1 {
            for i in 0..per_ring {
                let next = (i + 1) % per_ring;
                let (a, b) = (ring * per_ring + i, ring * per_ring + next);
                let (c, d) = (a + per_ring, b + per_ring);
                mesh.indices.extend_from_slice(&[a, b, d, a, d, c]);
            }
        }
        for i in 0..per_ring {
            mesh.indices
                .extend_from_slice(&[center_index, i, (i + 1) % per_ring]);
        }

        mesh.coverage = mesh
            .positions
            .iter()
            .map(|&p| coverage(p, half, radii, sigma))
            .collect();
        mesh
    }

    /// Appends the mesh at `center` in `color`.
    pub fn append_to(&self, center: emath::Pos2, color: Color32, out: &mut Mesh) {
        let base = out.vertices.len() as u32;
        out.vertices.reserve(self.positions.len());
        out.vertices.extend(
            self.positions
                .iter()
                .zip(&self.coverage)
                .map(|(p, c)| Vertex {
                    pos: center + *p,
                    uv: WHITE_UV,
                    color: color.gamma_multiply(*c),
                }),
        );
        out.indices.reserve(self.indices.len());
        out.indices.extend(self.indices.iter().map(|i| base + i));
    }

    /// The vertex positions, around the rectangle's centre.
    pub fn positions(&self) -> &[Vec2] {
        &self.positions
    }

    /// How much of the colour each vertex carries.
    pub fn coverage(&self) -> &[f32] {
        &self.coverage
    }

    /// The triangles, three indices each.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
}

/// How far the rings may go inside: to the middle of the rectangle's shorter side.
fn max_radius_inset(half: Vec2) -> f32 {
    half.min_elem()
}

/// Draws `rect` with `corner_radius` blurred with standard deviation `blur_width / 2` in
/// `color`, as CSS `box-shadow` does with a blur radius of `blur_width`.
pub fn tessellate_blurred_rect(
    rect: Rect,
    corner_radius: CornerRadiusF32,
    color: Color32,
    blur_width: f32,
    out: &mut Mesh,
) {
    BlurredRectMesh::new(rect.size(), corner_radius, blur_width / 2.0).append_to(
        rect.center(),
        color,
        out,
    );
}

/// Blurred meshes by their rectangle's size, corner radii and blur, kept while they are
/// drawn: shadows mostly stay the same size from frame to frame.
#[derive(Clone, Default)]
pub(crate) struct BlurredRectCache {
    meshes: ahash::HashMap<[u32; 7], (u64, Arc<BlurredRectMesh>)>,
    frame: u64,
}

impl BlurredRectCache {
    pub(crate) fn get(
        &mut self,
        size: Vec2,
        corner_radius: CornerRadiusF32,
        sigma: f32,
    ) -> Arc<BlurredRectMesh> {
        let key = [
            size.x.to_bits(),
            size.y.to_bits(),
            corner_radius.nw.to_bits(),
            corner_radius.ne.to_bits(),
            corner_radius.sw.to_bits(),
            corner_radius.se.to_bits(),
            sigma.to_bits(),
        ];
        let frame = self.frame;
        let entry = self.meshes.entry(key).or_insert_with(|| {
            (
                frame,
                Arc::new(BlurredRectMesh::new(size, corner_radius, sigma)),
            )
        });
        entry.0 = frame;
        Arc::clone(&entry.1)
    }

    /// Call once a frame: forgets the meshes no frame drew for a while.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        if self.frame.is_multiple_of(60) || 512 < self.meshes.len() {
            let frame = self.frame;
            self.meshes.retain(|_, (used, _)| frame <= *used + 60);
        }
    }
}

/// The blurred coverage at `p` (from the rectangle's centre) of a rectangle with
/// half size `half` and corner radii `radii` (nw, ne, se, sw), blurred with `sigma`.
fn coverage(p: Vec2, half: Vec2, radii: [f32; 4], sigma: f32) -> f32 {
    let radius = match (p.x < 0.0, p.y < 0.0) {
        (true, true) => radii[0],
        (false, true) => radii[1],
        (false, false) => radii[2],
        (true, false) => radii[3],
    };
    // Integrate the exact blur across along a vertical stretch of about three standard
    // deviations on either side, where the rectangle is.
    let low = p.y - half.y;
    let high = p.y + half.y;
    let start = (-3.0 * sigma).clamp(low, high);
    let end = (3.0 * sigma).clamp(low, high);
    const SAMPLES: usize = 8;
    let step = (end - start) / SAMPLES as f32;
    let mut y = start + 0.5 * step;
    let mut value = 0.0;
    for _ in 0..SAMPLES {
        value += across(p.x, p.y - y, sigma, radius, half) * gaussian(y, sigma) * step;
        y += step;
    }
    value.clamp(0.0, 1.0)
}

/// The blur across the rectangle's row at height `y` (from its centre), at `x`.
fn across(x: f32, y: f32, sigma: f32, radius: f32, half: Vec2) -> f32 {
    let delta = (half.y - radius - y.abs()).min(0.0);
    let curved = half.x - radius + (radius * radius - delta * delta).max(0.0).sqrt();
    let scale = core::f32::consts::FRAC_1_SQRT_2 / sigma;
    0.5 * (erf((x + curved) * scale) - erf((x - curved) * scale))
}

fn gaussian(x: f32, sigma: f32) -> f32 {
    (-(x * x) / (2.0 * sigma * sigma)).exp() / ((2.0 * core::f32::consts::PI).sqrt() * sigma)
}

/// The error function (Abramowitz and Stegun 7.1.26, within 1.5e-7).
fn erf(x: f32) -> f32 {
    let t = 1.0 / (1.0 + 0.327_591_1 * x.abs());
    let poly = t
        * (0.254_829_6
            + t * (-0.284_496_74 + t * (1.421_413_7 + t * (-1.453_152 + t * 1.061_405_4))));
    let y = 1.0 - poly * (-x * x).exp();
    y.copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erf_is_close() {
        for (x, expected) in [
            (0.0, 0.0),
            (0.5, 0.520_499_9),
            (1.0, 0.842_700_8),
            (2.0, 0.995_322_3),
        ] {
            assert!((erf(x) - expected).abs() < 1e-5, "{x}");
            assert!((erf(-x) + expected).abs() < 1e-5, "{x}");
        }
    }

    #[test]
    fn an_edge_fades_as_a_gaussian_with_half_the_colour_on_the_outline() {
        // A long sharp box: across its middle the blur is the error function.
        let half = vec2(500.0, 50.0);
        let sigma = 8.0;
        for (d, expected) in [
            (0.0, 0.5),
            (8.0, 0.158_655),
            (16.0, 0.022_75),
            (-8.0, 0.841_345),
        ] {
            let c = coverage(vec2(0.0, 50.0 + d), half, [0.0; 4], sigma);
            assert!((c - expected).abs() < 0.01, "{d}: {c}");
        }
    }

    #[test]
    fn a_box_smaller_than_its_blur_is_fainter() {
        let sigma = 20.0;
        let small = coverage(Vec2::ZERO, vec2(10.0, 10.0), [0.0; 4], sigma);
        // Two errors functions' worth: about 0.38 across and down.
        assert!((small - 0.147).abs() < 0.02, "{small}");
    }

    #[test]
    fn the_mesh_covers_three_standard_deviations_round_the_box() {
        let mesh = BlurredRectMesh::new(vec2(100.0, 60.0), CornerRadiusF32::same(8.0), 6.0);
        let extent = mesh
            .positions()
            .iter()
            .fold(Vec2::ZERO, |m, p| m.max(p.abs()));
        assert!((extent.x - (50.0 + 18.0)).abs() < 0.01, "{extent:?}");
        assert!((extent.y - (30.0 + 18.0)).abs() < 0.01, "{extent:?}");
        assert!(mesh.indices().len().is_multiple_of(3));
        assert!(
            mesh.indices()
                .iter()
                .all(|&i| (i as usize) < mesh.positions().len())
        );
        // Nearly nothing at the outer ring, nearly everything in the middle.
        let outer = mesh.coverage()[mesh.coverage().len() - 2];
        assert!(outer < 0.01, "{outer}");
        assert!(*mesh.coverage().last().unwrap() > 0.99);
    }
}
