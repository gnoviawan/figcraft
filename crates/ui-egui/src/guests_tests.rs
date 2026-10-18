use egui::{Color32, Rect, pos2, vec2};
use figcraft_doc::NodeId;

use crate::guests::{FlashKind, Guests, Placement, placement, ring_look};

const BLUE: Color32 = Color32::from_rgb(0x3b, 0x82, 0xf6);
const PINK: Color32 = Color32::from_rgb(0xec, 0x48, 0x99);

/// Frames at 60 Hz from `from` to `to` (fake clock); the time reached.
fn run(g: &mut Guests, from: f64, to: f64, reduce: bool) -> f64 {
    let mut t = from;
    while t < to {
        t = (t + 1.0 / 60.0).min(to);
        g.step(t, reduce, 1.0);
    }
    t
}

fn gap(a: (f64, f64), b: (f64, f64)) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

#[test]
fn a_new_guest_appears_at_its_target_and_fades_in() {
    let mut g = Guests::default();
    g.cursor(10.0, "s1", "Claude", BLUE, (100.0, 50.0), None);
    let guest = g.get("s1").unwrap();
    assert_eq!(guest.shown, (100.0, 50.0), "no fly-in from elsewhere");
    assert_eq!(guest.opacity(10.0), 0.0);
    assert!((0.1..1.0).contains(&guest.opacity(10.1)), "{}", guest.opacity(10.1));
    assert_eq!(guest.opacity(10.2), 1.0);
}

#[test]
fn a_known_guest_glides_slowly_toward_a_new_target() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    run(&mut g, 0.0, 1.0, false);
    g.cursor(1.0, "s1", "Claude", BLUE, (1000.0, 0.0), None);
    run(&mut g, 1.0, 1.5, false);
    // Exponential approach at 2.5/s: 1 - e^(-1.25) of the way after half a second.
    let x = g.get("s1").unwrap().shown.0;
    assert!((650.0..780.0).contains(&x), "x after 0.5 s: {x}");
    run(&mut g, 1.5, 4.5, false);
    assert_eq!(g.get("s1").unwrap().shown, (1000.0, 0.0), "settles on the target");
}

#[test]
fn reduced_motion_jumps_and_never_drifts() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    g.cursor(1.0, "s1", "Claude", BLUE, (500.0, 300.0), None);
    g.step(1.0 + 1.0 / 60.0, true, 1.0);
    assert_eq!(g.get("s1").unwrap().shown, (500.0, 300.0));
    run(&mut g, 1.02, 3.0, true);
    assert_eq!(g.get("s1").unwrap().wobble(3.0), vec2(0.0, 0.0));
}

#[test]
fn a_pointer_at_rest_drifts_a_few_points_and_stops_drifting_when_it_moves() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    let t = run(&mut g, 0.0, 2.0, false);
    let drift = (0..60).map(|i| g.get("s1").unwrap().wobble(t + f64::from(i) * 0.05).length()).fold(0.0f32, f32::max);
    assert!((1.0..=6.0).contains(&drift), "drift {drift}");
    g.cursor(2.0, "s1", "Claude", BLUE, (800.0, 0.0), None);
    let t = run(&mut g, 2.0, 2.8, false);
    assert!(g.get("s1").unwrap().wobble(t).length() < 0.5, "a gliding pointer does not drift");
}

#[test]
fn an_idle_guest_fades_out_after_five_seconds_and_is_dropped() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    run(&mut g, 0.0, 4.9, false);
    assert_eq!(g.get("s1").unwrap().opacity(4.9), 1.0);
    run(&mut g, 4.9, 5.75, false);
    let half = g.get("s1").unwrap().opacity(5.75);
    assert!((0.4..0.6).contains(&half), "halfway through the 1.5 s fade: {half}");
    run(&mut g, 5.75, 6.6, false);
    assert!(g.get("s1").is_none());
    assert!(g.is_empty());
}

#[test]
fn updates_keep_a_guest_alive() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    for i in 1..=4 {
        let t = f64::from(i) * 4.0;
        run(&mut g, t - 4.0, t, false);
        g.cursor(t, "s1", "Claude", BLUE, (t, 0.0), None);
    }
    run(&mut g, 16.0, 20.0, false);
    assert_eq!(g.get("s1").unwrap().opacity(20.0), 1.0);
}

#[test]
fn hide_fades_over_a_second_and_a_half_and_drops_the_guest() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    g.hide(1.0, "s1");
    let mid = g.get("s1").unwrap().opacity(1.75);
    assert!((0.4..0.6).contains(&mid), "{mid}");
    g.step(2.5, false, 1.0);
    assert!(g.get("s1").is_none());
}

