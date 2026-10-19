//! The web host API: what a page embedding the editor in an `<iframe>` can ask of it. Methods use
//! the control channel's names (`crates/ui-egui/src/control.rs`) plus host-only `host.*` ones;
//! `params` and results travel as JSON text, so 64-bit node ids never pass through JavaScript
//! numbers. The `postMessage` transport is in `bridge.rs`; this module is plain Rust.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use base64::Engine as _;
use figcraft_doc::{Document, ExportFormat, NodeId};
use figcraft_engine::{DocState, Session};
use figcraft_ui_egui::guests::FlashKind;
use figcraft_ui_egui::theme::{HostPalette, Tokens};
use figcraft_ui_egui::{ControlReply, ControlRequest, FigApp, i18n};
use serde_json::{Value, json};

/// The typed error codes a host sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Code {
    CanvasHidden,
    UnknownMethod,
    EngineError,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::CanvasHidden => "CANVAS_HIDDEN",
            Code::UnknownMethod => "UNKNOWN_METHOD",
            Code::EngineError => "ENGINE_ERROR",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct HostError {
    pub code: Code,
    pub message: String,
}

fn engine_error(message: impl Into<String>) -> HostError {
    HostError { code: Code::EngineError, message: message.into() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HostMethod {
    OpenDocument,
    Snapshot,
    MarkSaved,
    RenderPng,
    ExportBytes,
    Theme,
    Locale,
    Describe,
    GuestCursor,
    GuestFlash,
    GuestHide,
    File,
}

const HOST_METHODS: &[(&str, HostMethod)] = &[
    ("host.openDocument", HostMethod::OpenDocument),
    ("host.snapshot", HostMethod::Snapshot),
    ("host.markSaved", HostMethod::MarkSaved),
    ("host.renderPng", HostMethod::RenderPng),
    ("host.exportBytes", HostMethod::ExportBytes),
    ("host.theme", HostMethod::Theme),
    ("host.locale", HostMethod::Locale),
    ("host.describe", HostMethod::Describe),
    ("host.guestCursor", HostMethod::GuestCursor),
    ("host.guestFlash", HostMethod::GuestFlash),
    ("host.guestHide", HostMethod::GuestHide),
    ("host.file", HostMethod::File),
];

/// Control methods that act on the session alone: answered at once, outside any frame.
const ENGINE_METHODS: &[&str] = &["engine.execute", "engine.commands", "document.inspect", "node.get"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Route {
    Engine,
    Host(HostMethod),
    /// `ui.*`: needs an egui pass, so it waits for the next frame.
    Frame,
    Unknown,
}

fn route(method: &str) -> Route {
    if ENGINE_METHODS.contains(&method) {
        return Route::Engine;
    }
    if let Some((_, m)) = HOST_METHODS.iter().find(|(name, _)| *name == method) {
        return Route::Host(*m);
    }
    if method.starts_with("ui.") {
        return Route::Frame;
    }
    Route::Unknown
}

pub enum Answer {
    /// The result as JSON text.
    Now(Result<String, HostError>),
    /// A `ui.*` method queued for the next frame; the frame's control drain replies here.
    Later(Receiver<ControlReply>),
}

/// What `event.dirtyChanged` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirtyState {
    pub dirty: bool,
    pub content_version: u64,
}

impl DirtyState {
    pub fn to_json(self) -> String {
        json!({"dirty": self.dirty, "contentVersion": self.content_version}).to_string()
    }
}

/// The document content last seen. Holding the `Arc` keeps copy-on-write honest: any later edit
/// replaces the document rather than mutating this one in place, so pointer identity means
/// "same content".
struct Seen {
    uid: u64,
    doc: Arc<Document>,
}

/// Host-side state of one mounted editor.
#[derive(Default)]
pub struct Host {
    seen: Option<Seen>,
    content_version: u64,
    /// The state last reported; `None` until the editor is ready (no dirty before ready).
    reported: Option<DirtyState>,
}

impl Host {
    /// Bumps `contentVersion` when the active document's content is no longer what was last seen.
    /// A gesture in progress is not content yet: it counts once, when it is committed.
    fn content_version(&mut self, session: &Session) -> u64 {
        let Ok(ds) = session.ds() else { return self.content_version };
        if ds.interaction.is_some() {
            return self.content_version;
        }
        let doc = ds.document_itself();
        if !self.seen.as_ref().is_some_and(|s| s.uid == ds.uid && Arc::ptr_eq(&s.doc, doc)) {
            self.content_version += 1;
            self.seen = Some(Seen { uid: ds.uid, doc: doc.clone() });
        }
        self.content_version
    }

    pub fn state(&mut self, session: &Session) -> DirtyState {
        let content_version = self.content_version(session);
        DirtyState { dirty: session.ds().is_ok_and(DocState::is_dirty), content_version }
    }

    /// The editor drew its first frame: from now on, changes are reported.
    pub fn mark_ready(&mut self, session: &Session) {
        self.reported = Some(self.state(session));
    }

    /// The state to report when it differs from the last one reported (after ready only).
    pub fn changed(&mut self, session: &Session) -> Option<DirtyState> {
        self.reported?;
        let now = self.state(session);
        (self.reported != Some(now)).then(|| {
            self.reported = Some(now);
            now
        })
    }

    /// Answer `method` with `params` (JSON text). `hidden`: no frames are drawn now (the page is
    /// hidden or the canvas is off screen).
    pub fn call(&mut self, app: &mut FigApp, ctx: &egui::Context, method: &str, params: &str, hidden: bool) -> Answer {
        let unknown = || Answer::Now(Err(HostError { code: Code::UnknownMethod, message: format!("unknown method `{method}`") }));
        let route = route(method);
        if route == Route::Unknown {
            return unknown();
        }
        let p: Value = if params.trim().is_empty() {
            json!({})
        } else {
            match serde_json::from_str(params) {
                Ok(p) => p,
                Err(e) => return Answer::Now(Err(engine_error(format!("params are not JSON: {e}")))),
            }
        };
        if app.shell.rejects(method, &p) {
            return unknown();
        }
        let result = match route {
            Route::Frame if hidden => {
                return Answer::Now(Err(HostError {
                    code: Code::CanvasHidden,
                    message: format!("`{method}` needs a frame and the canvas is hidden"),
                }));
            }
            Route::Frame => {
                let (reply, rx) = std::sync::mpsc::channel();
                let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
                app.control_queue.push(ControlRequest { id: Value::Null, method: method.to_string(), params: p, reply, cancelled });
                ctx.request_repaint();
                return Answer::Later(rx);
            }
            Route::Engine => figcraft_ui_egui::control::handle(app, ctx, method, &p).map_err(engine_error),
            Route::Host(m) => self.host(app, ctx, m, &p),
            Route::Unknown => return unknown(),
        };
        ctx.request_repaint();
        Answer::Now(result.and_then(|v| serde_json::to_string(&v).map_err(|e| engine_error(e.to_string()))))
    }

    fn host(&mut self, app: &mut FigApp, ctx: &egui::Context, m: HostMethod, p: &Value) -> Result<Value, HostError> {
        let b64 = base64::engine::general_purpose::STANDARD;
        match m {
            HostMethod::OpenDocument => {
                let text = p.get("bytes").and_then(Value::as_str).ok_or_else(|| engine_error("`bytes` (base64) required"))?;
                let bytes = b64.decode(text).map_err(|e| engine_error(format!("`bytes` is not base64: {e}")))?;
                let loaded = figcraft_format::load_bytes_with_versions(&bytes).map_err(|e| engine_error(e.to_string()))?;
                // One document per mount: the opened file replaces whatever was open.
                let ds = DocState::new(loaded.doc, None).with_versions(loaded.versions).with_branches(loaded.branches);
                app.session.docs = vec![ds];
                app.session.active = 0;
                app.ui.home = false;
                Ok(json!({"contentVersion": self.content_version(&app.session)}))
            }
            HostMethod::Snapshot => {
                let content_version = self.content_version(&app.session);
                let bytes = on_the_document_itself(&mut app.session, |s| {
                    let ds = s.ds().map_err(|e| engine_error(e.to_string()))?;
                    figcraft_engine::cmd::file::file_bytes(ds).map_err(|e| engine_error(e.to_string()))
                })?;
                Ok(json!({"contentVersion": content_version, "bytes": b64.encode(bytes)}))
            }
            HostMethod::MarkSaved => {
                let saved = p.get("contentVersion").and_then(Value::as_u64).ok_or_else(|| engine_error("`contentVersion` required"))?;
                // A snapshot older than the content now must not clear dirty.
                if saved == self.content_version(&app.session) {
                    on_the_document_itself(&mut app.session, |s| s.ds_mut().map(DocState::mark_saved).map_err(|e| engine_error(e.to_string())))?;
                }
                let s = self.state(&app.session);
                Ok(json!({"dirty": s.dirty, "contentVersion": s.content_version}))
            }
            HostMethod::RenderPng => export(&app.session, ExportFormat::Png, p).map(|bytes| json!(b64.encode(bytes))),
            HostMethod::ExportBytes => {
                let name = p.get("format").and_then(Value::as_str).ok_or_else(|| engine_error("`format` required: png|jpg|svg|pdf|webp"))?;
                let format = ExportFormat::from_ext(&name.to_ascii_lowercase()).ok_or_else(|| engine_error(format!("unknown format `{name}`")))?;
                export(&app.session, format, p).map(|bytes| json!(b64.encode(bytes)))
            }
            HostMethod::Theme => {
                let tokens = palette(p.get("tokens").ok_or_else(|| engine_error("`tokens` required (null clears them)"))?)?;
                app.ui.host_theme = tokens.as_ref().map(Tokens::from_host);
                app.apply_theme(ctx);
                Ok(json!({"dark": app.ui.dark}))
            }
            HostMethod::Locale => {
                let tag = p.get("bcp47").and_then(Value::as_str).ok_or_else(|| engine_error("`bcp47` required"))?;
                app.session.execute("pref.language", &json!({"language": tag})).map_err(|e| engine_error(e.to_string()))?;
                // At once, not at the next frame: a hidden mount draws none.
                i18n::set(&app.session.prefs.language, app.services.system_language().as_deref());
                Ok(json!({"language": i18n::current().map_or("en", |l| l.code)}))
            }
            HostMethod::Describe => Ok(json!({"tag": option_env!("FIGCRAFT_HOST_TAG").unwrap_or("dev"), "engineVersion": env!("CARGO_PKG_VERSION")})),
            HostMethod::GuestCursor => {
                let (id, name) = (text(p, "id")?, text(p, "name")?);
                let color = text(p, "color")?;
                let color = hex(color).ok_or_else(|| engine_error(format!("`color` is not #rrggbb: `{color}`")))?;
                let at = (number(p, "x")?, number(p, "y")?);
                let page = match p.get("page") {
                    None | Some(Value::Null) => None,
                    Some(v) => Some(figcraft_engine::params::id_of(v).ok_or_else(|| engine_error("`page` is not a node id"))?),
                };
                let guests = &mut app.ui.guests;
                guests.cursor(guests.now(), id, name, color, at, page);
                Ok(json!({}))
            }
            HostMethod::GuestFlash => {
                let id = text(p, "id")?;
                let ids = p.get("ids").and_then(Value::as_array).ok_or_else(|| engine_error("`ids` (node ids) required"))?;
                let nodes = ids
                    .iter()
                    .map(figcraft_engine::params::id_of)
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(|| engine_error("`ids` holds something that is not a node id"))?;
                let kind = p
                    .get("kind")
                    .and_then(Value::as_str)
                    .and_then(FlashKind::parse)
                    .ok_or_else(|| engine_error("`kind` must be \"write\" or \"read\""))?;
                let guests = &mut app.ui.guests;
                guests.flash(guests.now(), id, &nodes, kind);
                Ok(json!({}))
            }
            HostMethod::GuestHide => {
                let id = text(p, "id")?;
                let guests = &mut app.ui.guests;
                guests.hide(guests.now(), id);
                Ok(json!({}))
            }
            HostMethod::File => {
                let name = text(p, "name")?.to_string();
                if name.is_empty() {
                    return Err(engine_error("`name` must not be empty"));
                }
                let (path, location) = (optional_text(p, "path")?, optional_text(p, "location")?);
                let content_version = self.content_version(&app.session);
                on_the_document_itself(&mut app.session, |s| {
                    let ds = s.ds_mut().map_err(|e| engine_error(e.to_string()))?;
                    ds.path = path;
                    if ds.doc.name != name {
                        let clean = !ds.is_dirty();
                        ds.doc_mut().name = name;
                        if clean {
                            ds.mark_saved();
                        }
                    }
                    Ok(())
                })?;
                // The host's file name is not an edit: the renamed document keeps the version.
                self.seen = app.session.ds().ok().map(|ds| Seen { uid: ds.uid, doc: ds.document_itself().clone() });
                app.ui.host_location = location;
                Ok(json!({"contentVersion": content_version}))
            }
        }
    }
}

fn optional_text(p: &Value, key: &str) -> Result<Option<String>, HostError> {
    match p.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(engine_error(format!("`{key}` must be text or null"))),
    }
}

fn text<'a>(p: &'a Value, key: &str) -> Result<&'a str, HostError> {
    p.get(key).and_then(Value::as_str).ok_or_else(|| engine_error(format!("`{key}` (text) required")))
}

