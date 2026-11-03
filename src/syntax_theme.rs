//! Built-in syntax-highlight palettes for the raw JSON panel.
//!
//! Each named theme ships both a light and a dark variant, and the user picks
//! which theme to use per app mode (the mode itself follows the system).

use gpui_kit::Rgba;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyntaxColors {
    /// Property/key quotes.
    pub key: Rgba,
    /// String values (with quotes).
    pub string: Rgba,
    pub number: Rgba,
    pub bool_kw: Rgba,
    pub null: Rgba,
    /// Braces, brackets, colons, commas.
    pub punct: Rgba,
}

pub struct SyntaxTheme {
    pub name: &'static str,
    pub dark: SyntaxColors,
    pub light: SyntaxColors,
}

/// Const version of `gpui_kit::rgb` so the theme table can be a `const`.
const fn rgb(hex: u32) -> Rgba {
    let [_, r, g, b] = hex.to_be_bytes();
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a: 1.0,
    }
}

const fn colors(
    key: u32,
    string: u32,
    number: u32,
    bool_kw: u32,
    null: u32,
    punct: u32,
) -> SyntaxColors {
    SyntaxColors {
        key: rgb(key),
        string: rgb(string),
        number: rgb(number),
        bool_kw: rgb(bool_kw),
        null: rgb(null),
        punct: rgb(punct),
    }
}

pub const THEMES: &[SyntaxTheme] = &[
    SyntaxTheme {
        name: "Tokyo Night",
        dark: colors(0xBB9AF7, 0x9ECE6A, 0xFF9E64, 0x7AA2F7, 0x7DCFFF, 0x89DDFF),
        light: colors(0x815AB4, 0x41A358, 0xD96E13, 0x3760BF, 0x6C7396, 0x99A7BD),
    },
    SyntaxTheme {
        name: "GitHub",
        dark: colors(0x79C0FF, 0xA5D6FF, 0x79C0FF, 0xFFA657, 0x8B949E, 0xC9D1D9),
        light: colors(0x0550AE, 0x116329, 0x0969DA, 0x8250DF, 0x59636E, 0x57606A),
    },
    SyntaxTheme {
        name: "Catppuccin",
        // Mocha
        dark: colors(0xF5C2E7, 0xA6E3A1, 0xFAB387, 0xCBA6F7, 0x7F849C, 0x89DCEB),
        // Latte
        light: colors(0x8839EF, 0x40A02B, 0xFE640B, 0x04A5E5, 0x7C7F93, 0x5C5F77),
    },
    SyntaxTheme {
        name: "One",
        // Atom One Dark
        dark: colors(0xE06C75, 0x98C379, 0xD19A66, 0xC678DD, 0xD19A66, 0x828991),
        // Atom One Light
        light: colors(0xE45649, 0x50A14F, 0x986801, 0xA626A4, 0x9DA3AC, 0x4F5560),
    },
    SyntaxTheme {
        name: "Gruvbox",
        dark: colors(0x83A598, 0xB8BB26, 0xFE8019, 0xD3869B, 0x928374, 0xA89984),
        light: colors(0x076678, 0x79740E, 0xAF3A03, 0x8F3F71, 0x928374, 0x665C54),
    },
    SyntaxTheme {
        name: "Solarized",
        dark: colors(0x268BD2, 0x2AA198, 0xB58900, 0xCB4B16, 0x93A1A1, 0x839496),
        light: colors(0x268BD2, 0x2AA198, 0xB58900, 0xCB4B16, 0x93A1A1, 0x586E75),
    },
];

/// All selectable theme names, in catalog order.
pub fn theme_names() -> Vec<&'static str> {
    THEMES.iter().map(|t| t.name).collect()
}

/// Resolve a theme by name (case-insensitive); unknown names fall back to
/// "Tokyo Night".
pub fn find(name: &str) -> Option<&'static SyntaxTheme> {
    THEMES.iter().find(|t| t.name.eq_ignore_ascii_case(name))
}

pub fn fallback() -> &'static SyntaxTheme {
    &THEMES[0]
}
