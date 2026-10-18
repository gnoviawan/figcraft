//! Guests: an embedding host's agents on the canvas (`host.guestCursor`, `host.guestFlash`,
//! `host.guestHide`). A guest is a named pointer in its own colour that glides to where its agent
//! works and rings the nodes it writes or reads. Guests live in the window only: never synced,
//! saved, or part of the document or its undo. A guest nobody updates fades away on its own, so a
//! host that dies leaves no pointer behind.
//!
//! Every lifecycle step is a function of the time it is given, so the model runs on any clock;
//! the window uses [`Guests::now`].

use std::collections::BTreeMap;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke, Vec2, vec2};
use figcraft_doc::{Document, NodeId};
use serde_json::{Value, json};

use crate::FigApp;

/// A new pointer fades in over this long (seconds).
const FADE_IN: f64 = 0.2;
/// A guest with no update for this long starts to fade out...
const IDLE: f64 = 5.0;
/// ...over this long, then is dropped (`host.guestHide` fades the same way).
const FADE_OUT: f64 = 1.5;
/// How fast a pointer closes in on its target (per second): slow, so an agent's burst of calls
/// reads as one deliberate move.
const GLIDE: f64 = 2.5;
/// Idle drift of a pointer at rest, in screen points, and how fast it blends in and out.
const WOBBLE: f32 = 4.0;
const WOBBLE_AFTER: f64 = 0.25;
const WOBBLE_BLEND: f64 = 4.0;
/// Rings kept per guest (the newest).
const MAX_FLASHES: usize = 100;
/// A ring sits this far outside its node, and starts this much further out when it grows in.
const RING_PAD: f32 = 5.0;
const RING_GROW: f32 = 30.0;
/// How far a ring drifts outward as it fades.
const RING_RELEASE: f32 = 6.0;
/// A read ring's scan band: one sweep top to bottom takes this long.
const SCAN: f64 = 0.75;
/// The edge indicator keeps this far inside the canvas.
const EDGE_INSET: f32 = 18.0;
const NAME_MAX: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlashKind {
    Write,
    Read,
}

impl FlashKind {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "write" => Some(FlashKind::Write),
            "read" => Some(FlashKind::Read),
            _ => None,
        }
    }

    /// Grow in, hold, release (seconds).
    fn timeline(self) -> (f64, f64, f64) {
        match self {
            FlashKind::Write => (0.2, 0.4, 0.3),
            FlashKind::Read => (0.2, 1.5, 0.3),
        }
    }

    fn duration(self) -> f64 {
        let (grow, hold, release) = self.timeline();
        grow + hold + release
    }
}

/// How a ring looks `t` seconds after it started: how far outside its resting place and how
/// opaque, and for a read the scan band's place (0 top, 1 bottom). `None`: not started or over.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RingLook {
    pub out: f32,
    pub alpha: f32,
    pub scan: Option<f32>,
}

pub fn ring_look(kind: FlashKind, t: f64, reduce: bool) -> Option<RingLook> {
    let (grow, hold, release) = kind.timeline();
    if !(0.0..kind.duration()).contains(&t) {
        return None;
    }
    let (out, alpha) = if t < grow {
        let x = t / grow;
        (RING_GROW * (1.0 - ease_out_back(x) as f32), x as f32)
    } else if t < grow + hold {
        (0.0, 1.0)
    } else {
        let y = ((t - grow - hold) / release) as f32;
        (RING_RELEASE * y, 1.0 - y)
    };
    let scan = (kind == FlashKind::Read && !reduce && (grow..grow + hold).contains(&t)).then(|| (((t - grow) / SCAN) % 1.0) as f32);
    Some(RingLook { out: if reduce { 0.0 } else { out }, alpha, scan })
}

fn ease_out_back(x: f64) -> f64 {
    let (c1, c3) = (1.70158, 2.70158);
    1.0 + c3 * (x - 1.0).powi(3) + c1 * (x - 1.0).powi(2)
}

fn ease_out_cubic(x: f64) -> f64 {
    1.0 - (1.0 - x).powi(3)
}

struct Flash {
    node: NodeId,
    kind: FlashKind,
    start: f64,
}

