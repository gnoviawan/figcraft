//! The product name the UI shows. A build with `FIGCRAFT_BRAND_NAME` set ships under that name:
//! catalog strings say it instead of "FigCraft" (build.rs fills it in), and About drops the
//! ArtCraft name, links and mark for a plain-text attribution, as docs/brand/LICENSE-brand.txt
//! asks of modified versions.

use std::borrow::Cow;

/// Upstream's name. Catalog keys and the English text in the code keep it.
pub const UPSTREAM: &str = "FigCraft";

/// The name this build shows.
pub const NAME: &str = match option_env!("FIGCRAFT_BRAND_NAME") {
    Some(name) if !name.is_empty() => name,
    _ => UPSTREAM,
};

/// Built under another name than upstream's.
pub const REBRANDED: bool = !same(NAME, UPSTREAM);

/// What About says in a rebranded build, in place of the ArtCraft name and links.
pub const ATTRIBUTION: &str = "Based on FigCraft by the ArtCraft team.";

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// `text` with upstream's name replaced by this build's: for English text that doesn't come from
/// a catalog (the engine's command labels).
pub fn rename(text: &str) -> Cow<'_, str> {
    if REBRANDED && text.contains(UPSTREAM) { Cow::Owned(text.replace(UPSTREAM, NAME)) } else { Cow::Borrowed(text) }
}

#[cfg(test)]
#[path = "brand_tests.rs"]
mod tests;
