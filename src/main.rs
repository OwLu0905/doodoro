// Doodoro on GPUI directly: the gpuix-svelte version's App.svelte, ported to a GPUI view.

mod desktop;
mod draw;
mod native;
mod wobble;

use std::borrow::Cow;
use std::f64::consts::PI;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Animation, AnimationExt as _, App, Application, Bounds, Context, Div, ElementId,
    FocusHandle, FontWeight, Hsla, KeyDownEvent, MouseButton, MouseDownEvent, Rgba, Task,
    TitlebarOptions, Window, WindowBackgroundAppearance, WindowBounds, WindowKind, WindowOptions,
    div, point, prelude::*, px, size,
};

use draw::{Layer, Op, Poly, Seg, icon, shape};
use wobble::{Pt, arc, hatch, line, quad, rounded_rect, wobble};

fn hex(rgb: u32, a: f32) -> Hsla {
    let c = |shift: u32| ((rgb >> shift) & 0xff) as f32 / 255.0;
    Rgba { r: c(16), g: c(8), b: c(0), a }.into()
}

const INK: u32 = 0x5a3e2b;
const PAPER: u32 = 0xfdf5e6;
const PILL: u32 = 0xeadcc8;

/// A locked ribbon fades toward the paper but stays opaque, so the card's edge never shows
/// through it.
fn faded(rgb: u32, t: f32) -> Hsla {
    let mix = |shift: u32| {
        let (a, b) = (((rgb >> shift) & 0xff) as f32, ((PAPER >> shift) & 0xff) as f32);
        ((a + (b - a) * t).round() as u32) << shift
    };
    hex(mix(16) | mix(8) | mix(0), 1.0)
}

/// CSS's border-width, which GPUI only has in whole-pixel steps.
fn border(mut d: Div, width: f32) -> Div {
    let w = Some(px(width).into());
    d.style().border_widths = gpui::EdgesRefinement { top: w, right: w, bottom: w, left: w };
    d
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Focus,
    Short,
    Long,
}

const MODES: [Mode; 3] = [Mode::Focus, Mode::Short, Mode::Long];

impl Mode {
    fn i(self) -> usize {
        self as usize
    }
    fn color(self) -> u32 {
        [0xe9a08a, 0x8eb0e6, 0xa6aa5c][self.i()]
    }
    fn label(self) -> &'static str {
        ["專注", "短休息", "長休息"][self.i()]
    }
    fn title(self) -> &'static str {
        ["番茄專注時光", "小憩一下", "好好休息"][self.i()]
    }
    fn icon(self) -> &'static [Seg] {
        [FOCUS_ICON, SHORT_ICON, LONG_ICON][self.i()]
    }
}

use Seg::*;
#[rustfmt::skip]
const FOCUS_ICON: &[Seg] = &[
    Circle(16., 19., 9.),
    M(16., 10.), L(16., 6.),
    M(16., 10.), Q(11., 7., 9., 9.),
    M(16., 10.), Q(21., 7., 23., 9.),
];
#[rustfmt::skip]
const SHORT_ICON: &[Seg] = &[
    M(7., 13.), L(22., 13.), L(22., 20.), Q(22., 26., 14.5, 26.), Q(7., 26., 7., 20.), Z,
    M(22., 15.), L(24., 15.), Q(27., 15., 27., 18.), Q(27., 21., 24., 21.), L(22., 21.),
    M(12., 8.), Q(11., 6., 12., 4.),
    M(17., 8.), Q(16., 6., 17., 4.),
];
const LONG_ICON: &[Seg] = &[M(22., 6.), A(11., 11., true, false, 26., 21.), A(9., 9., false, true, 22., 6.), Z];
const MINUS: &[Seg] = &[M(11., 15.), L(19., 15.)];
const PLUS: &[Seg] = &[M(9.5, 15.), L(20.5, 15.), M(15., 9.5), L(15., 20.5)];
const CROSS: &[Seg] = &[M(3., 3.), L(9., 9.), M(9., 3.), L(3., 9.)];
const DASH: &[Seg] = &[M(3., 6.), L(9., 6.)];
// Six dots, the usual "grab here" mark.
#[rustfmt::skip]
const GRIP: &[Seg] = &[
    Dot(2.5, 2.5, 1.4), Dot(2.5, 7., 1.4), Dot(2.5, 11.5, 1.4),
    Dot(7.5, 2.5, 1.4), Dot(7.5, 7., 1.4), Dot(7.5, 11.5, 1.4),
];

