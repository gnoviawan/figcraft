//! SVG export of angular and diamond gradients, and the gradient and effect values the commands
//! refuse (gnoviawan/termul#1428).

use crate::common::Client;
use serde_json::{Value, json};

fn gradient_rect(c: &mut Client, kind: &str) -> Value {
    c.create(json!({"type": "rectangle", "width": 100, "height": 100,
        "props": {"fills": [{"type": format!("{kind}Gradient"), "stops": ["#FF0000", "#0000FF"]}]}}))
}

/// The exported SVG text and the warnings that came with it.
fn export_svg(c: &mut Client, id: &Value) -> (String, Vec<String>) {
    let r = c.call("export", json!({"ids": [id], "format": "svg"}));
    assert_eq!(r["isError"], false, "{r}");
    let blocks = r["content"].as_array().unwrap();
    let svg = blocks[0]["text"].as_str().unwrap().to_string();
    let warnings = blocks
        .iter()
        .skip(1)
        .filter_map(|b| serde_json::from_str::<Value>(b["text"].as_str()?).ok())
        .flat_map(|v| v["warnings"].as_array().cloned().unwrap_or_default())
        .filter_map(|w| w.as_str().map(str::to_string))
        .collect();
    (svg, warnings)
}

#[test]
fn angular_gradient_exports_as_an_image_with_a_warning() {
    let mut c = Client::new();
    let r = gradient_rect(&mut c, "angular");
    let (svg, warnings) = export_svg(&mut c, &r);
    assert!(!svg.contains("radialGradient"), "{svg}");
    assert!(svg.contains("<image") && svg.contains("data:image/png;base64,"), "{svg}");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("angular"), "{warnings:?}");

    let engine = c.run("file.export", json!({"ids": [r], "format": "svg"}));
    assert_eq!(engine["warnings"].as_array().map(Vec::len), Some(1), "{}", engine["warnings"]);
}

#[test]
fn diamond_gradient_exports_as_vectors_without_a_warning() {
    let mut c = Client::new();
    let r = gradient_rect(&mut c, "diamond");
    let (svg, warnings) = export_svg(&mut c, &r);
    assert!(!svg.contains("radialGradient"), "{svg}");
    assert!(!svg.contains("<image"), "{svg}");
    assert!(svg.contains("linearGradient"), "{svg}");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(c.run("file.export", json!({"ids": [r], "format": "svg"})).get("warnings").is_none());
}

#[test]
fn gradients_need_two_stops_inside_zero_to_one() {
    let mut c = Client::new();
    let r = c.create(json!({"type": "rectangle", "width": 100, "height": 100}));
    let set = |c: &mut Client, fills: Value| c.err("set_properties", json!({"ids": [r], "props": {"fills": fills}}));
    for stops in [json!([]), json!(["#FF0000"])] {
        let e = set(&mut c, json!([{"type": "linearGradient", "stops": stops}]));
        assert!(e.contains("`fills`") && e.contains("two stops"), "{e}");
    }
    for pos in [1.5, -1.0] {
        let e =
            set(&mut c, json!([{"type": "radialGradient", "stops": [{"position": 0, "color": "#FF0000"}, {"position": pos, "color": "#0000FF"}]}]));
        assert!(e.contains("`fills`") && e.contains("position") && e.contains(&pos.to_string()), "{e}");
    }
    let e = c.err("set_properties", json!({"ids": [r], "props": {"strokes": ["#000000", {"type": "linearGradient", "stops": []}]}}));
    assert!(e.contains("`strokes`[1]"), "{e}");
    c.ok("set_properties", json!({"ids": [r], "props": {"fills": [{"type": "linearGradient", "stops": ["#FF0000", "#0000FF"]}]}}));
}

#[test]
fn shadow_and_blur_radius_cannot_be_negative() {
    let mut c = Client::new();
    let r = c.create(json!({"type": "rectangle", "width": 100, "height": 100}));
    for effect in [
        json!({"type": "dropShadow", "radius": -5}),
        json!({"type": "innerShadow", "radius": -5}),
        json!({"type": "layerBlur", "radius": -5}),
        json!({"type": "backgroundBlur", "radius": -5}),
    ] {
        let e = c.err("set_properties", json!({"ids": [r], "props": {"effects": [effect]}}));
        assert!(e.contains("`effects`[0]") && e.contains("radius"), "{e}");
    }
    let e = c.err("run_command", json!({"command": "effect.add", "params": {"ids": [r], "effect": {"type": "dropShadow", "radius": -1}}}));
    assert!(e.contains("radius"), "{e}");
    c.ok("set_properties", json!({"ids": [r], "props": {"effects": [{"type": "dropShadow", "radius": 0}]}}));
}

#[test]
fn paint_opacity_is_zero_to_one_or_a_spelled_out_percentage() {
    let mut c = Client::new();
    let r = c.create(json!({"type": "rectangle", "width": 100, "height": 100, "props": {"fill": "#FF0000"}}));
    for command in ["fill.set", "stroke.set"] {
        let e = c.err("run_command", json!({"command": command, "params": {"ids": [r], "opacity": 80}}));
        assert!(e.contains("`opacity`"), "{e}");
    }
    c.run("fill.set", json!({"ids": [r], "opacity": "80%"}));
    assert!((c.node(&r)["fills"][0]["opacity"].as_f64().unwrap() - 0.8).abs() < 1e-6);
    c.run("fill.set", json!({"ids": [r], "opacity": 0.25}));
    assert!((c.node(&r)["fills"][0]["opacity"].as_f64().unwrap() - 0.25).abs() < 1e-6);
}
