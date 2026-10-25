//! The `issues` block: after each mutating tool call, the layout problems on the layers the call
//! touched, so an agent can fix them in its next call.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::Client;
use serde_json::{Value, json};

/// The (kind, layer id) pairs in a result's `issues`.
fn issues(out: &Value) -> Vec<(String, Value)> {
    out["issues"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|i| {
                    assert!(i["name"].is_string() && i["message"].as_str().is_some_and(|m| !m.is_empty()), "{i}");
                    (i["kind"].as_str().unwrap_or_default().to_string(), i["id"].clone())
                })
                .collect()
        })
        .unwrap_or_default()
}

fn frame(c: &mut Client, props: Value) -> Value {
    c.create(json!({"type": "frame", "x": 0, "y": 0, "width": 400, "height": 200, "props": props}))
}

/// A text layer on its own line in `parent`, below the layers already there.
fn text(c: &mut Client, parent: &Value, chars: &str, props: Value) -> Value {
    let above = c.node(parent)["children"].as_array().map_or(0, Vec::len);
    c.ok("create_node", json!({"type": "text", "parent": parent, "x": 16, "y": 16 + 44 * above, "text": chars, "props": props}))
}

#[test]
fn readable_text_reports_nothing() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let out = text(&mut c, &f, "Hello", json!({"fill": "#111111", "fontSize": 16}));
    assert!(out.get("issues").is_none(), "{out}");
}

#[test]
fn text_without_a_visible_fill_is_invisible_text() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let none = text(&mut c, &f, "No fill", json!({"fills": []}));
    assert_eq!(issues(&none), [("invisible_text".to_string(), none["id"].clone())]);
    let clear = text(&mut c, &f, "Clear fill", json!({"fill": "#11111100"}));
    assert_eq!(issues(&clear), [("invisible_text".to_string(), clear["id"].clone())]);
}

#[test]
fn text_too_close_to_its_background_is_low_contrast() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let pale = text(&mut c, &f, "Pale", json!({"fill": "#DDDDDD", "fontSize": 16}));
    assert_eq!(issues(&pale), [("low_contrast".to_string(), pale["id"].clone())]);
    let message = pale["issues"][0]["message"].as_str().unwrap();
    assert!(message.contains("4.5"), "body text needs 4.5:1: {message}");
    // #949494 on white is about 3:1: enough for large text, not for body text.
    let large = text(&mut c, &f, "Large", json!({"fill": "#949494", "fontSize": 32}));
    assert!(large.get("issues").is_none(), "{large}");
    let small = text(&mut c, &f, "Small", json!({"fill": "#949494", "fontSize": 14}));
    assert_eq!(issues(&small), [("low_contrast".to_string(), small["id"].clone())]);
}

#[test]
fn background_is_the_nearest_painted_ancestor_else_the_page() {
    let mut c = Client::new();
    let dark = frame(&mut c, json!({"fill": "#111827"}));
    let clear = c.create(json!({"type": "frame", "parent": dark, "x": 0, "y": 0, "width": 300, "height": 150, "props": {"fills": []}}));
    let white = text(&mut c, &clear, "On dark", json!({"fill": "#FFFFFF"}));
    assert!(white.get("issues").is_none(), "white on the dark grandparent reads well: {white}");
    let black = text(&mut c, &clear, "Lost", json!({"fill": "#000000"}));
    assert_eq!(issues(&black), [("low_contrast".to_string(), black["id"].clone())]);
    // Straight on the page (#F5F5F5 by default).
    let on_page = c.ok("create_node", json!({"type": "text", "x": 600, "y": 0, "text": "Page", "props": {"fill": "#EEEEEE"}}));
    assert_eq!(issues(&on_page), [("low_contrast".to_string(), on_page["id"].clone())]);
}

#[test]
fn a_layer_under_the_text_is_its_background() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    c.create(json!({"type": "rectangle", "parent": f, "x": 0, "y": 0, "width": 200, "height": 80, "props": {"fill": "#1E3A8A"}}));
    let label = text(&mut c, &f, "Button", json!({"fill": "#FFFFFF"}));
    assert!(label.get("issues").is_none(), "white on the blue rectangle reads well: {label}");
}

