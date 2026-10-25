//! Layout issues: what a person would see wrong in the drawn design. Text that doesn't show or
//! reads poorly against what is behind it, layers a clipping frame cuts off, frames with no size,
//! and characters no font draws. [`find`] checks a set of layers; [`touched`] says which layers an
//! edit can have changed the issues of. A programmatic caller gets them after each edit
//! (`engine.execute` with `issues: true`, see [`Session::execute_checked`](crate::Session::execute_checked))
//! and on demand with `doc.issues`.

use std::collections::BTreeSet;

use figcraft_color::{Color, Paint};
use figcraft_doc::{Document, Node, NodeId, NodeKind, TextStyle, style_weight};
use figcraft_geom::Rect;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    /// A frame, section or text less than a unit wide or tall.
    Collapsed,
    /// Text with no visible fill.
    InvisibleText,
    /// Text below the WCAG AA contrast against what is behind it.
    LowContrast,
    /// Text a clipping frame cuts, or a layer the nearest clipping frame around it hides entirely.
    Clipped,
    /// Characters no font draws (they show as boxes).
    MissingGlyph,
}

/// One issue on one layer.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub id: NodeId,
    /// The layer's name.
    pub name: String,
    pub kind: IssueKind,
    /// What is wrong and what to change, in one sentence.
    pub message: String,
}

/// Layout rounding reports sizes to the unit: one unit of overflow or size is not an issue
/// (`scripts/canvas-eval/checks.ts` in Termul uses the same slack).
const SLACK: f64 = 1.0;

type Check = fn(&Document, &Node) -> Option<String>;

/// Every check, with the kind it reports.
const CHECKS: &[(IssueKind, Check)] = &[
    (IssueKind::Collapsed, collapsed),
    (IssueKind::InvisibleText, invisible_text),
    (IssueKind::LowContrast, low_contrast),
    (IssueKind::Clipped, clipped),
    (IssueKind::MissingGlyph, missing_glyph),
];

/// The issues on `ids`, in id order. Hidden layers, and layers inside hidden ones, have none.
pub fn find(doc: &Document, ids: &BTreeSet<NodeId>) -> Vec<Issue> {
    let mut out = Vec::new();
    for &id in ids {
        let Some(n) = doc.node(id) else { continue };
        if n.is_page() || !shown(doc, n) {
            continue;
        }
        for (kind, check) in CHECKS {
            if let Some(message) = check(doc, n) {
                out.push(Issue { id, name: n.name.clone(), kind: *kind, message });
            }
        }
    }
    out
}

/// The layers whose issues the edit from `before` to `doc` can have changed: each layer it added
/// or changed (the layout's own moves and resizes included); everything inside a layer it added,
/// moved, repainted, shrank or re-clipped; and the layers above one it added, moved, repainted or
/// resized that overlap it, or that overlapped one it deleted, with their contents; and every layer
/// on a page whose background it changed.
pub fn touched(doc: &Document, before: &Document) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    let background = |n: &Node| match n.kind {
        NodeKind::Page { background, .. } => Some(background),
        _ => None,
    };
    for id in doc.changed_ids(before, usize::MAX).unwrap_or_default() {
        let Some(n) = doc.node(id) else {
            // Deleted: the layers that were above it no longer have it behind them. Under a deleted
            // parent they are gone too, and scanning them would cost a bulk delete O(n²).
            let parent_gone = before.node(id).and_then(|o| o.parent).is_some_and(|p| doc.node(p).is_none());
            if !parent_gone && let Some(area) = before.world_bounds(id) {
                out.extend(stacked_on(before, id, area).into_iter().flat_map(|above| doc.subtree(above)));
            }
            continue;
        };
        if n.is_page() {
            if before.node(id).is_some_and(|o| background(o) != background(n)) {
                out.extend(doc.subtree(id));
            }
            continue;
        }
        out.insert(id);
        let old = before.node(id);
        let moved = old.is_none_or(|o| o.parent != n.parent || o.transform != n.transform);
        let painted = old.is_none_or(|o| o.fills != n.fills || o.opacity != n.opacity || o.visible != n.visible);
        let shrank = old.is_some_and(|o| n.width < o.width || n.height < o.height);
        let reclipped = old.is_some_and(|o| n.clip != o.clip);
        let resized = old.is_some_and(|o| n.width != o.width || n.height != o.height);
        if moved || painted || shrank || reclipped {
            out.extend(doc.subtree(id));
        }
        if !(moved || painted || resized) {
            continue;
        }
        // Under a new parent every sibling is new and reports itself with its subtree.
        let parent_new = n.parent.is_some_and(|p| before.node(p).is_none());
        if !parent_new && let Some(area) = doc.world_bounds(id) {
            out.extend(stacked_on(doc, id, area).into_iter().flat_map(|above| doc.subtree(above)));
        }
    }
    out
}

