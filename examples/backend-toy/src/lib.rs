//! A `Backend` that forwards each call as a host-API envelope (`params` and `result` as JSON
//! text) through a transport, as a host does through its iframe bridge.

use base64::Engine as _;
use figcraft_mcp::Backend;
use serde_json::{Value, json};

/// Sends one request envelope and returns the reply envelope (both JSON text).
pub type Transport = Box<dyn FnMut(&str) -> String>;

pub struct Bridge {
    transport: Transport,
    next_id: u64,
}

impl Bridge {
    pub fn new(transport: Transport) -> Self {
        Self { transport, next_id: 1 }
    }

    fn request(&mut self, method: &str, params: &Value) -> Result<Value, String> {
        let id = self.next_id;
        self.next_id += 1;
        let envelope = json!({"id": id, "method": method, "params": params.to_string(), "mountSeq": 1});
        let reply: Value = serde_json::from_str(&(self.transport)(&envelope.to_string())).map_err(|e| e.to_string())?;
        if reply["ok"].as_bool() == Some(true) {
            serde_json::from_str(reply["result"].as_str().unwrap_or("null")).map_err(|e| e.to_string())
        } else {
            // The typed error travels as the JSON text of `{code, message}`.
            Err(reply["error"].to_string())
        }
    }
}

impl Backend for Bridge {
    fn call(&mut self, method: &str, params: Value) -> Result<Value, String> {
        self.request(method, &params)
    }

    fn has_ui(&self) -> bool {
        true
    }

    fn describe(&self) -> String {
        "toy bridge".into()
    }

    fn render_png(&mut self, ids: &[u64], scale: f64) -> Result<Vec<u8>, String> {
        let mut params = json!({"scale": scale});
        if !ids.is_empty() {
            params["ids"] = json!(ids);
        }
        let b64 = self.request("host.renderPng", &params)?;
        base64::engine::general_purpose::STANDARD.decode(b64.as_str().unwrap_or_default()).map_err(|e| e.to_string())
    }
}
