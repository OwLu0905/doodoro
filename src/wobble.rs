// Hand-drawn outlines, ported from the gpuix-svelte version's wobble.ts with the same noise, so the
// shapes come out the same. Each shape is sampled into a polyline and every point is pushed by a
// smooth 2D noise field, standing in for the design's feTurbulence → feDisplacementMap.

use std::f64::consts::PI;

pub type Pt = [f64; 2];

const FREQ: f64 = 0.018;
const SCALE: f64 = 6.0;
const STEP: f64 = 3.0;

/// JavaScript's `| 0`, `>>>` and `Math.imul`, done on the same 32 bits.
fn hash(x: f64, y: f64, seed: f64) -> f64 {
    let h = ((x * 374761393.0 + y * 668265263.0 + seed * 2147483647.0) as i64) as u32;
    let h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    (h ^ (h >> 16)) as f64 / 4294967295.0
}

fn value_noise(x: f64, y: f64, seed: f64) -> f64 {
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let sx = fx * fx * (3.0 - 2.0 * fx);
    let sy = fy * fy * (3.0 - 2.0 * fy);
    let a = hash(xi, yi, seed);
    let b = hash(xi + 1.0, yi, seed);
    let c = hash(xi, yi + 1.0, seed);
    let d = hash(xi + 1.0, yi + 1.0, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// Two octaves, like `numOctaves=2`, centred on 0.5.
fn fractal(x: f64, y: f64, seed: f64) -> f64 {
    (value_noise(x, y, seed) * 2.0 + value_noise(x * 2.0, y * 2.0, seed + 7.0)) / 3.0
}

/// `scale` is the filter's displacement scale: points move by up to half of it.
pub fn wobble(points: &[Pt], scale: Option<f64>) -> Vec<Pt> {
    let scale = scale.unwrap_or(SCALE);
    let seed = 4.0;
    points
        .iter()
        .map(|&[x, y]| {
            [
                x + scale * (fractal(x * FREQ, y * FREQ, seed) - 0.5),
                y + scale * (fractal(x * FREQ, y * FREQ, seed + 101.0) - 0.5),
            ]
        })
        .collect()
}

fn steps(length: f64) -> usize {
    ((length / STEP).ceil() as usize).max(2)
}

pub fn line(a: Pt, b: Pt) -> Vec<Pt> {
    let n = steps((b[0] - a[0]).hypot(b[1] - a[1]));
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
        })
        .collect()
}

pub fn quad(a: Pt, c: Pt, b: Pt) -> Vec<Pt> {
    let n = steps((c[0] - a[0]).hypot(c[1] - a[1]) + (b[0] - c[0]).hypot(b[1] - c[1]));
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * c[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * c[1] + t * t * b[1],
            ]
        })
        .collect()
}

/// An arc of a circle, angles in radians, clockwise on screen.
pub fn arc(cx: f64, cy: f64, r: f64, from: f64, to: f64) -> Vec<Pt> {
    let n = steps((to - from).abs() * r);
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f64 / n as f64;
            [cx + r * a.cos(), cy + r * a.sin()]
        })
        .collect()
}

/// A rounded rectangle traced clockwise from the top-left corner's end.
pub fn rounded_rect(x: f64, y: f64, w: f64, h: f64, r: f64) -> Vec<Pt> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let q = PI / 2.0;
    [
        line([x + r, y], [x + w - r, y]),
        arc(x + w - r, y + r, r, -q, 0.0),
        line([x + w, y + r], [x + w, y + h - r]),
        arc(x + w - r, y + h - r, r, 0.0, q),
        line([x + w - r, y + h], [x + r, y + h]),
        arc(x + r, y + h - r, r, q, 2.0 * q),
        line([x, y + h - r], [x, y + r]),
        arc(x + r, y + r, r, 2.0 * q, 3.0 * q),
    ]
    .concat()
}

/// Diagonal hatching ("/") across a w × h box, `gap` apart, as separate open strokes.
pub fn hatch(w: f64, h: f64, gap: f64) -> Vec<Vec<Pt>> {
    let mut lines = Vec::new();
    let mut k = gap / 2.0;
    while k < w + h {
        let a = [(k - h).max(0.0), k.min(h)];
        let b = [k.min(w), (k - w).max(0.0)];
        if (b[0] - a[0]).hypot(b[1] - a[1]) > gap / 2.0 {
            lines.push(line(a, b));
        }
        k += gap;
    }
    lines
}
