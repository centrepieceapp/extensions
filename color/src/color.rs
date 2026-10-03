//! Colors as CSS writes them: `#rgb` and its longer forms, `rgb()`, `rgba()`,
//! `hsl()` and `hsla()`, read and written back out.

/// One color, in the channels a swatch is drawn with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
}

impl Color {
    /// Reads a whole text as a color: a hex color, or one of the CSS color
    /// functions with either the comma or the space syntax. Anything with
    /// more around it is not one — `#12` is someone still typing, and
    /// `#12345` would have to be guessed at.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(digits) = text.strip_prefix('#') {
            return Self::parse_hex(digits);
        }
        let (name, args) = text.strip_suffix(')')?.split_once('(')?;
        let args = arguments(args)?;
        match name.trim().to_ascii_lowercase().as_str() {
            "rgb" | "rgba" => Self::from_rgb_args(&args),
            "hsl" | "hsla" => Self::from_hsl_args(&args),
            _ => None,
        }
    }

    fn parse_hex(digits: &str) -> Option<Self> {
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let value = u32::from_str_radix(digits, 16).ok()?;
        let [a, b, c, d] = value.to_be_bytes();
        // The short forms double each digit: `#f80` is `#ff8800`.
        let nibble = |shift: u32| ((value >> shift) & 0xf) as u8 * 0x11;

        match digits.len() {
            3 => Some(Self::new(nibble(8), nibble(4), nibble(0), 0xff)),
            4 => Some(Self::new(nibble(12), nibble(8), nibble(4), nibble(0))),
            6 => Some(Self::new(b, c, d, 0xff)),
            8 => Some(Self::new(a, b, c, d)),
            _ => None,
        }
    }

    fn from_rgb_args(args: &[&str]) -> Option<Self> {
        let [red, green, blue, alpha @ ..] = args else {
            return None;
        };
        let channel = |text: &str| -> Option<u8> {
            let value = match text.strip_suffix('%') {
                Some(percent) => number(percent)? * 2.55,
                None => number(text)?,
            };
            Some(value.clamp(0.0, 255.0).round() as u8)
        };
        Some(Self::new(
            channel(red)?,
            channel(green)?,
            channel(blue)?,
            self::alpha(alpha)?,
        ))
    }

    fn from_hsl_args(args: &[&str]) -> Option<Self> {
        let [hue, saturation, lightness, alpha @ ..] = args else {
            return None;
        };
        let hue = number(hue.strip_suffix("deg").unwrap_or(hue))?;
        // CSS wants the percent signs, but a bare number means the same.
        let fraction = |text: &str| -> Option<f64> {
            Some((number(text.strip_suffix('%').unwrap_or(text))? / 100.0).clamp(0.0, 1.0))
        };
        let (red, green, blue) = hsl_to_rgb(hue, fraction(saturation)?, fraction(lightness)?);
        Some(Self::new(red, green, blue, self::alpha(alpha)?))
    }

    const fn new(red: u8, green: u8, blue: u8, alpha: u8) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }

    pub fn is_opaque(self) -> bool {
        self.alpha == 0xff
    }

    /// `0xRRGGBBAA`, which is what a swatch icon takes.
    pub fn rgba(self) -> u32 {
        u32::from_be_bytes([self.red, self.green, self.blue, self.alpha])
    }

    /// The long form, with the alpha digits only when they say something.
    pub fn hex(self) -> String {
        let Self {
            red,
            green,
            blue,
            alpha,
        } = self;
        if self.is_opaque() {
            format!("#{red:02x}{green:02x}{blue:02x}")
        } else {
            format!("#{red:02x}{green:02x}{blue:02x}{alpha:02x}")
        }
    }

    /// `rgb()`, or `rgba()` when the color lets something through.
    pub fn rgb(self) -> String {
        let Self {
            red, green, blue, ..
        } = self;
        if self.is_opaque() {
            format!("rgb({red}, {green}, {blue})")
        } else {
            format!("rgba({red}, {green}, {blue}, {})", self.alpha_text())
        }
    }

    /// `hsl()`, or `hsla()` when the color lets something through, rounded to
    /// whole degrees and percents.
    pub fn hsl(self) -> String {
        let (hue, saturation, lightness) = rgb_to_hsl(self.red, self.green, self.blue);
        let (hue, saturation, lightness) = (
            hue.round() as u32 % 360,
            (saturation * 100.0).round() as u32,
            (lightness * 100.0).round() as u32,
        );
        if self.is_opaque() {
            format!("hsl({hue}, {saturation}%, {lightness}%)")
        } else {
            format!(
                "hsla({hue}, {saturation}%, {lightness}%, {})",
                self.alpha_text()
            )
        }
    }

    /// The alpha as CSS writes it, to two places without trailing zeros:
    /// every such value survives the trip through a byte and back.
    fn alpha_text(self) -> String {
        let text = format!("{:.2}", f64::from(self.alpha) / 255.0);
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// A color function's arguments: `1, 2, 3` or `1 2 3 / 0.5`, three or four of
/// them either way.
fn arguments(text: &str) -> Option<Vec<&str>> {
    let args: Vec<&str> = if text.contains(',') {
        text.split(',').map(str::trim).collect()
    } else {
        let (channels, alpha) = match text.split_once('/') {
            Some((channels, alpha)) => (channels, Some(alpha.trim())),
            None => (text, None),
        };
        let mut args: Vec<&str> = channels.split_whitespace().collect();
        if args.len() != 3 {
            return None;
        }
        args.extend(alpha);
        args
    };
    (matches!(args.len(), 3 | 4) && args.iter().all(|arg| !arg.is_empty())).then_some(args)
}

/// The optional fourth argument: a fraction or a percentage, opaque when
/// left out.
fn alpha(rest: &[&str]) -> Option<u8> {
    let value = match rest {
        [] => return Some(0xff),
        [text] => match text.strip_suffix('%') {
            Some(percent) => number(percent)? / 100.0,
            None => number(text)?,
        },
        _ => return None,
    };
    Some((value.clamp(0.0, 1.0) * 255.0).round() as u8)
}

/// A plain decimal number. `f64::from_str` alone would also take `inf` and
/// `NaN`, which no color is made of.
fn number(text: &str) -> Option<f64> {
    let text = text.trim();
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'));
    plain.then(|| text.parse().ok()).flatten()
}

/// `hue` in degrees, the others from 0 to 1.
fn hsl_to_rgb(hue: f64, saturation: f64, lightness: f64) -> (u8, u8, u8) {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let hue = hue.rem_euclid(360.0) / 60.0;
    let second = chroma * (1.0 - (hue % 2.0 - 1.0).abs());
    let (red, green, blue) = match hue as u32 {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let lift = lightness - chroma / 2.0;
    let byte = |value: f64| ((value + lift) * 255.0).clamp(0.0, 255.0).round() as u8;
    (byte(red), byte(green), byte(blue))
}

/// Hue in degrees, saturation and lightness from 0 to 1.
fn rgb_to_hsl(red: u8, green: u8, blue: u8) -> (f64, f64, f64) {
    let [red, green, blue] = [red, green, blue].map(|byte| f64::from(byte) / 255.0);
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta == 0.0 {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == red {
        ((green - blue) / delta).rem_euclid(6.0)
    } else if max == green {
        (blue - red) / delta + 2.0
    } else {
        (red - green) / delta + 4.0
    };
    (hue * 60.0, saturation, lightness)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORANGE: Color = Color::new(0xff, 0x88, 0x00, 0xff);

    #[test]
    fn reads_every_hex_length_css_allows() {
        assert_eq!(Color::parse("#f80"), Some(ORANGE));
        assert_eq!(
            Color::parse("#f808"),
            Some(Color::new(0xff, 0x88, 0x00, 0x88))
        );
        assert_eq!(Color::parse("#FF8800"), Some(ORANGE));
        assert_eq!(
            Color::parse(" #ff880080 "),
            Some(Color::new(0xff, 0x88, 0x00, 0x80))
        );
    }

    #[test]
    fn reads_rgb_both_ways_css_writes_it() {
        assert_eq!(Color::parse("rgb(255, 136, 0)"), Some(ORANGE));
        assert_eq!(Color::parse("RGB(255 136 0)"), Some(ORANGE));
        assert_eq!(Color::parse("rgb(100%, 53.3%, 0%)"), Some(ORANGE));
        let clear = Some(Color::new(0xff, 0x88, 0x00, 0x80));
        assert_eq!(Color::parse("rgba(255, 136, 0, 0.5)"), clear);
        assert_eq!(Color::parse("rgb(255 136 0 / 50%)"), clear);
        assert_eq!(Color::parse("rgba(255,136,0,.5)"), clear);
    }

    #[test]
    fn reads_hsl_both_ways_css_writes_it() {
        assert_eq!(Color::parse("hsl(32, 100%, 50%)"), Some(ORANGE));
        assert_eq!(Color::parse("hsl(32deg 100% 50%)"), Some(ORANGE));
        assert_eq!(
            Color::parse("hsla(0, 0%, 100%, 0.5)"),
            Some(Color::new(0xff, 0xff, 0xff, 0x80))
        );
        assert_eq!(
            Color::parse("hsl(240 100% 50% / 25%)"),
            Some(Color::new(0x00, 0x00, 0xff, 0x40))
        );
        assert_eq!(
            Color::parse("hsl(-120, 100%, 50%)"),
            Some(Color::new(0x00, 0x00, 0xff, 0xff))
        );
    }

    #[test]
    fn leaves_everything_else_alone() {
        for text in ["", "#", "#f", "#ff", "#ff880", "#ff88000", "#ff8800800"] {
            assert_eq!(Color::parse(text), None, "{text:?}");
        }
        for text in ["ff8800", "#ggg", "#+ff", "# f80", "#f80 x", "#ｆ８０"] {
            assert_eq!(Color::parse(text), None, "{text:?}");
        }
        for text in [
            "rgb(",
            "rgb()",
            "rgb(1, 2)",
            "rgb(1, 2, 3, 4, 5)",
            "rgb(1 2 3 4)",
            "rgb(1, 2, 3",
            "rgb(1, 2, 3) x",
            "rgb(1, , 3)",
            "rgb(inf, 2, 3)",
            "rgb(red, 2, 3)",
            "hsl(1, 2%)",
            "cmyk(1, 2, 3, 4)",
            "the rgb(1, 2, 3)",
        ] {
            assert_eq!(Color::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn writes_itself_back_out() {
        assert_eq!(ORANGE.hex(), "#ff8800");
        assert_eq!(ORANGE.rgb(), "rgb(255, 136, 0)");
        assert_eq!(ORANGE.hsl(), "hsl(32, 100%, 50%)");
        assert_eq!(ORANGE.rgba(), 0xff8800ff);

        let clear = Color::parse("#ff880080").unwrap();
        assert_eq!(clear.hex(), "#ff880080");
        assert_eq!(clear.rgb(), "rgba(255, 136, 0, 0.5)");
        assert_eq!(clear.hsl(), "hsla(32, 100%, 50%, 0.5)");

        let gone = Color::parse("rgba(0, 0, 0, 0)").unwrap();
        assert_eq!(gone.rgb(), "rgba(0, 0, 0, 0)");
        assert_eq!(Color::parse("#fff").unwrap().hsl(), "hsl(0, 0%, 100%)");
    }

    #[test]
    fn every_two_place_alpha_survives_the_round_trip() {
        for hundredths in 0..100 {
            let text = format!("rgba(1, 2, 3, {})", f64::from(hundredths) / 100.0);
            let color = Color::parse(&text).unwrap();
            assert_eq!(Color::parse(&color.rgb()), Some(color), "{text}");
        }
    }
}