#[test]
fn deleting_the_layer_under_the_text_reports_the_text_it_leaves_unreadable() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let rect = c.create(json!({"type": "rectangle", "parent": f, "x": 0, "y": 0, "width": 200, "height": 80, "props": {"fill": "#1E3A8A"}}));
    let label = text(&mut c, &f, "Button", json!({"fill": "#FFFFFF"}))["id"].clone();
    let out = c.ok("run_command", json!({"command": "edit.delete", "params": {"ids": [rect]}}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), label)], "white text left on the white frame: {out}");
}

#[test]
fn moving_a_light_layer_under_the_text_reports_the_text() {
    let mut c = Client::new();
    let card = frame(&mut c, json!({"fill": "#111827"}));
    let label = text(&mut c, &card, "White", json!({"fill": "#FFFFFF"}))["id"].clone();
    let aside = c.create(json!({"type": "rectangle", "parent": card, "x": 300, "y": 120, "width": 80, "height": 60, "props": {"fill": "#FFFFFF"}}));
    let cover = c.create(json!({"type": "rectangle", "parent": card, "x": 0, "y": 0, "width": 400, "height": 200, "props": {"fill": "#FFFFFF"}}));
    let out = c.ok("run_command", json!({"command": "node.reorder", "params": {"id": cover, "index": 0}}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), label.clone())], "white text now on the white rectangle: {out}");
    let apart = c.ok("run_command", json!({"command": "node.reorder", "params": {"id": aside, "index": 0}}));
    assert!(apart.get("issues").is_none(), "a reorder reports only the layers whose stacking it changed: {apart}");
}

#[test]
fn a_new_page_background_reports_the_text_on_the_page() {
    let mut c = Client::new();
    let on_page = c.ok("create_node", json!({"type": "text", "x": 0, "y": 0, "text": "Page", "props": {"fill": "#333333"}}));
    assert!(on_page.get("issues").is_none(), "{on_page}");
    let out = c.ok("run_command", json!({"command": "page.setBackground", "params": {"color": "#222222"}}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), on_page["id"].clone())], "{out}");
}

#[test]
fn a_divider_on_a_clipping_frame_edge_is_not_clipped() {
    let mut c = Client::new();
    let card = c.create(json!({"type": "frame", "width": 100, "height": 40, "props": {"fill": "#FFFFFF"}}));
    let rule = |x: f64, y: f64, w: f64, h: f64| json!({"type": "rectangle", "parent": card, "x": x, "y": y, "width": w, "height": h, "props": {"fill": "#111111"}});
    let line = |y: f64| json!({"type": "line", "parent": card, "x": 0, "y": y, "width": 100, "props": {"stroke": "#111111", "strokeWeight": 2}});
    for (what, args) in [
        ("a 1-unit rule on the top edge", rule(0.0, 0.0, 100.0, 1.0)),
        ("a 1-unit rule on the bottom edge", rule(0.0, 39.0, 100.0, 1.0)),
        ("a 1-unit bar on the left edge", rule(0.0, 0.0, 1.0, 40.0)),
        ("a 1-unit bar on the right edge", rule(99.0, 0.0, 1.0, 40.0)),
        ("a half-unit hairline on the top edge", rule(0.0, 0.0, 100.0, 0.5)),
        ("a line half a unit below the top edge", line(0.5)),
        ("a line on the top edge, half its stroke inside", line(0.0)),
        ("a line on the bottom edge, half its stroke inside", line(40.0)),
    ] {
        let out = c.ok("create_node", args);
        assert!(out.get("issues").is_none(), "{what} draws inside the frame: {out}");
    }
}

