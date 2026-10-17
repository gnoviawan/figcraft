use base64::Engine as _;
use figcraft_engine::Session;
use figcraft_ui_egui::{FigApp, NoServices};
use serde_json::{Value, json};

use crate::host::{Answer, Code, DirtyState, Host, HostError};

fn app() -> (FigApp, egui::Context, Host) {
    (FigApp::new(Session::new(), Box::new(NoServices)), egui::Context::default(), Host::default())
}

fn now(answer: Answer) -> Result<String, HostError> {
    match answer {
        Answer::Now(r) => r,
        Answer::Later(_) => panic!("expected an answer now"),
    }
}

fn ok(host: &mut Host, app: &mut FigApp, ctx: &egui::Context, method: &str, params: Value) -> Value {
    let text = now(host.call(app, ctx, method, &params.to_string(), false)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn rect(host: &mut Host, app: &mut FigApp, ctx: &egui::Context) -> Value {
    ok(host, app, ctx, "engine.execute", json!({"command": "shape.rectangle", "params": {"x": 0, "y": 0, "width": 40, "height": 20}}))
}

fn b64(v: &Value) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(v.as_str().unwrap()).unwrap()
}

#[test]
fn unknown_and_desktop_only_methods_are_typed_errors() {
    let (mut app, ctx, mut host) = app();
    for method in ["nope", "app.quit", "window.new", "event.ready"] {
        let err = now(host.call(&mut app, &ctx, method, "{}", false)).unwrap_err();
        assert_eq!(err.code, Code::UnknownMethod, "{method}");
    }
}

#[test]
fn engine_errors_and_bad_params_are_engine_errors() {
    let (mut app, ctx, mut host) = app();
    let err = now(host.call(&mut app, &ctx, "engine.execute", r#"{"command": "no.such"}"#, false)).unwrap_err();
    assert_eq!(err.code, Code::EngineError);
    let err = now(host.call(&mut app, &ctx, "node.get", "{not json", false)).unwrap_err();
    assert_eq!(err.code, Code::EngineError);
}

#[test]
fn execute_runs_at_once_and_node_ids_beyond_2_pow_53_survive_as_text() {
    let (mut app, ctx, mut host) = app();
    app.session.doc_mut().unwrap().set_client_id(1 << 22);
    let text =
        now(host.call(&mut app, &ctx, "engine.execute", r#"{"command":"shape.rectangle","params":{"x":0,"y":0,"width":40,"height":20}}"#, false))
            .unwrap();
    let id = app.session.selection()[0].0;
    assert!(id > 1 << 53, "the id needs more than a JavaScript number's 53 bits");
    assert!(text.contains(&id.to_string()), "{text}");
    let node = now(host.call(&mut app, &ctx, "node.get", &format!(r#"{{"id":{id}}}"#), false)).unwrap();
    assert!(node.contains(&format!(r#""id":{id}"#)), "{node}");
}

#[test]
fn ui_methods_wait_for_a_frame_and_fail_at_once_while_hidden() {
    let (mut app, ctx, mut host) = app();
    let err = now(host.call(&mut app, &ctx, "ui.inspect", "{}", true)).unwrap_err();
    assert_eq!(err.code, Code::CanvasHidden);
    assert!(app.control_queue.is_empty());
    assert!(matches!(host.call(&mut app, &ctx, "ui.inspect", "{}", false), Answer::Later(_)));
    assert_eq!(app.control_queue.len(), 1);
    assert_eq!(app.control_queue[0].method, "ui.inspect");
}

#[test]
fn content_version_counts_content_changes_only() {
    let (mut app, ctx, mut host) = app();
    let v0 = host.state(&app.session);
    rect(&mut host, &mut app, &ctx);
    let v1 = host.state(&app.session);
    assert_eq!(v1, DirtyState { dirty: true, content_version: v0.content_version + 1 });
    ok(&mut host, &mut app, &ctx, "engine.execute", json!({"command": "select.none"}));
    ok(&mut host, &mut app, &ctx, "engine.execute", json!({"command": "view.zoomIn"}));
    assert_eq!(host.state(&app.session), v1, "selection and viewport are not content");
}

#[test]
fn mark_saved_clears_dirty_only_for_the_current_content() {
    let (mut app, ctx, mut host) = app();
    rect(&mut host, &mut app, &ctx);
    let snap = ok(&mut host, &mut app, &ctx, "host.snapshot", json!({}));
    let at = snap["contentVersion"].as_u64().unwrap();
    rect(&mut host, &mut app, &ctx);
    let stale = ok(&mut host, &mut app, &ctx, "host.markSaved", json!({"contentVersion": at}));
    assert_eq!(stale, json!({"dirty": true, "contentVersion": at + 1}));
    let fresh = ok(&mut host, &mut app, &ctx, "host.markSaved", json!({"contentVersion": at + 1}));
    assert_eq!(fresh, json!({"dirty": false, "contentVersion": at + 1}));
}

#[test]
fn snapshot_reopens_as_the_same_clean_document() {
    let (mut app, ctx, mut host) = app();
    rect(&mut host, &mut app, &ctx);
    let snap = ok(&mut host, &mut app, &ctx, "host.snapshot", json!({}));
    let (mut other, ctx2, mut host2) = self::app();
    let ack = ok(&mut host2, &mut other, &ctx2, "host.openDocument", json!({"bytes": snap["bytes"]}));
    assert_eq!(ack["contentVersion"], json!(host2.state(&other.session).content_version));
    assert!(!host2.state(&other.session).dirty);
    let doc = ok(&mut host2, &mut other, &ctx2, "document.inspect", json!({}));
    assert!(doc.to_string().contains("Rectangle"), "{doc}");
    let err = now(host2.call(&mut other, &ctx2, "host.openDocument", r#"{"bytes":"bm90IGEgZmlsZQ=="}"#, false)).unwrap_err();
    assert_eq!(err.code, Code::EngineError);
}

#[test]
fn render_and_export_return_base64_bytes() {
    let (mut app, ctx, mut host) = app();
    let err = now(host.call(&mut app, &ctx, "host.renderPng", "{}", false)).unwrap_err();
    assert_eq!(err.code, Code::EngineError, "an empty page has nothing to render");
    rect(&mut host, &mut app, &ctx);
    let png = b64(&ok(&mut host, &mut app, &ctx, "host.renderPng", json!({"scale": 2})));
    assert_eq!(png.get(..8), Some(&b"\x89PNG\r\n\x1a\n"[..]));
    let svg = b64(&ok(&mut host, &mut app, &ctx, "host.exportBytes", json!({"format": "SVG"})));
    assert!(String::from_utf8(svg).unwrap().contains("<svg"));
    let err = now(host.call(&mut app, &ctx, "host.exportBytes", r#"{"format":"tga"}"#, false)).unwrap_err();
    assert_eq!(err.code, Code::EngineError);
}

#[test]
fn changes_are_reported_once_and_only_after_ready() {
    let (mut app, ctx, mut host) = app();
    rect(&mut host, &mut app, &ctx);
    assert_eq!(host.changed(&app.session), None, "nothing before ready");
    host.mark_ready(&app.session);
    assert_eq!(host.changed(&app.session), None);
    rect(&mut host, &mut app, &ctx);
    let change = host.changed(&app.session).unwrap();
    assert!(change.dirty);
    assert_eq!(host.changed(&app.session), None, "reported once");
}

#[test]
fn theme_locale_and_describe_answer() {
    let (mut app, ctx, mut host) = app();
    ok(&mut host, &mut app, &ctx, "host.theme", json!({"tokens": {"bg": "#000"}}));
    ok(&mut host, &mut app, &ctx, "host.locale", json!({"bcp47": "id-ID"}));
    assert_eq!(host.theme, Some(json!({"bg": "#000"})));
    assert_eq!(host.locale.as_deref(), Some("id-ID"));
    let d = ok(&mut host, &mut app, &ctx, "host.describe", json!({}));
    assert_eq!(d["tag"], json!(option_env!("FIGCRAFT_HOST_TAG").unwrap_or("dev")));
    assert_eq!(d["engineVersion"], json!(env!("CARGO_PKG_VERSION")));
}

fn kiosk_app() -> (FigApp, egui::Context, Host) {
    let (mut app, ctx, host) = app();
    app.shell = figcraft_ui_egui::Shell::Kiosk;
    (app, ctx, host)
}

#[test]
fn kiosk_answers_unknown_method_to_the_calls_that_reopen_the_shell() {
    let (mut app, ctx, mut host) = kiosk_app();
    let refused = [
        ("app.quit", "{}"),
        ("window.new", "{}"),
        ("window.focus", r#"{"id":1}"#),
        ("ui.set", r#"{"home":true}"#),
        ("ui.set", r#"{"mainMenu":"File"}"#),
        ("ui.menu.invoke", r#"{"id":"file.open"}"#),
        ("engine.execute", r#"{"command":"window.new"}"#),
    ];
    for (method, params) in refused {
        for hidden in [false, true] {
            let err = now(host.call(&mut app, &ctx, method, params, hidden)).unwrap_err();
            assert_eq!(err.code, Code::UnknownMethod, "{method} {params} hidden={hidden}");
        }
    }
    assert!(app.control_queue.is_empty(), "nothing reached the frame queue");
    assert!(!app.ui.home);
}

#[test]
fn kiosk_still_drives_the_editor() {
    let (mut app, ctx, mut host) = kiosk_app();
    rect(&mut host, &mut app, &ctx);
    assert!(matches!(host.call(&mut app, &ctx, "ui.set", r#"{"rightTab":"inspect"}"#, false), Answer::Later(_)));
    assert!(matches!(host.call(&mut app, &ctx, "ui.menu.invoke", r#"{"id":"edit.undo"}"#, false), Answer::Later(_)));
    ok(&mut host, &mut app, &ctx, "host.snapshot", json!({}));
}

#[test]
fn outside_kiosk_the_same_calls_go_through() {
    let (mut app, ctx, mut host) = app();
    assert!(matches!(host.call(&mut app, &ctx, "ui.set", r#"{"home":true}"#, false), Answer::Later(_)));
    assert!(matches!(host.call(&mut app, &ctx, "ui.menu.invoke", r#"{"id":"file.open"}"#, false), Answer::Later(_)));
    let ran = now(host.call(&mut app, &ctx, "engine.execute", r#"{"command":"window.new"}"#, false));
    assert!(ran.is_ok(), "{ran:?}");
}

fn rect_count(bytes: &[u8]) -> usize {
    let loaded = figcraft_format::load_bytes_with_versions(bytes).unwrap();
    loaded.doc.nodes().filter(|n| n.name.starts_with("Rectangle")).count()
}

/// One rectangle saved as a version, a second one drawn, then the version shown in place of the document.
fn previewing_an_older_version() -> (FigApp, egui::Context, Host) {
    let (mut app, ctx, mut host) = app();
    rect(&mut host, &mut app, &ctx);
    ok(&mut host, &mut app, &ctx, "engine.execute", json!({"command": "file.saveVersion", "params": {"name": "one rectangle"}}));
    rect(&mut host, &mut app, &ctx);
    ok(&mut host, &mut app, &ctx, "engine.execute", json!({"command": "version.preview", "params": {"index": 0}}));
    assert!(app.session.ds().unwrap().preview.is_some());
    (app, ctx, host)
}

#[test]
fn a_snapshot_taken_during_a_version_preview_is_the_real_document() {
    let (mut app, ctx, mut host) = previewing_an_older_version();
    let before = host.state(&app.session);
    let snap = ok(&mut host, &mut app, &ctx, "host.snapshot", json!({}));
    assert_eq!(rect_count(&b64(&snap["bytes"])), 2, "the two rectangles, not the version's one");
    assert!(app.session.ds().unwrap().preview.is_some(), "the user is still looking at the version");
    assert_eq!(app.session.doc().unwrap().nodes().filter(|n| n.name.starts_with("Rectangle")).count(), 1, "and it is what the canvas shows");
    assert_eq!(host.state(&app.session), before, "a preview is not a content change");
}

#[test]
fn marking_saved_during_a_version_preview_marks_the_real_document() {
    let (mut app, ctx, mut host) = previewing_an_older_version();
    let at = ok(&mut host, &mut app, &ctx, "host.snapshot", json!({}))["contentVersion"].as_u64().unwrap();
    let saved = ok(&mut host, &mut app, &ctx, "host.markSaved", json!({"contentVersion": at}));
    assert_eq!(saved["dirty"], json!(false));
    assert!(app.session.ds_mut().unwrap().end_preview());
    assert!(!host.state(&app.session).dirty, "still clean once the preview is over");
}