// The window is transparent and borderless so the card's own shape forms it. It is a little
// larger than the 264 × 360 card, leaving room for the ribbons above it and the shadow below it.
// Minimised, it shrinks to a pill around the time.
const FULL: (f32, f32) = (280., 392.);
const MINI: (f32, f32) = (128., 50.);
const CHIP: (f32, f32) = (62., 18.);

/// A compact take on the 380 × 560 design: the card is 264 × 360. The ribbons and the ring are
/// drawn in the design's own coordinates and scaled down; the rest is drawn at size, with a
/// gentler wobble to suit it.
struct Shapes {
    card: Rc<Poly>,
    ribbon: Rc<Poly>,
    fold: Rc<Poly>,
    round_button: Rc<Poly>,
    chip_hatch: Vec<Rc<Poly>>,
    mini_pill: Rc<Poly>,
    mini_mark: Rc<Poly>,
    /// The main button, wide and narrow.
    buttons: [Rc<Poly>; 2],
    /// The ring in the prototype's 240 × 240 space: track r=100, outlines at r=111 and r=89.
    track: Rc<Poly>,
    outlines: [Rc<Poly>; 2],
}

fn w(points: Vec<Pt>, closed: bool, scale: Option<f64>) -> Rc<Poly> {
    Poly::new(wobble(&points, scale), closed)
}

impl Shapes {
    fn new() -> Self {
        let circle = |r: f64| w(arc(120., 120., r, 0., 2. * PI), true, None);
        let button = |width: f64| w(rounded_rect(0., 0., width, 36., 18.), true, Some(3.));
        let ribbon = [
            quad([6., 10.], [6., 3.], [13., 3.]),
            line([13., 3.], [45., 3.]),
            quad([45., 3.], [52., 3.], [52., 10.]),
            line([52., 10.], [52., 86.]),
            line([52., 86.], [29., 74.]),
            line([29., 74.], [6., 86.]),
            line([6., 86.], [6., 10.]),
        ];
        let fold = [quad([2., 14.], [2., 6.], [10., 6.]), line([10., 6.], [48., 6.]), quad([48., 6.], [56., 6.], [56., 14.])];
        Self {
            card: w(rounded_rect(1.25, 1.25, 261.5, 357.5, 28.75), true, Some(4.)),
            ribbon: w(ribbon.concat(), true, None),
            fold: w(fold.concat(), false, None),
            round_button: w(arc(15., 15., 13.5, 0., 2. * PI), true, Some(2.)),
            chip_hatch: hatch(CHIP.0 as f64, CHIP.1 as f64, 4.).into_iter().map(|l| w(l, false, Some(2.))).collect(),
            mini_pill: w(rounded_rect(1.1, 1.1, 117.8, 37.8, 18.9), true, Some(2.)),
            mini_mark: w(line([3., 2.], [17., 2.]), false, Some(1.)),
            buttons: [button(132.), button(100.)],
            track: circle(100.),
            outlines: [circle(111.), circle(89.)],
        }
    }
}

/// CSS's cubic-bezier(), for the design's easings.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 {
    move |x: f32| {
        let bez = |a: f32, b: f32, t: f32| 3. * a * t * (1. - t).powi(2) + 3. * b * t * t * (1. - t) + t.powi(3);
        let (mut lo, mut hi) = (0f32, 1f32);
        for _ in 0..24 {
            let mid = (lo + hi) / 2.;
            if bez(x1, x2, mid) < x { lo = mid } else { hi = mid }
        }
        bez(y1, y2, (lo + hi) / 2.)
    }
}

/// Animates one number from `from` to `to`, or sets it outright when there's nothing to animate.
fn tween<E: IntoElement + 'static>(
    el: E,
    id: ElementId,
    (from, to): (f32, f32),
    secs: f32,
    ease: impl Fn(f32) -> f32 + 'static,
    apply: impl Fn(E, f32) -> E + 'static,
) -> AnyElement {
    if from == to {
        return apply(el, to).into_any_element();
    }
    el.with_animation(id, Animation::new(Duration::from_secs_f32(secs)).with_easing(ease), move |el, d| {
        apply(el, from + (to - from) * d)
    })
    .into_any_element()
}

fn id(name: &'static str, n: usize) -> ElementId {
    ElementId::NamedInteger(name.into(), n as u64)
}