pub struct Guest {
    pub name: String,
    pub color: Color32,
    /// The page the host means (`None`: whatever page is in front).
    pub page: Option<NodeId>,
    /// Where the host last sent the pointer, and where it is drawn now (page coordinates).
    pub target: (f64, f64),
    pub shown: (f64, f64),
    /// When the fade-in started.
    born: f64,
    /// The host's last update.
    touched: f64,
    /// `host.guestHide`: the fade-out started here rather than `IDLE` after the last update.
    hiding: Option<f64>,
    /// When the pointer reached its target; `None` while it glides.
    arrived: Option<f64>,
    /// How much of the idle drift shows (0..1), eased so starting a glide never jumps.
    calm: f32,
    phase: f32,
    flashes: Vec<Flash>,
}

impl Guest {
    fn fade_out_at(&self) -> f64 {
        self.hiding.unwrap_or(self.touched + IDLE)
    }

    pub fn opacity(&self, now: f64) -> f32 {
        let fade_in = ease_out_cubic(((now - self.born) / FADE_IN).clamp(0.0, 1.0));
        let fade_out = 1.0 - ((now - self.fade_out_at()) / FADE_OUT).clamp(0.0, 1.0);
        (fade_in * fade_out) as f32
    }

    fn gone(&self, now: f64) -> bool {
        now >= self.fade_out_at() + FADE_OUT
    }

    /// The rings still to show, with when they started.
    pub fn flashes(&self) -> impl Iterator<Item = (NodeId, FlashKind, f64)> + '_ {
        self.flashes.iter().map(|f| (f.node, f.kind, f.start))
    }

    /// The idle drift in screen points.
    pub fn wobble(&self, now: f64) -> Vec2 {
        let t = now as f32;
        vec2((t * 1.3 + self.phase).sin(), (t * 0.83 + self.phase * 1.7).sin()) * WOBBLE * self.calm
    }

    /// Back in view: an update or a hide being taken back while it fades.
    fn revive(&mut self, now: f64) {
        let shown = self.opacity(now);
        if shown < 1.0 {
            // Fade in from where the fade-out left it rather than popping to full.
            let x = 1.0 - (1.0 - f64::from(shown)).cbrt();
            self.born = now - x * FADE_IN;
        }
        self.hiding = None;
        self.touched = now;
    }
}

/// The guests in a window, by the id their host gave them.
pub struct Guests {
    epoch: web_time::Instant,
    guests: BTreeMap<String, Guest>,
    /// The last frame's time, for the glide step.
    last: Option<f64>,
}

impl Default for Guests {
    fn default() -> Self {
        Self { epoch: web_time::Instant::now(), guests: BTreeMap::new(), last: None }
    }
}

impl Guests {
    /// The window's clock (seconds). It runs while no frames are drawn, so a guest left by a host
    /// that died while the canvas was hidden is gone by the time the canvas shows again.
    pub fn now(&self) -> f64 {
        self.epoch.elapsed().as_secs_f64()
    }

    pub fn get(&self, id: &str) -> Option<&Guest> {
        self.guests.get(id)
    }

    pub fn is_empty(&self) -> bool {
        self.guests.is_empty()
    }

