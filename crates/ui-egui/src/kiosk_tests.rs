//! The kiosk shell: which host calls it refuses, and what it takes off the screen and out of the
//! app's own timers.

use std::cell::Cell;
use std::rc::Rc;

use egui::accesskit::Role;
use egui::{Event, Pos2};
use figcraft_engine::Session;
use serde_json::json;

use crate::layers_drag_tests::Ui;
use crate::{DialogFuture, FigApp, NoServices, Services, Shell};

fn ui(shell: Shell) -> Ui {
    let mut u = Ui::new(Session::new());
    u.app.shell = shell;
    for _ in 0..3 {
        u.frame(vec![]);
    }
    u
}

fn canvas_top(u: &Ui) -> f64 {
    u.app.inspect()["canvas"]["rect"][1].as_f64().unwrap()
}

fn tab_labels(u: &mut Ui) -> Vec<String> {
    u.ctx.enable_accesskit();
    u.frame(vec![]);
    let update = u.frame_output(vec![]).platform_output.accesskit_update.unwrap();
    update.nodes.into_iter().map(|(_, n)| n).filter(|n| n.role() == Role::Tab).filter_map(|n| n.label().map(str::to_string)).collect()
}

fn click_logo(u: &mut Ui, top: f32) {
    let p = Pos2::new(28.0, top + 24.0);
    u.button(p, true);
    u.button(p, false);
}

#[test]
fn kiosk_refuses_the_calls_that_reopen_the_shell_or_leave_the_document() {
    let refused = [
        ("app.quit", json!({})),
        ("window.new", json!({})),
        ("window.list", json!({})),
        ("window.focus", json!({"id": 1})),
        ("ui.set", json!({"home": true})),
        ("ui.set", json!({"mainMenu": "Plugins"})),
        ("ui.set", json!({"rightTab": "design", "homeSection": "templates"})),
        ("ui.menu.invoke", json!({"id": "file.open"})),
        ("ui.menu.invoke", json!({"id": "file.import"})),
        ("ui.menu.invoke", json!({"id": "file.save"})),
        ("ui.menu.invoke", json!({"id": "window.new"})),
        ("engine.execute", json!({"command": "window.new"})),
    ];
    for (method, params) in refused {
        assert!(Shell::Kiosk.rejects(method, &params), "{method} {params}");
        assert!(!Shell::Full.rejects(method, &params), "{method} {params} outside kiosk");
    }
}

#[test]
fn kiosk_keeps_the_calls_the_host_drives_the_editor_with() {
    let kept = [
        ("engine.execute", json!({"command": "shape.rectangle", "params": {}})),
        ("engine.commands", json!({})),
        ("ui.set", json!({"rightTab": "inspect", "leftTab": "assets"})),
        ("ui.menu.invoke", json!({"id": "edit.undo"})),
        ("ui.inspect", json!({})),
        ("ui.screenshot", json!({})),
        ("document.inspect", json!({})),
    ];
    for (method, params) in kept {
        assert!(!Shell::Kiosk.rejects(method, &params), "{method} {params}");
    }
}

#[test]
fn the_shell_is_full_unless_asked() {
    assert_eq!(Shell::default(), Shell::Full);
    assert!(!FigApp::new(Session::new(), Box::new(NoServices)).shell.is_kiosk());
}

#[test]
fn kiosk_draws_no_tab_bar_and_keeps_the_editor_panels() {
    let mut full = ui(Shell::Full);
    assert!(tab_labels(&mut full).contains(&"Home".to_string()), "the tab bar's Home tab shows outside kiosk");
    assert_eq!(canvas_top(&full), f64::from(crate::tabs::HEIGHT));

    let mut kiosk = ui(Shell::Kiosk);
    assert!(!tab_labels(&mut kiosk).contains(&"Home".to_string()));
    assert_eq!(canvas_top(&kiosk), 0.0, "the canvas starts at the top of the frame");
    assert!(kiosk.app.inspect()["panels"]["left"].as_f64().unwrap() > 0.0, "layers panel is still there");
}

#[test]
fn the_main_menu_button_opens_the_menu_outside_kiosk_and_is_gone_in_kiosk() {
    let mut full = ui(Shell::Full);
    click_logo(&mut full, crate::tabs::HEIGHT);
    assert!(full.app.ui.main_menu);

    let mut kiosk = ui(Shell::Kiosk);
    click_logo(&mut kiosk, 0.0);
    kiosk.frame(vec![]);
    assert!(!kiosk.app.ui.main_menu);
}

