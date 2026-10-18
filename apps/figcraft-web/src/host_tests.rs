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
    let sent: Value = serde_json::from_str(&change.to_json()).unwrap();
    assert_eq!(sent, json!({"dirty": true, "contentVersion": change.content_version}), "the event payload names its fields as the page reads them");
    assert_eq!(host.changed(&app.session), None, "reported once");
}

#[test]
fn describe_answers_the_tag_and_engine_version() {
    let (mut app, ctx, mut host) = app();
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

fn termul_dark() -> Value {
    json!({"mode": "dark", "background": "#08090a", "card": "#0f1011", "foreground": "#e5e5e6", "mutedForeground": "#8a8f98",
        "border": "#23252a", "primary": "#5b9bff", "primaryFill": "#3b82f6", "destructive": "#f0605d"})
}

fn termul_light() -> Value {
    json!({"mode": "light", "background": "#ffffff", "card": "#ffffff", "foreground": "#0d0d0d", "mutedForeground": "#6b6b6b",
        "border": "#e6e6e6", "primary": "#2563eb", "primaryFill": "#3B82F6", "destructive": "#c0262d"})
}

/// The UI language is one per process: tests that draw frames or switch it take turns.
static LANGUAGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn frame(ctx: &egui::Context, app: &mut FigApp) {
    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.ui(ui));
    output.textures_delta.clear();
}

#[test]
fn theme_tokens_restyle_the_editor_at_once_and_null_hands_back_the_preference() {
    let _turn = LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    use egui::Color32;
    use figcraft_ui_egui::theme::Tokens;
    let (mut app, ctx, mut host) = app();
    frame(&ctx, &mut app);
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.theme", json!({"tokens": termul_dark()})), json!({"dark": true}));
    let t = Tokens::get(&ctx);
    assert_eq!(
        (t.canvas, t.panel, t.text, t.accent),
        (Color32::from_rgb(8, 9, 10), Color32::from_rgb(15, 16, 17), Color32::from_rgb(0xe5, 0xe5, 0xe6), Color32::from_rgb(0x3b, 0x82, 0xf6))
    );
    assert!(ctx.global_style().visuals.dark_mode);
    frame(&ctx, &mut app);
    assert_eq!(Tokens::get(&ctx), t, "the next frame keeps the host's colours");

    assert_eq!(ok(&mut host, &mut app, &ctx, "host.theme", json!({"tokens": termul_light()})), json!({"dark": false}));
    assert_eq!(Tokens::get(&ctx).panel, Color32::WHITE);
    assert!(!ctx.global_style().visuals.dark_mode);
    assert!(!app.theme_toggle_shown());

    ok(&mut host, &mut app, &ctx, "host.theme", json!({"tokens": null}));
    assert_eq!(app.ui.host_theme, None);
    assert!(app.theme_toggle_shown());
}

#[test]
fn malformed_theme_tokens_are_engine_errors_and_keep_the_colours() {
    let (mut app, ctx, mut host) = app();
    ok(&mut host, &mut app, &ctx, "host.theme", json!({"tokens": termul_dark()}));
    let before = app.ui.host_theme;
    let with = |key: &str, v: Value| {
        let mut t = termul_dark();
        t[key] = v;
        json!({"tokens": t})
    };
    let mut no_border = termul_dark();
    no_border.as_object_mut().unwrap().remove("border");
    let bad = [
        json!({}),
        json!({"tokens": no_border}),
        with("mode", json!("dim")),
        with("card", json!("#fff")),
        with("card", json!("#gg0000")),
        with("card", json!("#+fffff")),
        with("card", json!("red")),
        with("card", json!(0x0f1011)),
        with("card", json!("#0f1011ff")),
    ];
    for params in bad {
        let err = now(host.call(&mut app, &ctx, "host.theme", &params.to_string(), false)).unwrap_err();
        assert_eq!(err.code, Code::EngineError, "{params}");
        assert_eq!(app.ui.host_theme, before, "{params}");
    }
}

#[test]
fn locale_switches_the_catalog_and_frames_keep_it() {
    use figcraft_ui_egui::i18n::tr;
    let _turn = LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, ctx, mut host) = app();
    frame(&ctx, &mut app);
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.locale", json!({"bcp47": "de-DE"})), json!({"language": "de"}));
    assert!(tr("About FigCraft").starts_with("Über "), "{}", tr("About FigCraft"));
    frame(&ctx, &mut app);
    assert!(tr("About FigCraft").starts_with("Über "), "a frame keeps the host's language");
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.locale", json!({"bcp47": "pt-BR"})), json!({"language": "pt-BR"}));
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.locale", json!({"bcp47": "en"})), json!({"language": "en"}));
    assert!(tr("About FigCraft").starts_with("About "));
    for bad in [json!({}), json!({"bcp47": "not a tag"}), json!({"bcp47": 7})] {
        let err = now(host.call(&mut app, &ctx, "host.locale", &bad.to_string(), false)).unwrap_err();
        assert_eq!(err.code, Code::EngineError, "{bad}");
    }
}

#[test]
fn error_codes_reach_the_page_under_their_documented_names() {
    let names: Vec<&str> = [Code::CanvasHidden, Code::UnknownMethod, Code::EngineError].into_iter().map(Code::as_str).collect();
    assert_eq!(names, ["CANVAS_HIDDEN", "UNKNOWN_METHOD", "ENGINE_ERROR"]);
}

