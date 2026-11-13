use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// An opaque sRGB colour, written in JSON as `#rrggbb`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0 };
    pub const WHITE: Color = Color { r: 255, g: 255, b: 255 };

    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// `#rgb` or `#rrggbb`, case-insensitive.
    pub fn parse(s: &str) -> Option<Color> {
        let h = s.strip_prefix('#')?;
        if !h.is_ascii() {
            return None;
        }
        let n = |i: usize, len: usize| u8::from_str_radix(h.get(i..i + len)?, 16).ok();
        match h.len() {
            3 => Some(Color::rgb(n(0, 1)? * 17, n(1, 1)? * 17, n(2, 1)? * 17)),
            6 => Some(Color::rgb(n(0, 2)?, n(2, 2)?, n(4, 2)?)),
            _ => None,
        }
    }

    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl Serialize for Color {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Color::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("not a #rrggbb colour: {s}")))
    }
}