/// The siblings drawn above `id` in `doc` that overlap `area`.
fn stacked_on(doc: &Document, id: NodeId, area: Rect) -> Vec<NodeId> {
    let Some(parent) = doc.node(id).and_then(|n| n.parent).and_then(|p| doc.node(p)) else { return Vec::new() };
    parent.children.iter().skip_while(|c| **c != id).skip(1).copied().filter(|&c| doc.world_bounds(c).is_some_and(|b| overlap(b, area))).collect()
}

fn shown(doc: &Document, n: &Node) -> bool {
    let visible = |n: &Node| n.visible && n.opacity > 0.0;
    visible(n) && doc.ancestors(n.id).iter().filter_map(|a| doc.node(*a)).all(visible)
}

fn overlap(a: Rect, b: Rect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

fn covers(outer: Rect, inner: Rect) -> bool {
    outer.x0 <= inner.x0 && outer.y0 <= inner.y0 && outer.x1 >= inner.x1 && outer.y1 >= inner.y1
}

fn units(v: f64) -> String {
    format!("{}", (v * 10.0).round() / 10.0)
}

fn collapsed(_: &Document, n: &Node) -> Option<String> {
    let sized = n.kind.is_frame_like() || matches!(n.kind, NodeKind::Section { .. } | NodeKind::Text(_));
    (sized && (n.width < SLACK || n.height < SLACK)).then(|| {
        format!("It is {} × {}, so it doesn't show: give it a width and height, or content (and padding) to hug.", units(n.width), units(n.height))
    })
}

/// The text's spans that have something to draw, each with the paints it draws in.
fn inked_spans(n: &Node) -> Vec<(&TextStyle, &[Paint])> {
    let Some(t) = n.kind.text() else { return Vec::new() };
    let chars: Vec<char> = t.characters.chars().collect();
    t.spans()
        .into_iter()
        .filter(|(start, end, _)| chars.get(*start..*end).is_some_and(|s| s.iter().any(|c| !c.is_whitespace())))
        .map(|(_, _, style)| (style, style.fills.as_deref().unwrap_or(&n.fills)))
        .collect()
}

fn paints(p: &Paint) -> bool {
    p.visible && p.opacity > 0.0 && p.solid_color().is_none_or(|c| c.a > 0.0)
}

fn invisible_text(_: &Document, n: &Node) -> Option<String> {
    inked_spans(n)
        .iter()
        .any(|(_, fills)| !fills.iter().any(paints))
        .then(|| "Its text has no visible fill, so it doesn't show: set `fill` to a colour that stands out from its background.".to_string())
}

/// `top` drawn over `under`.
fn over(top: Color, under: Color) -> Color {
    let a = top.a + under.a * (1.0 - top.a);
    if a <= 0.0 {
        return Color::TRANSPARENT;
    }
    let mix = |t: f32, u: f32| (t * top.a + u * under.a * (1.0 - top.a)) / a;
    Color::new(mix(top.r, under.r), mix(top.g, under.g), mix(top.b, under.b), a)
}

/// The one colour `fills` paint, at `opacity`: transparent when none shows, `None` when one is
/// not a solid colour.
fn solid(fills: &[Paint], opacity: f32) -> Option<Color> {
    let mut acc = Color::TRANSPARENT;
    for p in fills.iter().filter(|p| paints(p)) {
        acc = over(p.solid_color()?, acc);
    }
    Some(acc.with_alpha(acc.a * opacity))
}

/// The colour behind `n`: what the layers under it paint there, from the nearest out to the first
/// opaque one (its lower siblings, its parent, their lower siblings, …, the page). `None` when it
/// can't be told ([`behind`]).
fn backdrop(doc: &Document, n: &Node) -> Option<Color> {
    let area = doc.world_bounds(n.id)?;
    let opaque = |layers: &[Color]| layers.last().is_some_and(|c| c.a >= 0.999);
    let mut layers: Vec<Color> = Vec::new();
    let mut cur = n;
    while !opaque(&layers) {
        let parent = doc.node(cur.parent?)?;
        let below: Vec<&Node> = parent.children.iter().take_while(|c| **c != cur.id).filter_map(|c| doc.node(*c)).collect();
        for b in below.into_iter().rev() {
            if !opaque(&layers) {
                layers.push(behind(doc, b, area)?);
            }
        }
        if !opaque(&layers) {
            layers.push(match &parent.kind {
                NodeKind::Page { background, .. } => *background,
                _ => solid(&parent.fills, 1.0)?,
            });
        }
        cur = parent;
    }
    Some(layers.iter().rev().fold(Color::TRANSPARENT, |under, top| over(*top, under)))
}

/// What `b`, a layer drawn before the text at `area`, paints behind it: transparent when it doesn't
/// reach the text or is text itself (glyphs make no background). `None` when it can't be told: a
/// gradient or image fill, a layer that isn't a box or covers only part of the text, or one with
/// layers of its own there.
fn behind(doc: &Document, b: &Node, area: Rect) -> Option<Color> {
    let reach = doc.world_bounds(b.id).filter(|r| overlap(*r, area));
    let Some(r) = reach.filter(|_| b.visible && b.opacity > 0.0 && !b.is_mask && !matches!(b.kind, NodeKind::Text(_))) else {
        return Some(Color::TRANSPARENT);
    };
    let boxy = b.kind.is_frame_like() || matches!(b.kind, NodeKind::Rectangle | NodeKind::Section { .. });
    let crowded = doc
        .subtree(b.id)
        .into_iter()
        .skip(1)
        .filter_map(|c| doc.node(c))
        .any(|c| c.visible && doc.world_bounds(c.id).is_some_and(|r| overlap(r, area)));
    if !boxy || !covers(r, area) || crowded {
        return None;
    }
    solid(&b.fills, b.opacity)
}

fn low_contrast(doc: &Document, n: &Node) -> Option<String> {
    let spans = inked_spans(n);
    if spans.is_empty() {
        return None;
    }
    let behind = backdrop(doc, n)?;
    let (ratio, need, ink) = spans
        .iter()
        .filter_map(|(style, fills)| {
            let ink = solid(fills, n.opacity).filter(|c| c.a > 0.0)?;
            let ink = over(ink, behind);
            // WCAG AA: large text is 24 px, or 18.66 px bold.
            let large = style.font_size >= 24.0 || (style.font_size >= 18.66 && style_weight(&style.font_style) >= 700);
            Some((ink.contrast(behind), if large { 3.0 } else { 4.5 }, ink))
        })
        .min_by(|a, b| (a.0 / a.1).total_cmp(&(b.0 / b.1)))?;
    (ratio < need).then(|| {
        format!(
            "Its text is #{} on #{}, a contrast of {:.2}:1, and text this size needs {need}:1: change its `fill` or the background.",
            ink.to_hex(),
            behind.to_hex(),
            ratio
        )
    })
}

/// Text any clipping frame around it cuts, or a layer the nearest clipping frame around it hides.
/// A frame further out that hides the layer crops something larger on purpose (an illustration in
/// a thumbnail), so only text counts there.
fn clipped(doc: &Document, n: &Node) -> Option<String> {
    let area = doc.world_bounds(n.id)?;
    let text = matches!(n.kind, NodeKind::Text(_));
    let clips = doc.ancestors(n.id).into_iter().filter_map(|a| doc.node(a)).filter(|a| a.clip && a.kind.is_frame_like());
    for (i, a) in clips.enumerate() {
        let Some(frame) = doc.world_bounds(a.id) else { continue };
        // The slack is at most the layer's own size: a line has no height, so it hides only past the edge.
        let (sx, sy) = (SLACK.min(area.width()), SLACK.min(area.height()));
        let hidden = area.x0 >= frame.x1 - sx || area.x1 <= frame.x0 + sx || area.y0 >= frame.y1 - sy || area.y1 <= frame.y0 + sy;
        if hidden && (i == 0 || text) {
            return Some(format!(
                "It lies outside “{}” ({}), which clips its content, so it doesn't show: move it inside, or turn off that frame's `clip`.",
                a.name, a.id.0
            ));
        }
        let cut = (frame.x0 - area.x0).max(frame.y0 - area.y0).max(area.x1 - frame.x1).max(area.y1 - frame.y1);
        if text && cut > SLACK {
            return Some(format!(
                "Its text is cut off by {} px where “{}” ({}) clips its content: make that frame larger or let it hug, or turn off its `clip`.",
                units(cut),
                a.name,
                a.id.0
            ));
        }
    }
    None
}

/// Characters that draw nothing even when a font has them.
fn blank(c: char) -> bool {
    c.is_whitespace()
        || c.is_control()
        || matches!(c, '\u{AD}' | '\u{200B}'..='\u{200F}' | '\u{2060}'..='\u{2064}' | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}')
}

fn missing_glyph(_: &Document, n: &Node) -> Option<String> {
    let t = n.kind.text()?;
    let (layout, _) = figcraft_text::layout_node(n)?;
    let chars: Vec<char> = t.characters.chars().collect();
    let mut missing: Vec<char> = Vec::new();
    for g in layout.glyphs.iter().filter(|g| g.gid == 0) {
        for &c in chars.get(g.char_start..g.char_end).unwrap_or(&[]) {
            if !blank(c) && !missing.contains(&c) {
                missing.push(c);
            }
        }
    }
    if missing.is_empty() {
        return None;
    }
    let listed: Vec<String> = missing.iter().take(5).map(|c| format!("“{c}” (U+{:04X})", u32::from(*c))).collect();
    let more = if missing.len() > listed.len() { format!(" and {} more", missing.len() - listed.len()) } else { String::new() };
    let shows = if missing.len() == 1 { "it shows as a box" } else { "they show as boxes" };
    Some(format!("No available font draws {}{more}, so {shows}: use other characters, or a font that has them.", listed.join(", ")))
}