fn number(p: &Value, key: &str) -> Result<f64, HostError> {
    p.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).ok_or_else(|| engine_error(format!("`{key}` (number) required")))
}

/// Runs `f` with the document itself in `ds.doc`, not the version or live text preview shown in
/// its place (saving encodes and marks saved `ds.doc`). A version preview is shown again after.
fn on_the_document_itself<R>(session: &mut Session, f: impl FnOnce(&mut Session) -> R) -> R {
    figcraft_engine::textpreview::end(session);
    let preview = session.ds_mut().ok().and_then(|ds| {
        let shown = ds.preview.clone().map(|p| (p, ds.doc.clone()));
        ds.end_preview();
        shown
    });
    let result = f(session);
    if let (Some((p, shown)), Ok(ds)) = (preview, session.ds_mut()) {
        ds.start_preview(&p.id, &p.name, p.time, shown);
    }
    result
}

/// The host's colour roles in `host.theme`'s `tokens`, each `#rrggbb`, with `mode` `light` or
/// `dark`. `null` hands the colours back to the editor's theme preference.
fn palette(tokens: &Value) -> Result<Option<HostPalette>, HostError> {
    if tokens.is_null() {
        return Ok(None);
    }
    let dark = match tokens.get("mode").and_then(Value::as_str) {
        Some("dark") => true,
        Some("light") => false,
        _ => return Err(engine_error("`tokens.mode` must be \"light\" or \"dark\"")),
    };
    let color = |key: &str| {
        let text = tokens.get(key).and_then(Value::as_str).ok_or_else(|| engine_error(format!("`tokens.{key}` (#rrggbb) required")))?;
        hex(text).ok_or_else(|| engine_error(format!("`tokens.{key}` is not #rrggbb: `{text}`")))
    };
    Ok(Some(HostPalette {
        dark,
        background: color("background")?,
        card: color("card")?,
        foreground: color("foreground")?,
        muted_foreground: color("mutedForeground")?,
        border: color("border")?,
        primary: color("primary")?,
        primary_fill: color("primaryFill")?,
        destructive: color("destructive")?,
    }))
}