/// GPUI has no letter-spacing, so each character gets its own box and the gap stands in for it.
/// `em` is the spacing in ems of `size`.
fn spaced(text: &str, size: f32, em: f32) -> Div {
    div().flex().flex_row().items_center().whitespace_nowrap().gap(px(size * em)).text_size(px(size)).children(
        text.chars().map(|ch| if ch == ' ' { div().w(px(size * 0.3)) } else { div().child(ch.to_string()) }),
    )
}

/// Tabular figures: every digit sits in a box of the same width, so the clock doesn't jitter.
fn digits(text: &str, size: f32) -> Div {
    div()
        .flex()
        .flex_row()
        .h(px(size))
        .text_size(px(size))
        .font_family("Grandstander")
        .font_weight(FontWeight::EXTRA_BOLD)
        .children(text.chars().map(|ch| {
            div()
                .flex()
                .items_center()
                .justify_center()
                .w(px(size * if ch == ':' { 0.3 } else { 0.6 }))
                .h(px(size))
                .child(ch.to_string())
        }))
}

#[derive(Clone, Copy, PartialEq)]
enum Morph {
    Shrink,
    Grow,
    /// The box has reached the card's outline and stays under it while the card fades in.
    Settle,
}

#[derive(Clone, Copy, PartialEq)]
struct BoxPos {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    radius: f32,
}

impl BoxPos {
    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let l = |x: f32, y: f32| x + (y - x) * t;
        Self {
            left: l(a.left, b.left),
            top: l(a.top, b.top),
            width: l(a.width, b.width),
            height: l(a.height, b.height),
            radius: l(a.radius, b.radius),
        }
    }
}

// Between the card and the pill, a plain paper box morphs from one outline to the other while the
// detail fades. The window is full size for the whole morph: it shrinks after, and grows before.
const CARD_BOX: BoxPos = BoxPos { left: 8., top: 18., width: 264., height: 360., radius: 29. };
const PILL_BOX: BoxPos = BoxPos { left: 4., top: 3., width: 120., height: 40., radius: 20. };
const MORPH_S: f32 = 0.3;

struct Doodoro {
    shapes: Rc<Shapes>,
    focus: FocusHandle,
    sound: bool,
    notifications: bool,

    mode: Mode,
    /// Minutes for focus, short, long.
    dur: [u32; 3],
    /// Milliseconds; None means a full session.
    remaining: Option<u64>,
    running: bool,
    end_at: Instant,
    /// Ticks only while a session runs, so an idle card costs nothing.
    ticker: Option<Task<()>>,
    grip_hint: bool,
    mini: bool,
    /// The session ran out and waits on 下一步.
    done: bool,
    cycle: u32,

    morph: Option<Morph>,
    morph_gen: usize,
    /// Opened near a screen edge, the card is pushed back onto the screen, so the pill it grows
    /// from (and shrinks back to) is no longer at the window's corner.
    pill_box: BoxPos,
    /// Bumped to fade the card in or out again.
    card_gen: usize,
    /// Where each ribbon starts its bounce from when the session changes.
    tab_from: [f32; 3],
    tab_gen: usize,
    /// The pill's time fades in, and dims while paused.
    mini_fade: (f32, f32),
    mini_gen: usize,
}

