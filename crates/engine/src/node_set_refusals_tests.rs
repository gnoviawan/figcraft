//! `node.set` doesn't store what can't apply: auto layout keys go only to layers that can have auto
//! layout (the others are named in `warnings`, and with none of them the call is refused), and a
//! `fontStyle` the family lacks says which face draws instead.

use serde_json::{Value, json};

use crate::{EngineError, Session};

fn ex(s: &mut Session, cmd: &str, p: Value) -> Value {
    s.execute(cmd, &p).unwrap_or_else(|e| panic!("{cmd}: {e}"))
}

fn id(v: &Value) -> crate::NodeId {
    crate::params::id_of(v).unwrap()
}

fn bad_msg(r: crate::Result<Value>) -> String {
    match r {
        Err(EngineError::BadParams { msg, .. }) => msg,
        other => panic!("expected BadParams, got {other:?}"),
    }
}

#[test]
fn auto_layout_keys_on_layers_without_auto_layout_are_refused() {
    let mut s = Session::new();
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi"}))["id"]);
    let r = id(&ex(&mut s, "node.create", json!({"type": "rectangle"}))["id"]);
    for (target, key, value) in [(t, "gap", json!(8)), (r, "padding", json!(4)), (t, "mainAlign", json!("center")), (r, "wrap", json!(true))] {
        let msg = bad_msg(s.execute("node.set", &json!({"ids": [target], "props": {key: value}})));
        assert!(msg.contains(key) && msg.contains("auto layout"), "{msg}");
        assert!(s.doc().unwrap().node(target).unwrap().layout.is_none(), "no hidden auto layout on {target}");
    }
    // Frames still take them, with or without a layout mode first.
    let f = id(&ex(&mut s, "node.create", json!({"type": "frame"}))["id"]);
    let out = ex(&mut s, "node.set", json!({"ids": [f], "props": {"gap": 8, "padding": 4}}));
    assert_eq!(s.doc().unwrap().node(f).unwrap().layout.as_ref().map(|l| l.gap), Some(8.0));
    assert!(out.get("warnings").is_none(), "{out}");
}

#[test]
fn with_several_layers_the_frames_take_auto_layout_keys_and_the_others_are_named() {
    let mut s = Session::new();
    let f = id(&ex(&mut s, "node.create", json!({"type": "frame", "name": "Card"}))["id"]);
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi", "name": "Title"}))["id"]);
    let out = ex(&mut s, "node.set", json!({"ids": [f, t], "props": {"gap": 8, "opacity": 0.5}}));
    let doc = s.doc().unwrap();
    assert_eq!(doc.node(f).unwrap().layout.as_ref().map(|l| l.gap), Some(8.0));
    assert!(doc.node(t).unwrap().layout.is_none(), "no hidden auto layout on the text");
    assert_eq!(doc.node(t).unwrap().opacity, 0.5, "the text still gets the other keys");
    let w = out["warnings"][0].as_str().unwrap();
    assert!(w.contains("`gap`") && w.contains(&format!("{t} “Title”")) && w.contains("TEXT"), "{w}");
    // One undo step for all of it.
    ex(&mut s, "edit.undo", json!({}));
    assert!(s.doc().unwrap().node(f).unwrap().layout.as_ref().is_none_or(|l| l.gap != 8.0));
}

#[test]
fn a_font_style_draws_in_the_nearest_face_and_says_so_when_it_differs() {
    figcraft_text::scan_system_fonts();
    // Any installed family with a regular and a bold face and nothing heavier (CI machines differ).
    let pick = figcraft_text::family_names().into_iter().find_map(|f| {
        let st = figcraft_text::family_styles(&f);
        let has = |w: u16| st.iter().any(|x| figcraft_doc::style_weight(x) == w && !x.to_lowercase().contains("italic"));
        (has(400) && has(700) && !st.iter().any(|x| figcraft_doc::style_weight(x) > 700)).then_some((f, st))
    });
    let Some((family, styles)) = pick else { return };
    let mut s = Session::new();
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi", "props": {"fontFamily": family}}))["id"]);
    let set = |s: &mut Session, style: &str| ex(s, "node.set", json!({"ids": [t], "props": {"fontStyle": style}}));
    // Any spelling of a style it has, and names that resolve to the weight they ask for, say nothing.
    let bold = styles.iter().find(|x| figcraft_doc::style_weight(x) == 700).unwrap().clone();
    for style in [bold.clone(), bold.to_uppercase(), bold.replace(' ', ""), "Roman".into(), "Normal".into()] {
        let out = set(&mut s, &style);
        assert!(out.get("warnings").is_none(), "{style}: {out}");
    }
    // A heavier weight than the family has applies (as a text layer resolves it) and says which face draws.
    let out = set(&mut s, "Black");
    let w = out["warnings"][0].as_str().unwrap();
    assert!(w.contains(&family) && w.contains("`Black`") && w.contains("nearest"), "{w}");
    assert_eq!(s.doc().unwrap().node(t).unwrap().kind.text().unwrap().spans()[0].2.font_style, "Black");
    // A family that isn't installed keeps any style name without a word (files from elsewhere name theirs).
    let out = ex(&mut s, "node.set", json!({"ids": [t], "props": {"fontFamily": "No Such Family 7f3a", "fontStyle": "Hairline"}}));
    assert!(out.get("warnings").is_none(), "{out}");
}

