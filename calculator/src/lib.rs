//! Arithmetic suggestions at the root and under the `calc` prefix.

mod calc;

use calc::Calculation;
use centrepiece_extension::{Extension, Item, Response, Screen, export_extension, host};

struct Calculator;

fn preview(query: &str) -> Option<Item> {
    let answer = Calculation::parse(query)?.display();
    Some(
        Item::new(query.trim(), answer)
            .subtitle(query.trim())
            .detail("Copy")
            .glyph('='),
    )
}

impl Extension for Calculator {
    fn new() -> Self {
        Self
    }

    fn activate(&mut self) -> Response {
        self.search("")
    }

    fn search(&mut self, query: &str) -> Response {
        Response::Replace(
            Screen::search(self.suggest(query))
                .placeholder("Type arithmetic: 2 * (3 + 4)")
                .status(if query.trim().is_empty() {
                    "Type arithmetic to calculate an answer"
                } else {
                    "Not a complete arithmetic expression"
                }),
        )
    }

    fn suggest(&mut self, query: &str) -> Vec<Item> {
        preview(query).into_iter().collect()
    }

    fn select(&mut self, id: &str) -> Response {
        let Some(calculation) = Calculation::parse(id) else {
            return Response::None;
        };
        host::copy(&calculation.display());
        Response::Dismiss
    }
}

export_extension!(Calculator);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_preserve_the_expression_and_formatted_answer() {
        let mut calculator = Calculator::new();
        let items = calculator.suggest(" 2 * (3 + 4) ");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "2 * (3 + 4)");
        assert_eq!(items[0].title, "14");
        assert_eq!(items[0].subtitle.as_deref(), Some("2 * (3 + 4)"));
        assert_eq!(items[0].detail.as_deref(), Some("Copy"));
        assert!(calculator.suggest("google-chrome").is_empty());
        assert!(calculator.suggest("2 +").is_empty());
        assert!(matches!(calculator.select("invalid"), Response::None));
    }

    #[test]
    fn prefix_search_uses_the_same_preview() {
        let Response::Replace(screen) = Calculator::new().search("0.1 + 0.2") else {
            panic!("expected a search screen");
        };
        assert_eq!(screen.items.len(), 1);
        assert_eq!(screen.items[0].title, "0.3");
    }
}
