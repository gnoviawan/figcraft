use super::*;
use crate::i18n::{Text, languages};

/// The About strings a rebranded build doesn't show (it shows [`ATTRIBUTION`] instead).
const ABOUT_ONLY: &[&str] = &[
    "Dual-licensed MIT OR Apache-2.0. Made by the ArtCraft team and community.",
    "Interface design and prototyping. A free, open-source, clean-room design tool written from scratch in Rust, part of the ArtCraft Crafting Apps.",
];

fn forms(text: &Text) -> Vec<&'static str> {
    match text {
        Text::Plain(s) => vec![s],
        Text::Plural { one, few, many, other } => [*one, *few, *many, Some(*other)].into_iter().flatten().collect(),
    }
}

/// Every catalog string in every language, with its language and key.
fn catalog() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    languages().iter().flat_map(|l| {
        [l.ui, l.commands, l.menu].into_iter().flatten().flat_map(move |(key, text)| forms(text).into_iter().map(move |s| (l.code, *key, s)))
    })
}

#[test]
fn the_catalogs_name_this_build() {
    let english = languages().first().expect("English comes first");
    let about = english.ui.iter().find(|(k, _)| *k == "About FigCraft").map(|(_, t)| forms(t)).expect("About is in the English catalog");
    assert_eq!(about, vec![format!("About {NAME}")]);
    let named = catalog().filter(|(_, key, _)| key.contains(UPSTREAM)).filter(|(.., text)| text.contains(NAME)).count();
    assert!(named > 100, "only {named} strings say {NAME}");
}

#[test]
fn a_rebranded_build_shows_no_upstream_name_or_artcraft_mark() {
    let marked: Vec<_> = catalog()
        .filter(|(_, key, _)| !ABOUT_ONLY.contains(key))
        .filter(|(.., text)| text.contains(UPSTREAM) || text.contains("ArtCraft"))
        .map(|(code, key, _)| format!("{code}: {key}"))
        .collect();
    if REBRANDED {
        assert!(marked.is_empty(), "{marked:#?}");
    } else {
        assert!(!marked.is_empty(), "the upstream build keeps its catalogs as written");
    }
}

#[test]
fn engine_labels_take_this_builds_name() {
    assert_eq!(rename("About FigCraft"), format!("About {NAME}"));
    assert_eq!(rename("Rectangle"), "Rectangle");
}
