//! Arithmetic typed into the query, so Centrepiece can answer it.

/// A query that reads as a sum: numbers joined by `+ - * / ^`, with
/// parentheses and signs where they are wanted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Calculation {
    pub value: f64,
}

impl Calculation {
    /// Reads a whole query as arithmetic. It has to be all arithmetic — `2 +`
    /// is someone still typing, and `google-chrome` is an app — and it has to
    /// use at least one operator, or `42` and `-3` would get a row that only
    /// repeats them. Answers that are not a number, such as `1/0`, are none.
    pub fn parse(text: &str) -> Option<Self> {
        // The parser recurses once per bracket and sign, so a pasted wall of
        // `(((` must not get to it. Nobody types a sum this long.
        if text.len() > 256 {
            return None;
        }
        let mut parser = Parser {
            chars: text.chars().collect(),
            at: 0,
            operators: 0,
        };
        let value = parser.sum()?;
        parser.skip_space();
        let whole = parser.at == parser.chars.len();
        (whole && parser.operators > 0 && value.is_finite()).then_some(Self { value })
    }

    /// The answer as one would write it: no trailing `.0`, no floating-point
    /// noise (`0.1+0.2` is `0.3`), and an exponent only for numbers too large
    /// or too small to read otherwise.
    pub fn display(self) -> String {
        // Twelve significant digits is well inside what an f64 carries, so
        // rounding there drops the noise without dropping anything real.
        let value: f64 = format!("{:.11e}", self.value).parse().unwrap_or(self.value);
        let magnitude = value.abs();
        if magnitude != 0.0 && !(1e-6..1e15).contains(&magnitude) {
            format!("{value:e}")
        } else if value == 0.0 {
            // `-0` is correct and unhelpful.
            "0".to_string()
        } else {
            value.to_string()
        }
    }
}

/// Recursive descent, one level per precedence:
///
/// ```text
/// sum     = product (("+" | "-") product)*
/// product = signed (("*" | "/") signed)*
/// signed  = ("+" | "-") signed | power
/// power   = atom ("^" signed)?
/// atom    = number | "(" sum ")"
/// ```
///
/// `^` binds tighter than a leading sign and to the right, so `-2^2` is `-4`
/// and `2^3^2` is `512`, as on paper.
struct Parser {
    chars: Vec<char>,
    at: usize,
    /// Binary operators seen, which is what makes a query arithmetic rather
    /// than a number.
    operators: usize,
}

impl Parser {
    fn sum(&mut self) -> Option<f64> {
        let mut value = self.product()?;
        loop {
            if self.eat('+') {
                value += self.product()?;
            } else if self.eat('-') {
                value -= self.product()?;
            } else {
                return Some(value);
            }
            self.operators += 1;
        }
    }

    fn product(&mut self) -> Option<f64> {
        let mut value = self.signed()?;
        loop {
            if self.eat('*') {
                value *= self.signed()?;
            } else if self.eat('/') {
                value /= self.signed()?;
            } else {
                return Some(value);
            }
            self.operators += 1;
        }
    }

    fn signed(&mut self) -> Option<f64> {
        if self.eat('-') {
            return Some(-self.signed()?);
        }
        if self.eat('+') {
            return self.signed();
        }
        self.power()
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        // `**` is how half the languages people use spell power.
        if self.eat('^') || self.eat_str("**") {
            self.operators += 1;
            return Some(base.powf(self.signed()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat('(') {
            let value = self.sum()?;
            return self.eat(')').then_some(value);
        }
        self.number()
    }

    fn number(&mut self) -> Option<f64> {
        self.skip_space();
        let start = self.at;
        while self
            .chars
            .get(self.at)
            .is_some_and(|c| c.is_ascii_digit() || *c == '.')
        {
            self.at += 1;
        }
        let digits: String = self.chars[start..self.at].iter().collect();
        // `str::parse` takes `inf` and `1e5` too; only plain decimals get here,
        // and it still turns away `.` and `1.2.3`.
        digits.parse().ok()
    }

    fn skip_space(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn eat_str(&mut self, text: &str) -> bool {
        self.skip_space();
        let end = self.at + text.chars().count();
        let found = self
            .chars
            .get(self.at..end)
            .is_some_and(|slice| slice.iter().copied().eq(text.chars()));
        if found {
            self.at = end;
        }
        found
    }

    fn eat(&mut self, expected: char) -> bool {
        self.skip_space();
        let found = self.chars.get(self.at) == Some(&expected);
        if found {
            self.at += 1;
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(text: &str) -> Option<String> {
        Calculation::parse(text).map(Calculation::display)
    }

    #[test]
    fn does_the_arithmetic() {
        for (text, expected) in [
            ("1+2", "3"),
            ("6 * 7", "42"),
            ("10 / 4", "2.5"),
            ("2^10", "1024"),
            ("2**10", "1024"),
            ("10 - 2 - 3", "5"),
            ("2 + 3 * 4", "14"),
            ("(2 + 3) * 4", "20"),
            ("-2^2", "-4"),
            ("2^-1", "0.5"),
            ("2^3^2", "512"),
            ("-(1 + 2) * -3", "9"),
            (".5 + .25", "0.75"),
            (" 3 - 3 ", "0"),
        ] {
            assert_eq!(answer(text).as_deref(), Some(expected), "{text:?}");
        }
    }

    #[test]
    fn hides_floating_point_noise() {
        assert_eq!(answer("0.1 + 0.2").as_deref(), Some("0.3"));
        assert_eq!(answer("1 / 3").as_deref(), Some("0.333333333333"));
        assert_eq!(answer("10^20").as_deref(), Some("1e20"));
        assert_eq!(answer("1 / 10^9").as_deref(), Some("1e-9"));
    }

    #[test]
    fn leaves_everything_else_alone() {
        for text in ["", "42", "-3", "(5)", "2 +", "* 2", "2 3 + 1"] {
            assert_eq!(Calculation::parse(text), None, "{text:?}");
        }
        for text in ["(1 + 2", "1 + 2)", "1.2.3 + 1", "inf + 1", "1e5 + 1"] {
            assert_eq!(Calculation::parse(text), None, "{text:?}");
        }
        for text in ["google-chrome", "a+b", "#f80", "1/0", "0/0"] {
            assert_eq!(Calculation::parse(text), None, "{text:?}");
        }
    }
}
