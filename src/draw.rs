// What Shape.svelte and the icon SVGs were in the gpuix-svelte version, painted as GPUI paths
// instead: no SVG is rasterised, and the ring's arc is re-tessellated rather than re-rendered.

use std::rc::Rc;

use gpui::{
    App, Bounds, Hsla, IntoElement, PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Styled,
    Window, canvas, point, px,
};
use lyon::tessellation::{LineCap, LineJoin};

use crate::wobble::Pt;

/// A polyline, from wobble.rs.
pub struct Poly {
    pub points: Vec<Pt>,
    pub closed: bool,
}

impl Poly {
    pub fn new(points: Vec<Pt>, closed: bool) -> Rc<Self> {
        Rc::new(Self { points, closed })
    }
}

pub enum Op {
    Fill(Rc<Poly>),
    Stroke(Rc<Poly>, f32),
}

pub struct Layer {
    pub ops: Vec<Op>,
    pub color: Hsla,
}

fn stroke_builder(width: f32) -> PathBuilder {
    PathBuilder::stroke(px(width)).with_style(PathStyle::Stroke(
        StrokeOptions::default()
            .with_line_width(width)
            .with_line_cap(LineCap::Round)
            .with_line_join(LineJoin::Round),
    ))
}

/// Stacks layers over a `width` × `height` box; `view_width` is the drawing's own coordinate width,
/// scaled to `width`. Paths may reach outside the box.
pub fn shape(width: f32, height: f32, view_width: f32, layers: Vec<Layer>) -> impl IntoElement + Styled {
    let k = width / view_width;
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let at = |[x, y]: Pt| point(bounds.origin.x + px(x as f32 * k), bounds.origin.y + px(y as f32 * k));
            for layer in &layers {
                for op in &layer.ops {
                    let (poly, mut builder) = match op {
                        Op::Fill(poly) => (poly, PathBuilder::fill()),
                        Op::Stroke(poly, width) => (poly, stroke_builder(width * k)),
                    };
                    let Some((&first, rest)) = poly.points.split_first() else { continue };
                    builder.move_to(at(first));
                    for &p in rest {
                        builder.line_to(at(p));
                    }
                    if poly.closed {
                        builder.close();
                    }
                    if let Ok(path) = builder.build() {
                        window.paint_path(path, layer.color);
                    }
                }
            }
        },
    )
    .absolute()
    .left_0()
    .top_0()
    .w(px(width))
    .h(px(height))
}

/// The icons' SVG, as the absolute path commands they use.
pub enum Seg {
    M(f32, f32),
    L(f32, f32),
    Q(f32, f32, f32, f32),
    /// rx ry large-arc sweep x y
    A(f32, f32, bool, bool, f32, f32),
    Z,
    /// A stroked circle: cx cy r.
    Circle(f32, f32, f32),
    /// A filled dot: cx cy r.
    Dot(f32, f32, f32),
}

/// Paints `segs`, drawn in a `view` × `view`-ish box, into `width` × `height`, stroked at
/// `stroke` (in view units) with round ends, like the SVGs' `stroke-linecap="round"`.
pub fn icon(segs: &'static [Seg], view: f32, stroke: f32, width: f32, height: f32, color: Hsla) -> impl IntoElement + Styled {
    let k = width / view;
    canvas(
        |_, _, _| {},
        move |bounds: Bounds<Pixels>, _, window: &mut Window, _: &mut App| {
            let at = |x: f32, y: f32| -> Point<Pixels> { point(bounds.origin.x + px(x * k), bounds.origin.y + px(y * k)) };
            let mut path = stroke_builder(stroke * k);
            let circle = |builder: &mut PathBuilder, cx: f32, cy: f32, r: f32| {
                let radii = point(px(r * k), px(r * k));
                builder.move_to(at(cx + r, cy));
                builder.arc_to(radii, px(0.), false, true, at(cx - r, cy));
                builder.arc_to(radii, px(0.), false, true, at(cx + r, cy));
                builder.close();
            };
            let mut dots = PathBuilder::fill();
            let mut has_dots = false;
            for seg in segs {
                match *seg {
                    Seg::M(x, y) => path.move_to(at(x, y)),
                    Seg::L(x, y) => path.line_to(at(x, y)),
                    Seg::Q(cx, cy, x, y) => path.curve_to(at(x, y), at(cx, cy)),
                    Seg::A(rx, ry, large, sweep, x, y) => {
                        path.arc_to(point(px(rx * k), px(ry * k)), px(0.), large, sweep, at(x, y))
                    }
                    Seg::Z => path.close(),
                    Seg::Circle(cx, cy, r) => circle(&mut path, cx, cy, r),
                    Seg::Dot(cx, cy, r) => {
                        circle(&mut dots, cx, cy, r);
                        has_dots = true;
                    }
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
            if has_dots && let Ok(dots) = dots.build() {
                window.paint_path(dots, color);
            }
        },
    )
    .w(px(width))
    .h(px(height))
}