#[test]
fn an_update_during_the_fade_brings_the_guest_back_without_a_pop() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    g.hide(1.0, "s1");
    let before = g.get("s1").unwrap().opacity(1.75);
    g.cursor(1.75, "s1", "Claude", BLUE, (10.0, 0.0), None);
    let guest = g.get("s1").unwrap();
    assert!((guest.opacity(1.75) - before).abs() < 0.01, "continues from {before}, got {}", guest.opacity(1.75));
    assert_eq!(guest.opacity(2.0), 1.0);
    assert_eq!(guest.opacity(6.5), 1.0, "no longer hiding");
}

#[test]
fn unknown_guests_are_ignored_by_flash_and_hide() {
    let mut g = Guests::default();
    g.flash(0.0, "nobody", &[NodeId(1)], FlashKind::Write);
    g.hide(0.0, "nobody");
    assert!(g.is_empty());
}

#[test]
fn flashes_ring_for_their_timeline_then_go() {
    let mut g = Guests::default();
    g.cursor(0.0, "s1", "Claude", BLUE, (0.0, 0.0), None);
    g.flash(1.0, "s1", &[NodeId(7), NodeId(8)], FlashKind::Write);
    g.flash(1.0, "s1", &[NodeId(9)], FlashKind::Read);
    let kinds = |g: &Guests| g.get("s1").unwrap().flashes().map(|(n, k, _)| (n.0, k)).collect::<Vec<_>>();
    assert_eq!(kinds(&g), vec![(7, FlashKind::Write), (8, FlashKind::Write), (9, FlashKind::Read)]);
    g.step(1.95, false, 1.0);
    assert_eq!(kinds(&g), vec![(9, FlashKind::Read)], "a write ring lasts 0.9 s");
    g.step(3.05, false, 1.0);
    assert!(kinds(&g).is_empty(), "a read ring lasts 2 s");
}

#[test]
fn a_write_ring_grows_in_holds_and_releases() {
    let w = |t| ring_look(FlashKind::Write, t, false);
    assert!(w(-0.01).is_none());
    let start = w(0.0).unwrap();
    assert!(start.out > 20.0 && start.alpha == 0.0, "{start:?}");
    assert_eq!((w(0.4).unwrap().out, w(0.4).unwrap().alpha), (0.0, 1.0), "held on the node");
    let late = w(0.85).unwrap();
    assert!(late.out > 0.0 && late.alpha < 0.3, "drifts out and fades: {late:?}");
    assert!(w(0.91).is_none());
    assert_eq!(w(0.4).unwrap().scan, None, "only reads scan");
}

#[test]
fn a_read_ring_holds_longer_and_scans() {
    let r = |t| ring_look(FlashKind::Read, t, false);
    assert!(r(1.5).unwrap().scan.is_some());
    assert_ne!(r(0.5).unwrap().scan, r(0.8).unwrap().scan, "the band moves");
    assert!(r(1.99).is_some());
    let still = ring_look(FlashKind::Read, 0.05, true).unwrap();
    assert_eq!((still.out, still.scan), (0.0, None), "reduced motion keeps only the fade");
}

#[test]
fn two_guests_are_independent() {
    let mut g = Guests::default();
    g.cursor(0.0, "a", "Claude", BLUE, (0.0, 0.0), None);
    g.cursor(0.0, "b", "opencode", PINK, (500.0, 500.0), None);
    g.cursor(1.0, "a", "Claude", BLUE, (100.0, 0.0), None);
    g.hide(1.0, "b");
    run(&mut g, 0.0, 3.5, false);
    assert_eq!(g.get("a").unwrap().shown, (100.0, 0.0));
    assert_eq!(g.get("a").unwrap().color, BLUE);
    assert!(g.get("b").is_none());
    assert_eq!(gap(g.get("a").unwrap().target, (100.0, 0.0)), 0.0);
}

#[test]
fn an_off_screen_pointer_shows_as_an_arrow_on_the_edge_toward_it() {
    let canvas = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0));
    assert_eq!(placement(canvas, pos2(300.0, 300.0)), Placement::OnCanvas(pos2(300.0, 300.0)));
    let Placement::Edge { at, dir } = placement(canvas, pos2(2000.0, 350.0)) else { panic!("expected an edge arrow") };
    assert!(canvas.contains(at), "{at:?}");
    assert!((at.x - (canvas.max.x - 18.0)).abs() < 0.5, "on the right edge: {at:?}");
    assert!(dir.x > 0.9, "points right: {dir:?}");
    let Placement::Edge { at, dir } = placement(canvas, pos2(500.0, -400.0)) else { panic!("expected an edge arrow") };
    assert!((at.y - (canvas.min.y + 18.0)).abs() < 0.5 && dir.y < 0.0, "top edge pointing up: {at:?} {dir:?}");
}