#[test]
fn dividers_at_the_ends_of_a_clipping_auto_layout_list_are_not_clipped() {
    let mut c = Client::new();
    let rule = |w: u32, h: u32| json!({"command": "node.create", "params": {"type": "rectangle", "parent": {"$step": 0}, "width": w, "height": h, "props": {"fill": "#E5E7EB"}}});
    let list = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "params": {"type": "frame", "name": "List", "width": 368, "props": {"fill": "#FFFFFF", "layout": "vertical", "sizingV": "hug"}}},
            rule(368, 1), rule(368, 40), rule(368, 1), rule(368, 40), rule(368, 1),
        ]}),
    );
    assert_eq!(list.as_array().map(Vec::len), Some(6), "horizontal dividers first, between and last draw in full: {list}");
    let row = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "params": {"type": "frame", "name": "Row", "x": 400, "height": 40, "props": {"fill": "#FFFFFF", "layout": "horizontal", "sizingH": "hug"}}},
            rule(1, 40), rule(80, 40), rule(1, 40), rule(80, 40), rule(1, 40),
        ]}),
    );
    assert_eq!(row.as_array().map(Vec::len), Some(6), "vertical dividers first, between and last draw in full: {row}");
    assert_eq!(c.ok("list_issues", json!({})), json!({"issues": []}));
}

#[test]
fn text_a_clipping_frame_cuts_is_clipped_unless_clip_is_off() {
    let mut c = Client::new();
    let f = c.create(json!({"type": "frame", "width": 200, "height": 60, "props": {"fill": "#FFFFFF"}}));
    let cut = c.ok("create_node", json!({"type": "text", "parent": f, "x": 8, "y": 40, "text": "Cut off", "props": {"fontSize": 32}}));
    assert_eq!(issues(&cut), [("clipped".to_string(), cut["id"].clone())]);
    let open = c.create(json!({"type": "frame", "x": 300, "width": 200, "height": 60, "props": {"fill": "#FFFFFF", "clip": false}}));
    let spills = c.ok("create_node", json!({"type": "text", "parent": open, "x": 8, "y": 40, "text": "Spills", "props": {"fontSize": 32}}));
    assert!(spills.get("issues").is_none(), "a frame that doesn't clip cuts nothing: {spills}");
}

#[test]
fn a_shape_is_clipped_only_when_none_of_it_shows() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let bleed = c.ok("create_node", json!({"type": "ellipse", "parent": f, "x": 350, "y": -50, "width": 100, "height": 100}));
    assert!(bleed.get("issues").is_none(), "a shape bleeding off the edge is a design choice: {bleed}");
    let lost = c.ok("create_node", json!({"type": "rectangle", "parent": f, "x": 500, "y": 0, "width": 50, "height": 50}));
    assert_eq!(issues(&lost), [("clipped".to_string(), lost["id"].clone())]);
    let below = c.ok("create_node", json!({"type": "rectangle", "parent": f, "x": 0, "y": 200, "width": 400, "height": 1}));
    assert_eq!(issues(&below), [("clipped".to_string(), below["id"].clone())], "a rule just past the bottom edge shows nothing");
    let past =
        c.ok("create_node", json!({"type": "line", "parent": f, "x": 0, "y": 220, "width": 400, "props": {"stroke": "#111111", "strokeWeight": 2}}));
    assert_eq!(issues(&past), [("clipped".to_string(), past["id"].clone())], "a line below the frame, stroke and all");
    // A thumbnail that crops a larger picture hides parts of it on purpose.
    let thumb = c.create(json!({"type": "frame", "x": 600, "width": 64, "height": 64}));
    let picture = c.create(json!({"type": "frame", "parent": thumb, "x": -30, "width": 360, "height": 200}));
    let sun = c.ok("create_node", json!({"type": "ellipse", "parent": picture, "x": 240, "y": 30, "width": 40, "height": 40}));
    assert!(sun.get("issues").is_none(), "{sun}");
}