impl Doodoro {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        Self {
            shapes: Rc::new(Shapes::new()),
            focus,
            sound: true,
            notifications: true,
            mode: Mode::Focus,
            dur: desktop::load_durations().unwrap_or([25, 5, 30]),
            remaining: None,
            running: false,
            end_at: Instant::now(),
            ticker: None,
            grip_hint: false,
            mini: false,
            done: false,
            cycle: 0,
            morph: None,
            morph_gen: 0,
            pill_box: PILL_BOX,
            card_gen: 0,
            tab_from: [6., 0., 0.],
            tab_gen: 0,
            mini_fade: (0., 0.),
            mini_gen: 0,
        }
    }

    fn total(&self) -> u64 {
        self.dur[self.mode.i()] as u64 * 60_000
    }
    fn rem(&self) -> u64 {
        self.remaining.unwrap_or(self.total())
    }
    fn rem_s(&self) -> u64 {
        self.rem().div_ceil(1000)
    }
    fn fresh(&self) -> bool {
        !self.done && !self.running && self.rem_s() * 1000 == self.total()
    }
    /// Mid-session, the other ribbons are locked until it is reset, skipped or done.
    fn locked(&self) -> bool {
        !self.fresh() && !self.done
    }
    fn time_text(&self) -> String {
        format!("{:02}:{:02}", self.rem_s() / 60, self.rem_s() % 60)
    }

    // Remaining time is derived from a deadline, not counted ticks, so it doesn't drift.
    fn left_ms(&self) -> u64 {
        self.end_at.saturating_duration_since(Instant::now()).as_millis() as u64
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.end_at = Instant::now() + Duration::from_millis(self.rem());
        self.running = true;
        self.ticker = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(100)).await;
                let going = this.update(cx, |this, cx| {
                    this.remaining = Some(this.left_ms());
                    if this.remaining == Some(0) {
                        this.complete();
                    }
                    cx.notify();
                    this.running
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        }));
    }

    fn stop(&mut self) {
        self.running = false;
        self.ticker = None;
    }

    fn main(&mut self, cx: &mut Context<Self>) {
        if self.done {
            self.advance();
        } else if self.running {
            self.remaining = Some(self.left_ms());
            self.stop();
        } else {
            self.start(cx);
        }
        cx.notify();
    }

    fn reset(&mut self) {
        self.stop();
        self.remaining = None;
    }

    fn set_mode(&mut self, m: Mode) {
        self.stop();
        self.done = false;
        if m != self.mode {
            self.tab_from = MODES.map(|t| if t == self.mode { 6. } else { 0. });
            self.tab_gen += 1;
        }
        self.mode = m;
        self.remaining = None;
    }

    fn adjust(&mut self, m: Mode, d: i32) {
        self.dur[m.i()] = (self.dur[m.i()] as i32 + d).clamp(1, 90) as u32;
        desktop::save_durations(self.dur);
        if m == self.mode {
            self.remaining = None;
        }
    }

    /// The timer ran out: stay put with a full ring until 下一步 is pressed.
    fn complete(&mut self) {
        self.stop();
        self.remaining = Some(0);
        self.done = true;
        let focus = self.mode == Mode::Focus;
        if focus {
            self.cycle += 1;
        }
        if self.sound {
            desktop::chime();
        }
        if self.notifications {
            desktop::notify(if focus { "專注完成！" } else { "休息結束" }, if focus { "休息一下吧" } else { "回來專心囉" });
        }
    }

    /// A focus session counts toward the cycle whether it ran out or was skipped.
    fn skip(&mut self) {
        if self.mode == Mode::Focus {
            self.cycle += 1;
        }
        self.advance();
    }

    /// On to the next session, idle: after focus a break (long every fourth), after a break focus.
    fn advance(&mut self) {
        let next = match self.mode {
            Mode::Focus if self.cycle % 4 == 0 => Mode::Long,
            Mode::Focus => Mode::Short,
            _ => Mode::Focus,
        };
        if self.mode == Mode::Long {
            self.cycle = 0;
        }
        self.set_mode(next);
    }

    /// Dragging the card's background (or the grip, which says so) moves the window. The shim
    /// polls the pointer, since mouse events stop arriving once the window moves under them.
    fn press(&mut self, cx: &mut Context<Self>) {
        if !native::drag_begin() {
            return;
        }
        cx.spawn(async move |_, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(8)).await;
                if !native::drag_step() {
                    break;
                }
            }
        })
        .detach();
    }

    fn place_pill(&mut self) {
        let (x, y) = native::pill_offset();
        self.pill_box = BoxPos { left: PILL_BOX.left + x, top: PILL_BOX.top + y, ..PILL_BOX };
    }

    fn after(&self, secs: f32, cx: &mut Context<Self>, f: impl FnOnce(&mut Self) + 'static) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs_f32(secs)).await;
            this.update(cx, |this, cx| {
                f(this);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // The window is resized from a task, outside any update: AppKit tells GPUI of the new size
    // right away, and while the app is borrowed GPUI drops that, keeping (and clipping to) the old
    // size until something else, like focus, makes it measure again.
    fn minimize(&mut self, cx: &mut Context<Self>) {
        if self.morph.is_some() {
            return;
        }
        self.place_pill();
        self.morph = Some(Morph::Shrink);
        self.morph_gen += 1;
        self.card_gen += 1;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs_f32(MORPH_S)).await;
            this.update(cx, |this, cx| {
                this.mini = true;
                this.morph = None;
                this.mini_fade = (0., this.mini_opacity());
                this.mini_gen += 1;
                cx.notify();
            })
            .ok();
            native::resize_window(MINI.0, MINI.1);
        })
        .detach();
    }

    /// A press on the pill drags it; a click that didn't move it opens the card again.
    fn expand(&mut self, cx: &mut Context<Self>) {
        if self.morph.is_some() || native::window_was_dragged() {
            return;
        }
        // Taken now, so a second click before the window has grown is ignored too.
        self.morph = Some(Morph::Grow);
        cx.spawn(async move |this, cx| {
            native::resize_window(FULL.0, FULL.1);
            this.update(cx, |this, cx| {
                this.place_pill();
                this.mini = false;
                this.morph_gen += 1;
                cx.notify();
                this.after(MORPH_S, cx, |this| {
                    this.morph = Some(Morph::Settle);
                    this.card_gen += 1;
                });
                this.after(MORPH_S + 0.2, cx, |this| this.morph = None);
            })
            .ok();
        })
        .detach();
    }

    fn mini_opacity(&self) -> f32 {
        if self.running || self.done { 1. } else { 0.55 }
    }

    fn render_morph(&self, morph: Morph) -> AnyElement {
        let (from, to) = match morph {
            Morph::Grow => (self.pill_box, CARD_BOX),
            Morph::Shrink => (CARD_BOX, self.pill_box),
            Morph::Settle => (CARD_BOX, CARD_BOX),
        };
        let place = |d: Div, b: BoxPos| d.left(px(b.left)).top(px(b.top)).w(px(b.width)).h(px(b.height)).rounded(px(b.radius));
        let el = border(div().absolute().bg(hex(PAPER, 1.)).border_color(hex(INK, 1.)), 2.5);
        if from == to {
            return place(el, to).into_any_element();
        }
        el.with_animation(
            id("morph", self.morph_gen),
            Animation::new(Duration::from_secs_f32(MORPH_S)).with_easing(cubic_bezier(0.4, 0., 0.2, 1.)),
            move |el, d| place(el, BoxPos::lerp(from, to, d)),
        )
        .into_any_element()
    }

    fn render_mini(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = &self.shapes;
        let mark = Layer { ops: vec![Op::Stroke(s.mini_mark.clone(), 3.)], color: hex(self.mode.color(), 1.) };
        let time = div()
            .absolute()
            .left_0()
            .top_0()
            .w(px(120.))
            .h(px(40.))
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(7.))
            .child(
                div()
                    .relative()
                    .w(px(20.))
                    .h(px(23.))
                    .child(icon(self.mode.icon(), 32., 2.6, 18., 18., hex(INK, 1.)).absolute().left(px(1.)).top_0())
                    .child(shape(20., 4., 20., vec![mark]).top(px(17.))),
            )
            // Grandstander's figures ride high in their line box; this centres the ink in the pill.
            .child(div().mt(px(5.)).child(digits(&self.time_text(), 20.)));
        div()
            .id("mini")
            .absolute()
            .left(px(4.))
            .top(px(3.))
            .w(px(120.))
            .h(px(44.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| this.press(cx)))
            .on_click(cx.listener(|this, _, _, cx| this.expand(cx)))
            .child(shape(120., 40., 120., vec![Layer { ops: vec![Op::Fill(s.mini_pill.clone())], color: hex(INK, 0.18) }]).top(px(4.)))
            .child(shape(
                120.,
                40.,
                120.,
                vec![
                    Layer { ops: vec![Op::Fill(s.mini_pill.clone())], color: hex(PAPER, 1.) },
                    Layer { ops: vec![Op::Stroke(s.mini_pill.clone(), 2.2)], color: hex(INK, 1.) },
                ],
            ))
            .child(tween(time, id("mini-time", self.mini_gen), self.mini_fade, 0.15, |t| t, |el, o| el.opacity(o)))
            .into_any_element()
    }

    fn render_setup(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = &self.shapes;
        let ink = hex(INK, 1.);
        let mode = self.mode;
        let round = |name: &'static str, glyph: &'static [Seg], d: i32| {
            div()
                .id(name)
                .relative()
                .w(px(30.))
                .h(px(30.))
                .rounded(px(15.))
                .cursor_pointer()
                .hover(|st| st.bg(hex(INK, 0.1)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.adjust(mode, d);
                    cx.notify();
                }))
                .child(shape(30., 30., 30., vec![Layer { ops: vec![Op::Stroke(s.round_button.clone(), 2.)], color: ink }]))
                .child(icon(glyph, 30., 2.4, 30., 30., ink))
        };
        let chips = MODES.into_iter().filter(|&m| m != mode).map(|m| {
            let hatching = Layer { ops: s.chip_hatch.iter().map(|l| Op::Stroke(l.clone(), 1.6)).collect(), color: hex(PILL, 1.) };
            div()
                .id(("chip", m.i()))
                .relative()
                .w(px(CHIP.0))
                .h(px(CHIP.1))
                .flex()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(4.))
                .cursor_pointer()
                .hover(|st| st.opacity(0.7))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_mode(m);
                    cx.notify();
                }))
                .child(shape(CHIP.0, CHIP.1, CHIP.0, vec![hatching]))
                .child(spaced(m.label(), 9., 0.04).font_weight(FontWeight::BLACK))
                .child(
                    div()
                        .font_family("Grandstander")
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_size(px(9.))
                        .mt(px(2.))
                        .child(self.dur[m.i()].to_string()),
                )
        });
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(round("down", MINUS, -1))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap(px(4.))
                            .child(digits(&self.dur[mode.i()].to_string(), 52.))
                            .child(spaced("分鐘", 10., 0.05).font_weight(FontWeight::BLACK).mb(px(7.))),
                    )
                    .child(round("up", PLUS, 1)),
            )
            .child(div().flex().flex_row().gap(px(12.)).children(chips))
            .into_any_element()
    }

    fn render_ring(&self) -> AnyElement {
        let s = &self.shapes;
        // The arc grows clockwise from 12 o'clock as the session elapses.
        let p = 1. - self.rem() as f64 / self.total() as f64;
        let mut progress = vec![];
        if p > 0.001 {
            let top = -PI / 2.;
            progress.push(Op::Stroke(w(arc(120., 120., 100., top, top + p * 2. * PI), p >= 0.999, None), 18.));
        }
        let status = match (self.done, self.running, self.mode == Mode::Focus) {
            (true, _, true) => "番茄完成囉",
            (true, _, false) => "休息結束囉",
            (false, true, true) => "專心一點點",
            (false, true, false) => "喝口茶吧",
            (false, false, _) => "休息一下再繼續",
        };
        let outlines = vec![Op::Stroke(s.outlines[0].clone(), 3.5), Op::Stroke(s.outlines[1].clone(), 3.5)];
        div()
            .relative()
            .w(px(168.))
            .h(px(168.))
            .child(shape(
                168.,
                168.,
                240.,
                vec![
                    Layer { ops: vec![Op::Stroke(s.track.clone(), 18.)], color: hex(PILL, 1.) },
                    Layer { ops: progress, color: hex(self.mode.color(), 1.) },
                    Layer { ops: outlines, color: hex(INK, 1.) },
                ],
            ))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size(px(168.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(3.))
                    .child(digits(&self.time_text(), 36.))
                    .child(spaced(status, 10., 0.15).font_weight(FontWeight::BOLD).opacity(0.75)),
            )
            .into_any_element()
    }

    fn render_card(&self, cx: &mut Context<Self>) -> AnyElement {
        let s = &self.shapes;
        let ink = hex(INK, 1.);
        let locked = self.locked();
        let kicker = if self.done {
            "完成了"
        } else if self.fresh() {
            "開始你的"
        } else if self.running {
            "正在進行"
        } else {
            "暫停中"
        };
        let main_label = if self.done {
            "下一步"
        } else if self.running {
            "暫停"
        } else if self.fresh() {
            "開始"
        } else {
            "繼續"
        };
        let middle = if self.fresh() { self.render_setup(cx) } else { self.render_ring() };
        let main_width = if locked { 100. } else { 132. };
        let main_shape = s.buttons[locked as usize].clone();
        let text_btn = |name: &'static str, label: &'static str, f: fn(&mut Self)| {
            div()
                .id(name)
                .h(px(28.))
                .px(px(7.))
                .rounded(px(14.))
                .flex()
                .items_center()
                .cursor_pointer()
                .hover(|st| st.bg(hex(INK, 0.1)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    f(this);
                    cx.notify();
                }))
                .child(spaced(label, 11., 0.1).font_weight(FontWeight::BOLD))
        };
        let main_button = div()
            .id("toggle")
            .relative()
            .w(px(main_width))
            .h(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .active(|st| st.top(px(2.)))
            .on_click(cx.listener(|this, _, _, cx| this.main(cx)))
            .child(shape(main_width, 36., main_width, vec![Layer { ops: vec![Op::Fill(main_shape.clone())], color: hex(INK, 0.25) }]).top(px(3.)))
            .child(shape(main_width, 36., main_width, vec![Layer { ops: vec![Op::Fill(main_shape)], color: ink }]))
            .child(spaced(main_label, 13., 0.3).font_weight(FontWeight::BLACK).text_color(hex(PAPER, 1.)));
        let dots = (0..4).map(|i| {
            border(div().size(px(8.)).rounded(px(4.)).border_color(ink), 1.5).when(i < self.cycle % 4, |d| d.bg(hex(Mode::Focus.color(), 1.)))
        });

        let content = div()
            .absolute()
            .left_0()
            .top_0()
            .w(px(264.))
            .h(px(360.))
            .pt(px(62.))
            .px(px(18.))
            .pb(px(18.))
            .flex()
            .flex_col()
            .items_center()
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| this.press(cx)))
            .child(spaced(kicker, 11., 0.3).font_weight(FontWeight::BOLD).opacity(0.8))
            .child(spaced(self.mode.title(), 20., 0.06).font_weight(FontWeight::BLACK).mt(px(4.)).h(px(24.)))
            .child(
                div()
                    .flex_grow()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .child(middle),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .when(locked, |d| d.child(text_btn("reset", "重設", Self::reset)))
                    .child(main_button)
                    .when(locked, |d| d.child(text_btn("skip", "跳過", Self::skip))),
            )
            .child(div().mt(px(12.)).flex().flex_row().gap(px(4.)).children(dots));

        let corner = |name: &'static str, glyph: &'static [Seg], left: f32| {
            div()
                .id(name)
                .absolute()
                .top(px(13.))
                .left(px(left))
                .size(px(20.))
                .rounded(px(10.))
                .flex()
                .items_center()
                .justify_center()
                .opacity(0.55)
                .cursor_pointer()
                .hover(|st| st.opacity(1.).bg(hex(INK, 0.1)))
                .child(icon(glyph, 12., 2.2, 10., 10., ink))
        };

        // Last in the tree so the ribbons paint, and take clicks, above the card.
        let tabs = div()
            .absolute()
            .left_0()
            .top(px(-14.))
            .w(px(264.))
            .h(px(66.))
            .flex()
            .flex_row()
            .justify_center()
            .gap(px(14.))
            .children(MODES.map(|m| {
                let dim = locked && m != self.mode;
                let line = if dim { faded(INK, 0.45) } else { ink };
                let body = if dim { faded(m.color(), 0.55) } else { hex(m.color(), 1.) };
                let ribbon = vec![
                    Layer { ops: vec![Op::Fill(s.ribbon.clone())], color: body },
                    Layer { ops: vec![Op::Stroke(s.ribbon.clone(), 3.5), Op::Stroke(s.fold.clone(), 3.5)], color: line },
                ];
                let tab = div()
                    .id(("tab", m.i()))
                    .relative()
                    .w(px(36.))
                    .h(px(60.))
                    .when(!dim, |d| d.cursor_pointer())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.locked() {
                            this.set_mode(m);
                            cx.notify();
                        }
                    }))
                    .child(shape(36., 60., 58., ribbon))
                    .child(icon(m.icon(), 32., 2.6, 18., 18., line).absolute().left(px(9.)).top(px(21.)));
                let to = if self.mode == m { 6. } else { 0. };
                tween(
                    tab,
                    id(["tab-focus", "tab-short", "tab-long"][m.i()], self.tab_gen),
                    (self.tab_from[m.i()], to),
                    0.25,
                    cubic_bezier(0.3, 1.6, 0.5, 1.),
                    |el, top| el.top(px(top)),
                )
            }));

        let grip = div()
            .id("grip")
            .absolute()
            .left(px(228.))
            .top(px(12.))
            .size(px(22.))
            .rounded(px(11.))
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.55)
            .cursor_grab()
            .hover(|st| st.opacity(1.).bg(hex(INK, 0.1)))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                this.grip_hint = *hovered;
                cx.notify();
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(|this, _: &MouseDownEvent, _, cx| this.press(cx)))
            .child(icon(GRIP, 10., 0., 8., 11.2, ink));

        let card = div()
            .absolute()
            .left(px(8.))
            .top(px(18.))
            .w(px(264.))
            .h(px(360.))
            .child(shape(264., 360., 264., vec![Layer { ops: vec![Op::Fill(s.card.clone())], color: hex(INK, 0.18) }]).top(px(5.)))
            .child(shape(
                264.,
                360.,
                264.,
                vec![
                    Layer { ops: vec![Op::Fill(s.card.clone())], color: hex(PAPER, 1.) },
                    Layer { ops: vec![Op::Stroke(s.card.clone(), 2.5)], color: ink },
                ],
            ))
            .child(content)
            // The ribbons take the top edge, so a grip in the corner shows where to hold. Close
            // sits top left, where macOS puts it.
            .child(grip)
            .child(corner("close", CROSS, 10.).on_click(|_, _, cx| cx.quit()))
            .child(corner("minimize", DASH, 30.).on_click(cx.listener(|this, _, _, cx| this.minimize(cx))))
            .child(tabs)
            .when(self.grip_hint, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(178.))
                        .top(px(54.))
                        .px(px(7.))
                        .py(px(2.))
                        .rounded(px(8.))
                        .bg(ink)
                        .text_color(hex(PAPER, 1.))
                        .text_size(px(10.))
                        .font_weight(FontWeight::BOLD)
                        .child("按住這裡拖曳"),
                )
            });
        let shrinking = self.morph == Some(Morph::Shrink);
        let fade = if shrinking { (1., 0.) } else { (0., 1.) };
        tween(card, id("card", self.card_gen), fade, if shrinking { 0.12 } else { 0.18 }, |t| t, |el, o| el.opacity(o))
    }
}