#[test]
fn a_ui_method_is_answered_on_its_receiver_once_frames_run() {
    let _turn = LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, ctx, mut host) = app();
    let Answer::Later(reply) = host.call(&mut app, &ctx, "ui.inspect", "{}", false) else { panic!("expected the answer later") };
    assert!(reply.try_recv().is_err(), "nothing before a frame has run");
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    let answered = reply.try_recv().expect("the frame answered");
    assert!(answered.result.is_ok(), "{:?}", answered.result);
}

fn guests(app: &FigApp) -> Vec<Value> {
    app.inspect()["guests"].as_array().cloned().unwrap_or_default()
}

fn undo_len(app: &FigApp) -> usize {
    app.session.ds().unwrap().history.undo.len()
}

#[test]
fn guests_show_ring_and_hide_without_touching_the_document() {
    let _turn = LANGUAGE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (mut app, ctx, mut host) = app();
    host.mark_ready(&app.session);
    rect(&mut host, &mut app, &ctx);
    let node = app.session.selection()[0].0.to_string();
    let (before, undo, doc) = (host.state(&app.session), undo_len(&app), app.session.ds().unwrap().document_itself().clone());
    host.changed(&app.session);
    let page = app.session.page().unwrap().0.to_string();
    let cursor = json!({"id": "s1", "name": "Claude", "color": "#3B82F6", "x": 20.0, "y": 10.0, "page": page});
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestCursor", cursor), json!({}));
    assert_eq!(
        ok(&mut host, &mut app, &ctx, "host.guestCursor", json!({"id": "s2", "name": "opencode", "color": "#ec4899", "x": 300, "y": 200})),
        json!({})
    );
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestFlash", json!({"id": "s1", "ids": [node], "kind": "write"})), json!({}));
    frame(&ctx, &mut app);
    frame(&ctx, &mut app);
    let shown = guests(&app);
    assert_eq!(shown.len(), 2, "{shown:?}");
    assert_eq!((shown[0]["id"].clone(), shown[0]["name"].clone(), shown[0]["color"].clone()), (json!("s1"), json!("Claude"), json!("#3b82f6")));
    assert_eq!(shown[0]["flashes"], json!([{"id": node, "kind": "write"}]));
    assert_eq!(shown[0]["target"], json!([20.0, 10.0]));
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestFlash", json!({"id": "s2", "ids": [node], "kind": "read"})), json!({}));
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestHide", json!({"id": "s2"})), json!({}));
    frame(&ctx, &mut app);
    let doc_now = app.session.ds().unwrap().document_itself().clone();
    assert!(std::sync::Arc::ptr_eq(&doc, &doc_now), "the document is the same object");
    assert_eq!(host.state(&app.session), before, "contentVersion and dirty unchanged");
    assert_eq!(host.changed(&app.session), None, "no dirtyChanged event");
    assert_eq!(undo_len(&app), undo, "nothing to undo");
}

#[test]
fn guest_calls_for_unknown_guests_are_no_ops() {
    let (mut app, ctx, mut host) = app();
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestFlash", json!({"id": "ghost", "ids": ["12"], "kind": "read"})), json!({}));
    assert_eq!(ok(&mut host, &mut app, &ctx, "host.guestHide", json!({"id": "ghost"})), json!({}));
    assert!(guests(&app).is_empty());
}

#[test]
fn guest_calls_with_bad_params_are_engine_errors() {
    let (mut app, ctx, mut host) = app();
    let bad = [
        ("host.guestCursor", json!({"id": "s1", "name": "Claude", "x": 0, "y": 0})),
        ("host.guestCursor", json!({"id": "s1", "name": "Claude", "color": "blue", "x": 0, "y": 0})),
        ("host.guestCursor", json!({"id": "s1", "name": "Claude", "color": "#3b82f6", "x": "0", "y": 0})),
        ("host.guestCursor", json!({"id": "s1", "name": "Claude", "color": "#3b82f6", "x": 0, "y": 0, "page": "first"})),
        ("host.guestFlash", json!({"id": "s1", "ids": ["12"], "kind": "erase"})),
        ("host.guestFlash", json!({"id": "s1", "ids": "12", "kind": "write"})),
        ("host.guestFlash", json!({"id": "s1", "ids": ["x"], "kind": "write"})),
        ("host.guestHide", json!({})),
    ];
    for (method, params) in bad {
        let err = now(host.call(&mut app, &ctx, method, &params.to_string(), false)).unwrap_err();
        assert_eq!(err.code, Code::EngineError, "{method} {params}");
    }
    assert!(guests(&app).is_empty());
}

#[test]
fn guest_calls_work_in_a_kiosk_and_while_the_canvas_is_hidden() {
    for (mut app, ctx, mut host) in [app(), kiosk_app()] {
        let cursor = json!({"id": "s1", "name": "Claude", "color": "#3b82f6", "x": 0, "y": 0}).to_string();
        for (method, params) in [
            ("host.guestCursor", cursor.as_str()),
            ("host.guestFlash", r#"{"id":"s1","ids":["5"],"kind":"write"}"#),
            ("host.guestHide", r#"{"id":"s1"}"#),
        ] {
            assert_eq!(now(host.call(&mut app, &ctx, method, params, true)).unwrap(), "{}", "{method} kiosk={}", app.shell.is_kiosk());
        }
        assert_eq!(guests(&app).len(), 1, "the state updated without a frame");
    }
}