#[test]
fn a_frame_with_no_size_is_collapsed() {
    let mut c = Client::new();
    let empty = c.ok("create_node", json!({"type": "frame", "props": {"layout": "vertical", "sizingH": "hug", "sizingV": "hug"}}));
    assert_eq!(issues(&empty), [("collapsed".to_string(), empty["id"].clone())]);
    let list = c.create(json!({"type": "frame", "x": 300, "width": 200, "height": 100, "props": {"layout": "vertical"}}));
    let slot = c.ok("create_node", json!({"type": "frame", "parent": list, "props": {"layout": "vertical", "sizingH": "hug", "sizingV": "hug"}}));
    assert_eq!(issues(&slot), [("collapsed".to_string(), slot["id"].clone())], "an empty slot on the list's top edge is not outside it");
    let line = c.ok("create_node", json!({"type": "line", "x": 0, "y": 300, "width": 200}));
    assert!(line.get("issues").is_none(), "a line has no height by nature: {line}");
}

#[test]
fn characters_no_font_draws_are_missing_glyphs() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let out = text(&mut c, &f, "Price \u{10FFFD}", json!({}));
    if c.node(&out["id"])["width"].as_f64().unwrap_or(0.0) <= 1.0 {
        eprintln!("no fonts: nothing lays out, so no glyph is missing (CI has no fonts); skipping");
        return;
    }
    assert_eq!(issues(&out), [("missing_glyph".to_string(), out["id"].clone())]);
    assert!(out["issues"][0]["message"].as_str().unwrap().contains("U+10FFFD"), "{out}");
}

#[test]
fn set_properties_reports_on_the_layers_inside_a_repainted_frame() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let t = text(&mut c, &f, "Was readable", json!({"fill": "#000000"}))["id"].clone();
    let out = c.ok("set_properties", json!({"ids": [f], "props": {"fill": "#000000"}}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), t)]);
}

#[test]
fn issues_on_layers_the_call_did_not_touch_are_not_repeated() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let ghost = text(&mut c, &f, "Ghost", json!({"fills": []}))["id"].clone();
    let other = c.ok("create_node", json!({"type": "rectangle", "x": 600, "y": 0, "width": 40, "height": 40}));
    assert!(other.get("issues").is_none(), "{other}");
    let edited = c.ok("set_properties", json!({"ids": [ghost], "props": {"characters": "Still a ghost"}}));
    assert_eq!(issues(&edited), [("invisible_text".to_string(), ghost.clone())], "a layer the call changed is reported as it is now");
}

#[test]
fn run_command_reports_issues_too() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let t = text(&mut c, &f, "Fine", json!({"fill": "#000000"}))["id"].clone();
    let out = c.ok("run_command", json!({"command": "node.set", "params": {"ids": [t], "props": {"fill": "#FAFAFA"}}}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), t)]);
}

#[test]
fn batch_returns_results_and_issues_only_when_there_are_issues() {
    let mut c = Client::new();
    let broken = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "params": {"type": "frame", "name": "Card", "width": 240, "height": 60, "props": {"fill": "#FFFFFF"}}},
            {"command": "node.create", "params": {"type": "text", "parent": {"$step": 0}, "x": 8, "y": 8, "text": "Faint", "props": {"fill": "#EEEEEE"}}},
            {"command": "node.create", "params": {"type": "text", "parent": {"$step": 0}, "x": 8, "y": 40, "text": "Cut", "props": {"fontSize": 40}}},
        ]}),
    );
    let results = broken["results"].as_array().unwrap_or_else(|| panic!("an object with results: {broken}"));
    assert_eq!(results.len(), 3);
    let mut found = issues(&broken);
    found.sort_by_key(|(kind, _)| kind.clone());
    assert_eq!(found, [("clipped".to_string(), results[2]["id"].clone()), ("low_contrast".to_string(), results[1]["id"].clone())]);

    let fixed = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "params": {"type": "frame", "name": "Card", "x": 400, "width": 240, "height": 50,
                "props": {"fill": "#FFFFFF", "layout": "vertical", "padding": 8, "gap": 4, "sizingV": "hug"}}},
            {"command": "node.create", "params": {"type": "text", "parent": {"$step": 0}, "text": "Clear", "props": {"fill": "#333333"}}},
            {"command": "node.create", "params": {"type": "text", "parent": {"$step": 0}, "text": "Whole", "props": {"fontSize": 40}}},
        ]}),
    );
    assert_eq!(fixed.as_array().map(Vec::len), Some(3), "no issues: the plain array of results: {fixed}");
}