impl Render for Doodoro {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Once a ribbon has bounced it stays put, so a later remount of the card doesn't replay it.
        let resting = MODES.map(|m| if m == self.mode { 6. } else { 0. });
        if self.tab_from != resting {
            let gen_now = self.tab_gen;
            self.after(0.3, cx, move |this| {
                if this.tab_gen == gen_now {
                    this.tab_from = resting;
                }
            });
        }
        let target = self.mini_opacity();
        if self.mini && self.mini_fade.1 != target {
            self.mini_fade = (self.mini_fade.1, target);
            self.mini_gen += 1;
        }
        div()
            .size_full()
            .font_family("Zen Maru Gothic")
            .text_color(hex(INK, 1.))
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                match e.keystroke.key.as_str() {
                    "space" => this.main(cx),
                    "r" if this.locked() => this.reset(),
                    "s" if this.locked() => this.skip(),
                    _ => return,
                }
                cx.notify();
            }))
            .when_some(self.morph, |d, m| d.child(self.render_morph(m)))
            .map(|d| {
                if self.mini {
                    d.child(self.render_mini(cx))
                } else if self.morph != Some(Morph::Grow) {
                    d.child(self.render_card(cx))
                } else {
                    d
                }
            })
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        // Only the weights the card uses: Zen Maru Gothic Bold and Black, Grandstander ExtraBold.
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(include_bytes!("../fonts/ZenMaruGothic-Bold.ttf")),
                Cow::Borrowed(include_bytes!("../fonts/ZenMaruGothic-Black.ttf")),
                Cow::Borrowed(include_bytes!("../fonts/Grandstander-ExtraBold.ttf")),
            ])
            .expect("fonts");

        // Opened straight where it was last dropped: GPUI takes the position up front, so nothing
        // moves once it is on screen.
        let full = size(px(FULL.0), px(FULL.1));
        let saved = native::saved_position(FULL.0 as f64, FULL.1 as f64);
        let display = saved.and_then(|(_, _, d)| cx.displays().into_iter().find(|s| u32::from(s.id()) == d));
        let bounds = match (saved, &display) {
            (Some((x, y, _)), Some(_)) => Bounds::new(point(px(x as f32), px(y as f32)), full),
            _ => Bounds::centered(None, full, cx),
        };
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions { title: Some("Doodoro".into()), appears_transparent: true, traffic_light_position: None }),
                // Shown below, in this same turn, once the traffic lights are gone: it waits on
                // nothing, so it doesn't delay the window.
                show: false,
                focus: false,
                kind: WindowKind::Normal,
                is_movable: false,
                is_resizable: false,
                is_minimizable: false,
                display_id: display.map(|d| d.id()),
                window_background: WindowBackgroundAppearance::Transparent,
                ..Default::default()
            },
            |window, cx| cx.new(|cx| Doodoro::new(window, cx)),
        )
        .expect("window");
        native::hide_window_buttons();
        native::float_window();
        native::show_window();

        // DOODORO_SCREENSHOT=/tmp/app.png writes what the window draws, a second after it opens.
        if let Ok(path) = std::env::var("DOODORO_SCREENSHOT") {
            cx.spawn(async move |cx| {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                if !native::screenshot(&path) {
                    eprintln!("[doodoro] couldn't write {path}");
                }
            })
            .detach();
        }
    });
}
