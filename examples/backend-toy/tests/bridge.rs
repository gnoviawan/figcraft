use backend_toy::Bridge;
use figcraft_mcp::{Backend, Headless, call_tool};
use serde_json::{Value, json};

/// A transport answering like the editor's bridge, from an in-process session.
fn editor() -> Bridge {
    let mut headless = Headless::new();
    Bridge::new(Box::new(move |text| {
        let req: Value = serde_json::from_str(text).unwrap();
        let params: Value = serde_json::from_str(req["params"].as_str().unwrap()).unwrap();
        let method = req["method"].as_str().unwrap();
        let reply = match method {
            "host.renderPng" => {
                use base64::Engine as _;
                let png = headless.render_png(&[], params["scale"].as_f64().unwrap_or(1.0));
                png.map(|b| json!(base64::engine::general_purpose::STANDARD.encode(b)))
            }
            m => headless.call(m, params),
        };
        match reply {
            Ok(v) => json!({"id": req["id"], "ok": true, "result": v.to_string(), "mountSeq": 1}),
            Err(m) => json!({"id": req["id"], "ok": false, "error": {"code": "ENGINE_ERROR", "message": m}, "mountSeq": 1}),
        }
        .to_string()
    }))
}

#[test]
fn upstream_tools_run_through_an_out_of_tree_backend() {
    let mut b = editor();
    let made = call_tool(&mut b, "create_node", &json!({"type": "rectangle", "x": 0, "y": 0, "width": 50, "height": 30}));
    assert!(!made.is_error, "{:?}", made.content);
    let doc = call_tool(&mut b, "inspect_document", &json!({}));
    assert!(!doc.is_error && json!(doc.content).to_string().contains("Rectangle"), "{:?}", doc.content);
    let shot = call_tool(&mut b, "export", &json!({"format": "png"}));
    assert!(!shot.is_error, "{:?}", shot.content);
}

#[test]
fn typed_errors_reach_the_tool_as_json_text() {
    let mut b = editor();
    let err = b.call("engine.execute", json!({"command": "no.such"})).unwrap_err();
    let parsed: Value = serde_json::from_str(&err).unwrap();
    assert_eq!(parsed["code"], "ENGINE_ERROR");
}
