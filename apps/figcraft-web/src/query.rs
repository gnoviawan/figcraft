//! The page's URL: the shell its query asks for and the invite link its fragment names.

/// The invite link in `#join=<link>` (`fragment` is `location.hash`, with or without its `#`), still
/// escaped. The fragment, unlike the query, never leaves the browser, so the link's secret stays out
/// of the web host's logs and `Referer` headers. `join` takes the rest of the fragment, so a pasted
/// `figcraft://join?host=…&file=…&token=…` keeps its own `&`s. Anything longer than an escaped invite
/// link can be is ignored; what is left is checked by `collab.join` like a link the desktop app is
/// opened with.
pub fn join_link(fragment: &str) -> Option<&str> {
    // An invite link is at most 2 KiB; every byte of it escaped is three.
    const MAX: usize = 3 * 2048;
    let fragment = fragment.strip_prefix('#').unwrap_or(fragment);
    let start = if fragment.starts_with("join=") { 0 } else { fragment.find("&join=")? + 1 };
    fragment.get(start + "join=".len()..).filter(|link| !link.is_empty() && link.len() <= MAX)
}

/// The shell the page's query (`location.search`, with or without its `?`) asks for: `embed=kiosk`
/// as a whole entry gives the kiosk, anything else the full editor.
pub fn shell(search: &str) -> figcraft_ui_egui::Shell {
    let query = search.strip_prefix('?').unwrap_or(search);
    if query.split('&').any(|entry| entry == "embed=kiosk") { figcraft_ui_egui::Shell::Kiosk } else { figcraft_ui_egui::Shell::Full }
}
