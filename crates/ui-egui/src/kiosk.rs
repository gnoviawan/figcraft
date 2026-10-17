//! The kiosk shell: the editor embedded in another app's window. A host page owns everything
//! outside the canvas (tabs, home, menus, files, saving), so the editor keeps its layers,
//! inspector and toolbar and gives up what would reopen that shell or take the document away.

use serde_json::Value;

/// How much of the editor's own shell shows. Chosen once, when the page loads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shell {
    #[default]
    Full,
    Kiosk,
}

/// Commands and methods that quit the app or open another window.
fn leaves_the_document(name: &str) -> bool {
    name == "app.quit" || name.starts_with("window.")
}

/// Menu items that open a file dialog or save the file where the editor chooses.
const FILE_MENU_ITEMS: &[&str] =
    &["file.open", "file.openRecent", "file.import", "file.placeImage", "file.save", "file.saveAs", "plugin.import"];

/// Requests a command raises for the UI that bring back the shell, open a window or ask about
/// saving.
const SHELL_REQUESTS: &[&str] = &["home", "home.templates", "home.gettingStarted", "dialog.saveAs", "dialog.unsaved"];

impl Shell {
    pub fn is_kiosk(self) -> bool {
        self == Shell::Kiosk
    }

    /// Whether a host call must be answered `UNKNOWN_METHOD`: the control `method` itself, or the
    /// command, menu item or UI state it names in `params`.
    pub fn rejects(self, method: &str, params: &Value) -> bool {
        if !self.is_kiosk() {
            return false;
        }
        let text = |key: &str| params.get(key).and_then(Value::as_str).unwrap_or_default();
        match method {
            "engine.execute" => leaves_the_document(text("command")),
            "ui.menu.invoke" => leaves_the_document(text("id")) || FILE_MENU_ITEMS.contains(&text("id")),
            "ui.set" => params.as_object().is_some_and(|p| p.keys().any(|k| k == "mainMenu" || k.starts_with("home"))),
            _ => leaves_the_document(method),
        }
    }

    /// Whether a UI request raised by a command is dropped instead of acted on.
    pub fn drops_request(self, kind: &str) -> bool {
        self.is_kiosk() && (kind.starts_with("window.") || SHELL_REQUESTS.contains(&kind))
    }
}
