//! Emoji, under the `em` prefix.
//!
//! The smallest useful extension, and the one to copy when starting a new one:
//! a fixed list Centrepiece filters and ranks, a pick that copies, and a couple
//! of rows reachable from the root without the prefix.

use centrepiece_extension::{Extension, Item, Response, Screen, export_extension, host};

/// The glyph, and the words it is found by.
const EMOJI: &[(&str, &str)] = &[
    ("👍", "thumbs up yes approve"),
    ("👎", "thumbs down no"),
    ("🎉", "party tada celebrate"),
    ("🙏", "pray thanks please"),
    ("😂", "joy laugh tears"),
    ("🙂", "smile slightly"),
    ("😅", "sweat smile phew"),
    ("🤔", "thinking hmm"),
    ("👀", "eyes look"),
    ("🔥", "fire hot lit"),
    ("✅", "check done yes"),
    ("❌", "cross no wrong"),
    ("⚠️", "warning caution"),
    ("🚀", "rocket ship launch"),
    ("🐛", "bug insect"),
    ("❤️", "heart love red"),
    ("💯", "hundred perfect"),
    ("🤷", "shrug dunno"),
    ("👋", "wave hello hi bye"),
    ("🥒", "pickle cucumber"),
];

struct Emoji;

fn item(glyph: &str, words: &str) -> Item {
    // The id is the glyph itself: picking a row copies its id.
    Item::new(glyph, words).glyph(glyph.chars().next().unwrap_or('?'))
}

impl Extension for Emoji {
    fn new() -> Self {
        Emoji
    }

    fn started(&mut self) {
        // Found from the root: typing `shrug` offers 🤷 without `em ` first.
        host::set_shortcuts(&[
            item("🤷", "shrug").subtitle("Copy 🤷"),
            item("👍", "thumbs up").subtitle("Copy 👍"),
        ]);
    }

    fn activate(&mut self) -> Response {
        let items = EMOJI
            .iter()
            .map(|(glyph, words)| item(glyph, words))
            .collect();
        Response::Replace(
            Screen::search(items)
                .placeholder("Search emoji")
                .host_filtered(),
        )
    }

    fn select(&mut self, id: &str) -> Response {
        host::copy(id);
        Response::Dismiss
    }
}

export_extension!(Emoji);