#[test]
fn a_named_batch_with_issues_returns_results_names_and_issues_side_by_side() {
    let mut c = Client::new();
    let out = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "as": "card", "params": {"type": "frame", "name": "Card", "width": 240, "height": 60, "props": {"fill": "#FFFFFF"}}},
            {"command": "node.create", "as": "faint", "params": {"type": "text", "parent": {"$name": "card"}, "x": 8, "y": 8, "text": "Faint", "props": {"fill": "#EEEEEE"}}},
        ]}),
    );
    let mut keys: Vec<&str> = out.as_object().unwrap_or_else(|| panic!("an object: {out}")).keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(keys, ["issues", "names", "results"], "{out}");
    let results = out["results"].as_array().unwrap_or_else(|| panic!("results is the flat array: {out}"));
    assert_eq!(results.len(), 2, "{out}");
    assert_eq!(out["names"], json!({"card": results[0]["id"], "faint": results[1]["id"]}));
    assert_eq!(issues(&out), [("low_contrast".to_string(), results[1]["id"].clone())]);

    let clean = c.ok(
        "batch",
        json!({"commands": [
            {"command": "node.create", "as": "card", "params": {"type": "frame", "name": "Card", "x": 400, "width": 240, "height": 60, "props": {"fill": "#FFFFFF"}}},
        ]}),
    );
    assert!(clean.get("issues").is_none() && clean["names"]["card"] == clean["results"][0]["id"], "names without issues: {clean}");
}

#[test]
fn x_and_y_inside_auto_layout_are_a_warning() {
    let mut c = Client::new();
    let row = c.create(json!({"type": "frame", "width": 300, "height": 60, "props": {"layout": "horizontal"}}));
    let placed = c.ok("create_node", json!({"type": "rectangle", "parent": row, "x": 40, "y": 10, "width": 20, "height": 20}));
    let warnings = placed["warnings"].to_string();
    assert!(warnings.contains("auto layout"), "{placed}");
    let moved = c.ok("set_properties", json!({"ids": [placed["id"]], "props": {"x": 100}}));
    assert!(moved["warnings"].to_string().contains("auto layout"), "{moved}");
    let free = c.ok("set_properties", json!({"ids": [placed["id"]], "props": {"absolute": true, "x": 100}}));
    assert!(free.get("warnings").is_none(), "an absolute child goes where x and y say: {free}");
    let anywhere = c.ok("create_node", json!({"type": "rectangle", "x": 500, "y": 10, "width": 20, "height": 20}));
    assert!(anywhere.get("warnings").is_none(), "{anywhere}");
}

#[test]
fn list_issues_reports_the_whole_page_or_the_given_layers() {
    let mut c = Client::new();
    let f = frame(&mut c, json!({"fill": "#FFFFFF"}));
    let ghost = text(&mut c, &f, "Ghost", json!({"fills": []}))["id"].clone();
    let pale = text(&mut c, &f, "Pale", json!({"fill": "#EEEEEE"}))["id"].clone();
    let all = c.ok("list_issues", json!({}));
    let mut found = issues(&all);
    found.sort_by_key(|(kind, _)| kind.clone());
    assert_eq!(found, [("invisible_text".to_string(), ghost.clone()), ("low_contrast".to_string(), pale)]);
    let one = c.ok("list_issues", json!({"ids": [ghost]}));
    assert_eq!(issues(&one), [("invisible_text".to_string(), ghost)]);
    let ok = c.create(json!({"type": "rectangle", "x": 600, "y": 0, "width": 40, "height": 40}));
    assert_eq!(c.ok("list_issues", json!({"ids": [ok]})), json!({"issues": []}));
}