fn hex(text: &str) -> Option<egui::Color32> {
    let digits = text.strip_prefix('#').filter(|d| d.len() == 6 && d.bytes().all(|b| b.is_ascii_hexdigit()))?;
    let byte = |i: usize| digits.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
    Some(egui::Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// `ids` (default: the current page's top-level layers) exported as `format` at `scale`.
fn export(session: &Session, format: ExportFormat, p: &Value) -> Result<Vec<u8>, HostError> {
    let scale = p.get("scale").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).map_or(1.0, |s| s.min(64.0));
    let doc = session.doc().map_err(|e| engine_error(e.to_string()))?;
    let mut ids: Vec<NodeId> =
        p.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(figcraft_engine::params::id_of).collect()).unwrap_or_default();
    if ids.is_empty() {
        let page = session.page().map_err(|e| engine_error(e.to_string()))?;
        ids = doc.node(page).map(|n| n.children.clone()).unwrap_or_default();
    }
    if ids.is_empty() {
        return Err(engine_error("the current page is empty: nothing to export"));
    }
    if let Some(missing) = ids.iter().find(|i| !doc.contains(**i)) {
        return Err(engine_error(format!("no such node {missing}")));
    }
    figcraft_engine::cmd::file::export_bytes(doc, &ids, format, scale).map_err(|e| engine_error(e.to_string()))
}
