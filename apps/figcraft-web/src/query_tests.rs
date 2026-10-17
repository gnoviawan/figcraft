//! `#join=` in the page's URL.

use crate::query::{join_link, shell};

#[test]
fn join_takes_the_rest_of_the_fragment() {
    let link = "figcraft://join?host=192.168.1.20:47900&file=abc&token=AbCdEfGhIjKlMnOpQrStUv";
    assert_eq!(join_link(&format!("#join={link}")), Some(link), "a pasted link keeps its own &s");
    assert_eq!(join_link(&format!("#x=1&join={link}")), Some(link), "after other entries");
    assert_eq!(join_link("#join=figcraft%3A%2F%2Fjoin%3Fhost%3Dh"), Some("figcraft%3A%2F%2Fjoin%3Fhost%3Dh"), "escaped, decoded later");
    assert_eq!(join_link("join=K7M-2QX-9TD"), Some("K7M-2QX-9TD"), "a join code, no leading #");
}

#[test]
fn no_join_without_the_entry() {
    assert_eq!(join_link(""), None);
    assert_eq!(join_link("#"), None);
    assert_eq!(join_link("#join="), None);
    assert_eq!(join_link("#rejoin=x"), None, "only the whole entry name");
    assert_eq!(join_link("#xjoin=x&a=1"), None);
    assert_eq!(join_link("?join=x"), None, "the query isn't read: it goes to the web host");
}

#[test]
fn an_overlong_link_is_ignored() {
    let long = format!("#join=figcraft://join?host=h&file=f&token={}", "a".repeat(7000));
    assert_eq!(join_link(&long), None);
    assert_eq!(join_link(&format!("#join={}", "é".repeat(4000))), None, "length counts bytes");
}

#[test]
fn the_kiosk_shell_needs_the_whole_embed_entry() {
    use figcraft_ui_egui::Shell;
    assert_eq!(shell("?embed=kiosk"), Shell::Kiosk);
    assert_eq!(shell("embed=kiosk"), Shell::Kiosk, "without the ?");
    assert_eq!(shell("?ho=https%3A%2F%2Fa.example&embed=kiosk&ms=3"), Shell::Kiosk, "among other entries");
    for other in ["", "?", "?webgl", "?embed=", "?embed=Kiosk", "?embed=kiosk2", "?xembed=kiosk", "?embed=full&x=kiosk", "?x=embed%3Dkiosk"] {
        assert_eq!(shell(other), Shell::Full, "{other:?}");
    }
}