    /// Show guest `id` heading for `at`: a new guest appears there and fades in, a known one
    /// glides there.
    pub fn cursor(&mut self, now: f64, id: &str, name: &str, color: Color32, at: (f64, f64), page: Option<NodeId>) {
        self.expire(now);
        let name: String = name.chars().take(NAME_MAX).collect();
        match self.guests.get_mut(id) {
            Some(g) => {
                g.revive(now);
                if g.target != at {
                    g.arrived = None;
                }
                (g.name, g.color, g.page, g.target) = (name, color, page, at);
            }
            None => {
                let phase = id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b))) as f32 % 6.283;
                self.guests.insert(
                    id.to_string(),
                    Guest {
                        name,
                        color,
                        page,
                        target: at,
                        shown: at,
                        born: now,
                        touched: now,
                        hiding: None,
                        arrived: Some(now),
                        calm: 0.0,
                        phase,
                        flashes: Vec::new(),
                    },
                );
            }
        }
    }

    /// Ring `nodes` in guest `id`'s colour. An unknown guest is ignored.
    pub fn flash(&mut self, now: f64, id: &str, nodes: &[NodeId], kind: FlashKind) {
        self.expire(now);
        let Some(g) = self.guests.get_mut(id) else { return };
        g.revive(now);
        g.flashes.extend(nodes.iter().map(|&node| Flash { node, kind, start: now }));
        let over = g.flashes.len().saturating_sub(MAX_FLASHES);
        g.flashes.drain(..over);
    }

    /// Fade guest `id` out and drop it. An unknown guest is ignored.
    pub fn hide(&mut self, now: f64, id: &str) {
        self.expire(now);
        if let Some(g) = self.guests.get_mut(id) {
            g.hiding.get_or_insert(now);
        }
    }

    /// Drop the guests that have faded out and the rings that are over.
    fn expire(&mut self, now: f64) {
        self.guests.retain(|_, g| !g.gone(now));
        for g in self.guests.values_mut() {
            g.flashes.retain(|f| now < f.start + f.kind.duration());
        }
    }

    /// One frame at `now`: pointers glide (or jump, with reduced motion) toward their targets.
    /// `zoom` turns page units into screen points for "close enough". Whether anything still
    /// moves, fades or rings.
    pub fn step(&mut self, now: f64, reduce: bool, zoom: f64) -> bool {
        self.expire(now);
        let dt = self.last.map_or(0.0, |l| (now - l).clamp(0.0, 0.1));
        self.last = Some(now);
        let k = if reduce { 1.0 } else { 1.0 - (-dt * GLIDE).exp() };
        let calm_k = 1.0 - (-dt * WOBBLE_BLEND).exp();
        let mut busy = false;
        for g in self.guests.values_mut() {
            let (tx, ty) = g.target;
            g.shown.0 += (tx - g.shown.0) * k;
            g.shown.1 += (ty - g.shown.1) * k;
            if ((tx - g.shown.0) * zoom).hypot((ty - g.shown.1) * zoom) < 0.5 {
                g.shown = g.target;
                g.arrived.get_or_insert(now);
            } else {
                busy = true;
            }
            let rest = !reduce && g.arrived.is_some_and(|a| now - a >= WOBBLE_AFTER);
            g.calm += (f32::from(u8::from(rest)) - g.calm) * calm_k as f32;
            // A pointer at rest drifts, and a fading or ringing one changes every frame; a still
            // one under reduced motion only waits for its fade (see `next_change`).
            busy |= !reduce || g.opacity(now) < 1.0 || !g.flashes.is_empty();
        }
        busy
    }

    /// When a still scene next looks different: the earliest fade-out.
    pub fn next_change(&self, now: f64) -> Option<f64> {
        self.guests.values().map(|g| (g.fade_out_at() - now).max(0.0)).min_by(f64::total_cmp)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Guest)> {
        self.guests.iter().map(|(id, g)| (id.as_str(), g))
    }
}

/// Where a pointer shows: on the canvas at its tip, or at the canvas edge pointing `dir` toward
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Placement {
    OnCanvas(Pos2),
    Edge { at: Pos2, dir: Vec2 },
}

pub fn placement(canvas: Rect, tip: Pos2) -> Placement {
    if canvas.contains(tip) {
        return Placement::OnCanvas(tip);
    }
    let inner = canvas.shrink(EDGE_INSET);
    let dir = (tip - inner.center()).normalized();
    // Along the ray from the centre toward the pointer, the first point on the inner edge.
    let half = inner.size() / 2.0;
    let reach = [(dir.x.abs() > 1e-6).then(|| half.x / dir.x.abs()), (dir.y.abs() > 1e-6).then(|| half.y / dir.y.abs())]
        .into_iter()
        .flatten()
        .fold(f32::INFINITY, f32::min);
    Placement::Edge { at: inner.center() + dir * reach.min((tip - inner.center()).length()), dir }
}

/// The guests on the canvas: rings on the nodes they touch, their pointers and name tags, and an
/// edge arrow for a pointer outside the view. The view never moves to follow them.
pub fn draw(app: &mut FigApp, painter: &egui::Painter, ctx: &egui::Context) {
    if app.ui.guests.is_empty() {
        return;
    }
    let now = app.ui.guests.now();
    let reduce = app.reduce_motion();
    let Ok(ds) = app.session.ds() else { return };
    let (doc, page, zoom) = (ds.doc.clone(), ds.page, ds.view.zoom.max(1e-9));
    let busy = app.ui.guests.step(now, reduce, zoom);
    let canvas = app.canvas.rect;
    for (_, g) in app.ui.guests.iter().filter(|(_, g)| g.page.is_none_or(|p| p == page)) {
        let mut p = painter.clone();
        p.multiply_opacity(g.opacity(now));
        for (node, kind, start) in g.flashes() {
            if doc.page_of(node) == Some(page)
                && let Some(look) = ring_look(kind, now - start, reduce)
            {
                ring(app, &p, &doc, node, look, g.color);
            }
        }
        match placement(canvas, app.to_screen(g.shown.0, g.shown.1) + g.wobble(now)) {
            Placement::OnCanvas(tip) => {
                crate::live::pointer(&p, tip, g.color);
                crate::live::name_tag(&p, tip + vec2(12.0, 16.0), Align2::LEFT_TOP, &g.name, g.color);
            }
            Placement::Edge { at, dir } => edge_arrow(&p, at, dir, &g.name, g.color),
        }
    }
    if busy {
        ctx.request_repaint();
    } else if let Some(wait) = app.ui.guests.next_change(now) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait));
    }
}

