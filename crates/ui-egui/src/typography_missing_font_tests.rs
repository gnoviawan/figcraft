//! #1422: the Typography section marks a font family that isn't installed.
use egui::{Event, Pos2, RawInput, Rect, Shape, vec2};
use figcraft_engine::Session;
use serde_json::json;

use crate::{FigApp, NoServices};

/// The texts the section paints over a few frames with the pointer resting on `family`'s dropdown.
fn texts_hovering(family: &str) -> Vec<String> {
    let mut s = Session::new();
    let t = s.execute("text.create", &json!({"text": "Hi", "props": {"fontFamily": family}})).unwrap()["id"].clone();
    s.execute("select.set", &json!({"ids": [t]})).unwrap();
    let mut app = FigApp::new(s, Box::new(NoServices));
    let ctx = egui::Context::default();
    crate::theme::install_fonts(&ctx, false, 0);
    crate::theme::apply(&ctx, crate::theme::Tokens::light());
    let mut frame = |time: f64, events: Vec<Event>| -> Vec<(String, Pos2)> {
        let input =
            RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(280.0, 600.0))), time: Some(time), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let ds = app.session.ds().unwrap();
            let (doc, sel) = (ds.doc.clone(), ds.selection.clone());
            crate::typography::typography(&mut app, ui, &doc, &sel);
        });
        out.textures_delta.clear();
        out.shapes
            .into_iter()
            .filter_map(|c| match c.shape {
                Shape::Text(t) => Some((t.galley.text().to_string(), t.pos)),
                _ => None,
            })
            .collect()
    };
    let shown = frame(0.0, vec![]);
    let at = shown.iter().find(|(s, _)| s == family).map(|(_, p)| *p + vec2(4.0, 4.0)).unwrap_or_else(|| panic!("no {family} row: {shown:?}"));
    let mut seen = Vec::new();
    for k in 1..=60 {
        let events = if k == 1 { vec![Event::PointerMoved(at)] } else { vec![] };
        seen.extend(frame(f64::from(k) / 30.0, events).into_iter().map(|(s, _)| s));
    }
    seen
}

#[test]
fn a_family_that_is_not_installed_is_marked_and_an_installed_one_is_not() {
    assert!(texts_hovering("NoSuchFontXYZ").iter().any(|s| s == "Font not installed"));
    if figcraft_text::db().read().unwrap().has_family("Inter") {
        assert!(!texts_hovering("Inter").iter().any(|s| s == "Font not installed"));
    }
}