#[test]
fn a_refused_call_changes_nothing() {
    let mut s = Session::new();
    let t = id(&ex(&mut s, "node.create", json!({"type": "text", "text": "Hi"}))["id"]);
    let before = s.doc().unwrap().node(t).unwrap().fills.clone();
    // `fill` comes before `gap` (keys apply in order), so this checks the rollback.
    assert!(s.execute("node.set", &json!({"ids": [t], "props": {"fill": "#FF0000", "gap": 8}})).is_err());
    assert_eq!(s.doc().unwrap().node(t).unwrap().fills, before);
}

fn frame_with_layout(s: &mut Session) -> crate::NodeId {
    let props = json!({"layout": "horizontal", "padding": 16, "sizingH": "fixed"});
    id(&ex(s, "node.create", json!({"type": "frame", "width": 400, "height": 100, "props": props}))["id"])
}

#[test]
fn out_of_range_numbers_are_refused_naming_the_key_and_its_range() {
    let mut s = Session::new();
    let f = frame_with_layout(&mut s);
    let r = id(&ex(&mut s, "node.create", json!({"type": "rectangle", "width": 40, "height": 40}))["id"]);
    let cases = [
        (f, json!({"opacity": 1.5}), "opacity", "between 0 and 1"),
        (f, json!({"opacity": 5}), "opacity", "between 0 and 1"),
        (f, json!({"opacity": 80}), "opacity", "between 0 and 1"),
        (f, json!({"opacity": -0.1}), "opacity", "between 0 and 1"),
        (f, json!({"cornerSmoothing": 1.5}), "cornerSmoothing", "between 0 and 1"),
        (f, json!({"width": -50}), "width", "0 or more"),
        (f, json!({"height": -1}), "height", "0 or more"),
        (f, json!({"w": -1}), "w", "0 or more"),
        (f, json!({"width": "50%"}), "width", "0 or more"),
        (r, json!({"cornerRadius": -10}), "cornerRadius", "0 or more"),
        (r, json!({"cornerRadius": [4, -1, 0, 0]}), "cornerRadius", "0 or more"),
        (r, json!({"topLeftRadius": -1}), "topLeftRadius", "0 or more"),
        (r, json!({"strokeWeight": -1}), "strokeWeight", "0 or more"),
        (r, json!({"strokeTopWeight": -2}), "strokeTopWeight", "0 or more"),
        (r, json!({"strokeSides": [1, -1, 1, 1]}), "strokeSides", "0 or more"),
    ];
    for (target, props, key, range) in cases {
        let before = s.doc().unwrap().node(target).unwrap().clone();
        let msg = bad_msg(s.execute("node.set", &json!({"ids": [target], "props": props})));
        assert!(msg.contains(&format!("`{key}`")) && msg.contains(range), "{props}: {msg}");
        let after = s.doc().unwrap().node(target).unwrap();
        assert_eq!(
            (after.opacity, after.width, after.height, after.corners.radii, after.stroke.weight, after.stroke.sides),
            (before.opacity, before.width, before.height, before.corners.radii, before.stroke.weight, before.stroke.sides),
            "{props} changed the layer"
        );
    }
}

#[test]
fn in_range_numbers_and_explicit_percentages_apply() {
    let mut s = Session::new();
    let r = id(&ex(&mut s, "node.create", json!({"type": "rectangle", "width": 40, "height": 40}))["id"]);
    let set = |s: &mut Session, props: Value| ex(s, "node.set", json!({"ids": [r], "props": props}));
    let opacity = |s: &Session| s.doc().unwrap().node(r).unwrap().opacity;
    for (v, want) in [(json!(0), 0.0), (json!(1), 1.0), (json!(0.5), 0.5), (json!("80%"), 0.8), (json!("100%"), 1.0), (json!("0%"), 0.0)] {
        set(&mut s, json!({"opacity": v}));
        assert!((opacity(&s) - want).abs() < 1e-6, "opacity {v} → {}", opacity(&s));
    }
    assert!(s.execute("node.set", &json!({"ids": [r], "props": {"opacity": "150%"}})).is_err(), "150% is out of range");
    set(&mut s, json!({"cornerSmoothing": "60%", "cornerRadius": [0, 4, 8, 12], "strokeWeight": 0, "width": 0}));
    let n = s.doc().unwrap().node(r).unwrap();
    assert_eq!((n.corners.smoothing, n.corners.radii, n.stroke.weight, n.width), (0.6, [0.0, 4.0, 8.0, 12.0], 0.0, 0.0));
}

#[test]
fn node_create_refuses_out_of_range_props_and_creates_nothing() {
    let mut s = Session::new();
    let page = s.page().unwrap();
    let count = |s: &Session| s.doc().unwrap().node(page).unwrap().children.len();
    let before = count(&s);
    let cases = [
        (json!({"type": "rectangle", "props": {"opacity": 1.5}}), "opacity"),
        (json!({"type": "rectangle", "props": {"cornerRadius": -10}}), "cornerRadius"),
        (json!({"type": "rectangle", "props": {"strokeWeight": -1}}), "strokeWeight"),
        (json!({"type": "rectangle", "props": {"width": -50}}), "width"),
        (json!({"type": "frame", "width": -50}), "width"),
        (json!({"type": "ellipse", "height": -1}), "height"),
    ];
    for (params, key) in cases {
        let msg = bad_msg(s.execute("node.create", &params));
        assert!(msg.contains(&format!("`{key}`")), "{params}: {msg}");
        assert_eq!(count(&s), before, "{params} left a layer behind");
    }
}