/// A ring around `node`, `look.out` points further out than its resting place.
fn ring(app: &FigApp, painter: &egui::Painter, doc: &Document, node: NodeId, look: RingLook, color: Color32) {
    let Some([a, b, c, d]) = crate::canvas::node_quad(app, doc, node) else { return };
    // A node without width or height still gets a ring around its point.
    let unit = |w: Vec2, fallback: Vec2| if w.length() > 1e-3 { w / w.length() } else { fallback };
    let (u, v) = (unit(b - a, Vec2::X), unit(d - a, Vec2::Y));
    let pad = RING_PAD + look.out;
    let q = [a - (u + v) * pad, b + (u - v) * pad, c + (u + v) * pad, d - (u - v) * pad];
    if let Some(s) = look.scan {
        let band = 0.18;
        let (top, bottom) = ((s - band).max(0.0), s);
        let at = |t: f32, left: Pos2, right: Pos2| (left + (q[3] - q[0]) * t, right + (q[2] - q[1]) * t);
        let (tl, tr) = at(top, q[0], q[1]);
        let (bl, br) = at(bottom, q[0], q[1]);
        painter.add(Shape::convex_polygon(vec![tl, tr, br, bl], color.gamma_multiply(0.2 * look.alpha), Stroke::NONE));
    }
    painter.add(Shape::closed_line(q.to_vec(), Stroke::new(2.0, color.gamma_multiply(look.alpha))));
}

/// A small arrow at the canvas edge pointing `dir` toward an off-screen pointer, with the name.
fn edge_arrow(painter: &egui::Painter, at: Pos2, dir: Vec2, name: &str, color: Color32) {
    let side = dir.rot90();
    let tip = at + dir * 8.0;
    painter.add(Shape::convex_polygon(vec![tip, at - dir * 4.0 + side * 6.0, at - dir * 4.0 - side * 6.0], color, Stroke::new(1.0, Color32::WHITE)));
    let (anchor, align) = if dir.x.abs() >= dir.y.abs() {
        (at - vec2(dir.x.signum() * 12.0, 0.0), if dir.x > 0.0 { Align2::RIGHT_CENTER } else { Align2::LEFT_CENTER })
    } else {
        (at - vec2(0.0, dir.y.signum() * 12.0), if dir.y > 0.0 { Align2::CENTER_BOTTOM } else { Align2::CENTER_TOP })
    };
    crate::live::name_tag(painter, anchor, align, name, color);
}

/// The guests as `ui.inspect` reports them.
pub fn inspect(app: &FigApp) -> Value {
    let guests = &app.ui.guests;
    let now = guests.now();
    let page = app.session.ds().ok().map(|ds| ds.page);
    let rows = guests.iter().filter(|(_, g)| !g.gone(now)).map(|(id, g)| {
        let here = g.page.is_none_or(|p| Some(p) == page);
        let on_canvas = here && matches!(placement(app.canvas.rect, app.to_screen(g.shown.0, g.shown.1)), Placement::OnCanvas(_));
        json!({
            "id": id,
            "name": g.name,
            "color": format!("#{:02x}{:02x}{:02x}", g.color.r(), g.color.g(), g.color.b()),
            "page": g.page.map(|p| p.to_string()),
            "target": [g.target.0, g.target.1],
            "shown": [g.shown.0, g.shown.1],
            "opacity": g.opacity(now),
            "onCanvas": on_canvas,
            "flashes": g.flashes().map(|(n, kind, _)| json!({"id": n.to_string(), "kind": if kind == FlashKind::Read { "read" } else { "write" }})).collect::<Vec<_>>(),
        })
    });
    Value::Array(rows.collect())
}
