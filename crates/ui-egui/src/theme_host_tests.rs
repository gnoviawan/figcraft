use egui::{Color32, Context, RawInput, Theme};
use figcraft_engine::Session;
use serde_json::json;

use super::*;
use crate::{FigApp, NoServices};

fn hex(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// Termul's dark theme.
fn termul_dark() -> HostPalette {
    HostPalette {
        dark: true,
        background: hex(0x08090A),
        card: hex(0x0F1011),
        foreground: hex(0xE5E5E6),
        muted_foreground: hex(0x8A8F98),
        border: hex(0x23252A),
        primary: hex(0x5B9BFF),
        primary_fill: hex(0x3B82F6),
        destructive: hex(0xF0605D),
    }
}

/// Termul's light theme.
fn termul_light() -> HostPalette {
    HostPalette {
        dark: false,
        background: hex(0xFFFFFF),
        card: hex(0xFFFFFF),
        foreground: hex(0x0D0D0D),
        muted_foreground: hex(0x6B6B6B),
        border: hex(0xE6E6E6),
        primary: hex(0x2563EB),
        primary_fill: hex(0x3B82F6),
        destructive: hex(0xC0262D),
    }
}

fn luma(c: Color32) -> u32 {
    u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b())
}

#[test]
fn host_roles_land_on_the_tokens_that_draw_them() {
    for p in [termul_dark(), termul_light()] {
        let t = Tokens::from_host(&p);
        assert_eq!(t.dark, p.dark);
        assert_eq!((t.canvas, t.tab_bar), (p.background, p.background));
        assert_eq!((t.panel, t.bg, t.tab_active, t.menu), (p.card, p.card, p.card, p.card));
        assert_eq!((t.text, t.icon, t.menu_text, t.focus), (p.foreground, p.foreground, p.foreground, p.foreground));
        assert_eq!((t.text_secondary, t.icon_secondary, t.menu_dim), (p.muted_foreground, p.muted_foreground, p.muted_foreground));
        assert_eq!(t.border, p.border);
        assert_eq!((t.accent, t.accent_text, t.danger), (p.primary_fill, p.primary, p.destructive));
        let base = if p.dark { Tokens::dark() } else { Tokens::light() };
        assert_eq!((t.component, t.success, t.warning, t.shadow), (base.component, base.success, base.warning, base.shadow));
    }
}

#[test]
fn hover_and_field_shades_sit_between_the_panel_and_the_text() {
    for p in [termul_dark(), termul_light()] {
        let t = Tokens::from_host(&p);
        let (lo, hi) = (luma(p.card).min(luma(p.foreground)), luma(p.card).max(luma(p.foreground)));
        for (name, c) in [("hover", t.hover), ("pressed", t.pressed), ("field", t.field), ("selected row", t.selected_row)] {
            assert_ne!(c, p.card, "{name} must differ from the panel");
            assert!(name == "selected row" || (lo..=hi).contains(&luma(c)), "{name} {c:?}");
        }
        assert!(luma(t.hover).abs_diff(luma(p.card)) < luma(t.pressed).abs_diff(luma(p.card)), "pressed reads stronger than hover");
    }
}

fn frame(ctx: &Context, app: &mut FigApp) {
    let mut output = ctx.run_ui(RawInput { system_theme: Some(Theme::Light), ..Default::default() }, |ui| app.ui(ui));
    output.textures_delta.clear();
}

#[test]
fn host_colours_replace_the_preference_live_and_clearing_them_restores_it() {
    let ctx = Context::default();
    let mut app = FigApp::new(Session::empty(), Box::new(NoServices));
    app.session.execute("view.themeLight", &json!({})).unwrap();
    frame(&ctx, &mut app);
    assert!(app.theme_toggle_shown());

    let dark = Tokens::from_host(&termul_dark());
    app.ui.host_theme = Some(dark);
    app.apply_theme(&ctx);
    assert_eq!(Tokens::get(&ctx), dark, "applied at once, before any frame");
    assert!(ctx.global_style().visuals.dark_mode);
    assert_eq!(ctx.global_style().visuals.panel_fill, termul_dark().card);
    frame(&ctx, &mut app);
    assert_eq!(Tokens::get(&ctx), dark, "a frame keeps the host's colours over the light preference");
    assert!(app.ui.dark && !app.theme_toggle_shown());

    let light = Tokens::from_host(&termul_light());
    app.ui.host_theme = Some(light);
    frame(&ctx, &mut app);
    assert_eq!(Tokens::get(&ctx), light);
    assert!(!ctx.global_style().visuals.dark_mode);

    app.ui.host_theme = None;
    frame(&ctx, &mut app);
    assert_eq!(Tokens::get(&ctx), Tokens::light());
    assert!(app.theme_toggle_shown());
}