#[test]
fn kiosk_never_shows_the_home_screen_or_the_main_menu() {
    let mut u = ui(Shell::Kiosk);
    u.app.ui.home = true;
    u.app.ui.main_menu = true;
    u.frame(vec![]);
    assert!(!u.app.ui.home && !u.app.ui.main_menu);
    u.app.run("doc.home", json!({}));
    u.frame(vec![]);
    assert!(!u.app.ui.home, "a command that asks for the home screen doesn't get it");
    u.app.run("file.close", json!({"force": true}));
    u.frame(vec![]);
    assert!(!u.app.ui.home, "closing the last file leaves a blank canvas, not the home screen");
}

#[test]
fn outside_kiosk_the_home_command_still_shows_the_home_screen() {
    let mut u = ui(Shell::Full);
    u.app.run("doc.home", json!({}));
    assert!(u.app.ui.home);
}

/// Services that count the file dialogs asked for.
struct Dialogs(Rc<Cell<u32>>);

impl Services for Dialogs {
    fn pick_open(&mut self, _: &str, _: &[&str]) -> DialogFuture {
        self.0.set(self.0.get() + 1);
        Box::pin(std::future::ready(None))
    }
    fn pick_save(&mut self, _: &str, _: &str, _: &[&str]) -> DialogFuture {
        self.0.set(self.0.get() + 1);
        Box::pin(std::future::ready(None))
    }
}

fn asked_for_dialogs(shell: Shell) -> u32 {
    let asked = Rc::new(Cell::new(0));
    let mut u = ui(shell);
    u.app.services = Box::new(Dialogs(asked.clone()));
    for item in ["file.open", "file.import", "file.placeImage", "file.saveAs", "file.save"] {
        crate::menus::invoke(&mut u.app, item);
        u.frame(vec![]);
    }
    let cmd_o = Event::Key { key: egui::Key::O, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND };
    u.frame(vec![cmd_o]);
    asked.get()
}

#[test]
fn kiosk_opens_no_file_dialog_and_outside_kiosk_still_does() {
    assert_eq!(asked_for_dialogs(Shell::Kiosk), 0);
    assert!(asked_for_dialogs(Shell::Full) >= 3);
}

#[test]
fn kiosk_takes_no_autosave_checkpoint_and_asks_nothing_about_saving() {
    let mut u = ui(Shell::Kiosk);
    u.app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10}));
    let before = u.app.session.ds().unwrap().versions.len();
    u.app.ui.next_autosave = 0.001;
    for _ in 0..4 {
        u.frame(vec![]);
    }
    assert_eq!(u.app.session.ds().unwrap().versions.len(), before, "no version.autosave checkpoint");

    u.app.save();
    u.app.save_as_dialog();
    u.frame(vec![Event::Key { key: egui::Key::S, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::COMMAND }]);
    assert!(u.app.ui.toast.is_none(), "saving shows no message");
}

#[test]
fn outside_kiosk_the_autosave_checkpoint_still_runs() {
    let mut u = ui(Shell::Full);
    u.app.run("shape.rectangle", json!({"x": 0, "y": 0, "width": 10, "height": 10}));
    let before = u.app.session.ds().unwrap().versions.len();
    u.app.ui.next_autosave = 0.001;
    for _ in 0..4 {
        u.frame(vec![]);
    }
    assert_eq!(u.app.session.ds().unwrap().versions.len(), before + 1);
}

/// A browser's services: files leave as downloads.
struct Downloads(Rc<Cell<u32>>);

impl Services for Downloads {
    fn downloads(&self) -> bool {
        true
    }
    fn download(&mut self, _: &str, _: &[u8]) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }
}

fn downloaded(shell: Shell) -> u32 {
    let sent = Rc::new(Cell::new(0));
    let mut u = ui(shell);
    u.app.services = Box::new(Downloads(sent.clone()));
    crate::variables_panel::export_dialog(&mut u.app, "json");
    sent.get()
}

#[test]
fn kiosk_hands_nothing_over_as_a_download_and_the_browser_still_does_outside_it() {
    assert_eq!(downloaded(Shell::Kiosk), 0);
    assert_eq!(downloaded(Shell::Full), 1);
}
