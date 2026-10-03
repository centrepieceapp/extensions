//! Colors, under the `col` prefix.
//!
//! A color typed at the root — `#f80`, `rgb(255 136 0)`, `hsl(32, 100%, 50%)`
//! — gets a row with a swatch of it, and so does one on the clipboard. Picking
//! either opens a menu that copies the color as hex, rgb or hsl. Under `col`,
//! the same rows answer what is typed after the prefix.

mod color;

use centrepiece_extension::{Extension, Icon, Item, Response, Screen, export_extension, host};

use color::Color;

/// The id prefixes of the rows that stand for a color; the color's hex
/// follows, so picking one needs nothing remembered.
const TYPED: &str = "typed:";
const COPIED: &str = "copied:";

/// The ids of the menu's rows.
const COPY_HEX: &str = "hex";
const COPY_RGB: &str = "rgb";
const COPY_HSL: &str = "hsl";

struct Colors {
    /// The color on the clipboard as of the last summon.
    clipboard: Option<Color>,
    /// The color whose menu is showing.
    picked: Option<Color>,
}

/// The row for a color: its swatch, its hex, and the other forms beneath.
fn row(id_prefix: &str, color: Color, detail: &str) -> Item {
    Item::new(format!("{id_prefix}{}", color.hex()), color.hex())
        .subtitle(format!("{}  ·  {}", color.rgb(), color.hsl()))
        .detail(detail)
        .icon(Icon::Color(color.rgba()))
}

fn typed(color: Color) -> Item {
    row(TYPED, color, "Color")
}

fn copied(color: Color) -> Item {
    row(COPIED, color, "Clipboard")
}

/// The ways to copy `color`, each on a number key.
fn menu(color: Color) -> Screen {
    let rgb = if color.is_opaque() { "rgb" } else { "rgba" };
    let hsl = if color.is_opaque() { "hsl" } else { "hsla" };
    let items = [
        (COPY_HEX, "hex".to_string(), color.hex()),
        (COPY_RGB, rgb.to_string(), color.rgb()),
        (COPY_HSL, hsl.to_string(), color.hsl()),
    ]
    .into_iter()
    .zip('1'..)
    .map(|((id, form, value), key)| {
        Item::new(id, format!("Copy as {form}"))
            .subtitle(value)
            .icon(Icon::builtin("clipboard"))
            .key(key)
    })
    .collect();
    Screen::menu(color.hex(), items)
}

impl Colors {
    /// Under the prefix: the typed color, then the copied one, unless they
    /// are the same.
    fn results(&self, query: &str) -> Screen {
        let typed = Color::parse(query);
        let items: Vec<Item> = typed
            .map(self::typed)
            .into_iter()
            .chain(self.clipboard.filter(|&c| Some(c) != typed).map(copied))
            .collect();
        let status = if query.trim().is_empty() {
            "Type a color: #f80, rgb(255 136 0), hsl(32 100% 50%)"
        } else {
            "Not a color: try #f80, rgb(255 136 0) or hsl(32 100% 50%)"
        };
        Screen::search(items)
            .placeholder("#hex, rgb(), rgba() or hsl()")
            .status(status)
    }
}

impl Extension for Colors {
    fn new() -> Self {
        Self {
            clipboard: None,
            picked: None,
        }
    }

    fn summoned(&mut self) {
        self.clipboard = host::clipboard_text().and_then(|text| Color::parse(&text));
        let offers: Vec<Item> = self.clipboard.map(copied).into_iter().collect();
        host::set_offers(&offers);
    }

    fn activate(&mut self) -> Response {
        // `picked` is left alone: a row picked at the root enters the extension
        // on its way to the menu, after the pick.
        Response::Replace(self.results(""))
    }

    fn search(&mut self, query: &str) -> Response {
        Response::Replace(self.results(query))
    }

    fn suggest(&mut self, query: &str) -> Vec<Item> {
        Color::parse(query).map(typed).into_iter().collect()
    }

    fn select(&mut self, id: &str) -> Response {
        if let Some(hex) = id.strip_prefix(TYPED).or_else(|| id.strip_prefix(COPIED)) {
            let Some(color) = Color::parse(hex) else {
                return Response::None;
            };
            self.picked = Some(color);
            return Response::Push(menu(color));
        }

        let Some(color) = self.picked else {
            return Response::Error("No color picked".into());
        };
        let text = match id {
            COPY_HEX => color.hex(),
            COPY_RGB => color.rgb(),
            COPY_HSL => color.hsl(),
            _ => return Response::None,
        };
        host::copy(&text);
        Response::Dismiss
    }

    fn dismissed(&mut self) {
        self.picked = None;
    }
}

export_extension!(Colors);
