//! The host API's `postMessage` transport. A page that frames the editor passes its origin as
//! `?ho=<origin>` (and its mount sequence as `?ms=<n>`); only messages from that origin and from
//! the parent window are answered, and every reply and event goes to that origin only.
//!
//! Request `{id, method, params, mountSeq}` → reply `{id, ok, result | error: {code, message},
//! mountSeq}`; `params` and `result` are JSON text. Events are `{method: "event.*", params,
//! mountSeq}`: `event.ready` after the first frame with content, `event.dirtyChanged {dirty,
//! contentVersion}` and `event.crashed {reason}`.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};

use figcraft_ui_egui::{ControlReply, FigApp};
use js_sys::{Object, Reflect};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast as _, JsValue};

use crate::host::{Answer, Code, Host, HostError};

/// The page embedding the editor.
#[derive(Clone)]
pub struct Link {
    parent: web_sys::Window,
    origin: String,
    mount_seq: JsValue,
    /// Whether the canvas intersects the viewport. Chromium draws no frames for a cross-origin
    /// iframe outside the viewport or under `display:none`, while `document.hidden` stays false.
    on_screen: Rc<Cell<bool>>,
}

impl Link {
    /// The framing page named by the URL's `ho`; none when the editor is not framed or `ho` is
    /// missing or a wildcard (replies would go to any page).
    pub fn from_page() -> Option<Self> {
        let window = web_sys::window()?;
        let search = window.location().search().ok()?;
        let query = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
        let origin = query.get("ho").filter(|o| !o.is_empty() && o != "*" && o != "null")?;
        let parent = window.parent().ok().flatten().filter(|p| !Object::is(p, &window))?;
        let mount_seq = query.get("ms").and_then(|s| s.parse::<f64>().ok()).map_or(JsValue::NULL, JsValue::from_f64);
        Some(Self { parent, origin, mount_seq, on_screen: Rc::new(Cell::new(true)) })
    }

    fn post(&self, msg: &Object) {
        if let Err(e) = self.parent.post_message(msg, &self.origin) {
            log::warn!("host bridge: postMessage failed: {e:?}");
        }
    }

    fn reply(&self, id: &JsValue, mount_seq: &JsValue, result: Result<String, HostError>) {
        let msg = Object::new();
        set(&msg, "id", id);
        set(&msg, "mountSeq", mount_seq);
        match result {
            Ok(text) => {
                set(&msg, "ok", &JsValue::TRUE);
                set(&msg, "result", &text.into());
            }
            Err(e) => {
                let error = Object::new();
                set(&error, "code", &e.code.as_str().into());
                set(&error, "message", &e.message.into());
                set(&msg, "ok", &JsValue::FALSE);
                set(&msg, "error", &error);
            }
        }
        self.post(&msg);
    }

    fn event(&self, method: &str, params: String) {
        let msg = Object::new();
        set(&msg, "method", &method.into());
        set(&msg, "params", &params.into());
        set(&msg, "mountSeq", &self.mount_seq);
        self.post(&msg);
    }
}

fn set(o: &Object, key: &str, v: &JsValue) {
    let _ = Reflect::set(o, &key.into(), v);
}

fn get(o: &JsValue, key: &str) -> JsValue {
    Reflect::get(o, &key.into()).unwrap_or(JsValue::UNDEFINED)
}

fn page_hidden() -> bool {
    web_sys::window().and_then(|w| w.document()).is_some_and(|d| d.hidden())
}

/// A `ui.*` request waiting for the frame that answers it.
struct Pending {
    id: JsValue,
    mount_seq: JsValue,
    reply: Receiver<ControlReply>,
}

/// The host API's state in the running app.
pub struct Embed {
    link: Link,
    host: Host,
    pending: Vec<Pending>,
    frames: u32,
}

impl Embed {
    pub fn new(link: Link) -> Self {
        Self { link, host: Host::default(), pending: Vec::new(), frames: 0 }
    }

    /// Answer one request at once (or queue a `ui.*` one for the next frame), then report a
    /// change it made to the document.
    fn message(&mut self, fig: &mut FigApp, ctx: &egui::Context, data: &JsValue) {
        let (id, mount_seq) = (get(data, "id"), get(data, "mountSeq"));
        let method = get(data, "method").as_string().unwrap_or_default();
        let params = get(data, "params").as_string().unwrap_or_default();
        let no_frames = page_hidden() || !self.link.on_screen.get();
        match self.host.call(fig, ctx, &method, &params, no_frames) {
            Answer::Now(result) => self.link.reply(&id, &mount_seq, result),
            Answer::Later(reply) => self.pending.push(Pending { id, mount_seq, reply }),
        }
        self.report(fig);
    }

    /// After each frame: answer the `ui.*` requests it ran, say when the editor is ready, and
    /// report a change the user made in it.
    pub fn after_frame(&mut self, fig: &FigApp) {
        let link = &self.link;
        self.pending.retain(|p| match p.reply.try_recv() {
            Ok(r) => {
                let result = r.result.map(|v| v.to_string()).map_err(|message| HostError { code: Code::EngineError, message });
                link.reply(&p.id, &p.mount_seq, result);
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                link.reply(&p.id, &p.mount_seq, Err(HostError { code: Code::EngineError, message: "the request was dropped".into() }));
                false
            }
        });
        // The first frame only installs fonts and draws nothing; the second is the first painted.
        self.frames = self.frames.saturating_add(1);
        if self.frames == 2 {
            self.host.mark_ready(&fig.session);
            self.link.event("event.ready", "{}".into());
        }
        self.report(fig);
    }

    fn report(&mut self, fig: &FigApp) {
        if let Some(state) = self.host.changed(&fig.session) {
            self.link.event("event.dirtyChanged", state.to_json());
        }
    }
}

/// Answer the framing page's messages synchronously against the running app, and follow whether
/// `canvas` is on screen.
pub fn listen(runner: eframe::WebRunner, link: Link, canvas: &web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or("no window")?;
    let on_screen = link.on_screen.clone();
    let on_intersect = Closure::<dyn FnMut(js_sys::Array)>::new(move |entries: js_sys::Array| {
        if let Some(e) = entries.iter().last().and_then(|e| e.dyn_into::<web_sys::IntersectionObserverEntry>().ok()) {
            on_screen.set(e.is_intersecting());
        }
    });
    web_sys::IntersectionObserver::new(on_intersect.as_ref().unchecked_ref())?.observe(canvas);
    on_intersect.forget();
    let on_message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |e: web_sys::MessageEvent| {
        if e.origin() != link.origin || !e.source().is_some_and(|s| Object::is(&s, &link.parent)) {
            return;
        }
        let data = e.data();
        let Some(mut shell) = runner.app_mut::<crate::web::WebShell>() else {
            let error = HostError { code: Code::EngineError, message: "the editor is not running".into() };
            link.reply(&get(&data, "id"), &get(&data, "mountSeq"), Err(error));
            return;
        };
        let shell = &mut *shell;
        if let Some(embed) = shell.embed.as_mut() {
            embed.message(&mut shell.fig, &shell.ctx, &data);
        }
    });
    window.add_event_listener_with_callback("message", on_message.as_ref().unchecked_ref())?;
    on_message.forget();
    Ok(())
}

/// Tell the framing page when the editor panics (after eframe's own handler has logged it).
/// The hook must be `Send`, which browser handles are not, so it finds the page again.
pub fn report_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if let Some(link) = Link::from_page() {
            link.event("event.crashed", serde_json::json!({"reason": info.to_string()}).to_string());
        }
    }));
}
