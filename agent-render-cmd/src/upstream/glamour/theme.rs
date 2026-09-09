//! Selected values ported from styles/dark.json, commit
//! cf874d7039af3485a38afa7d2e8e87ee42a7bbaa (MIT).
//! Adaptation: typed constants instead of a JSON stylesheet; document foreground
//! follows the terminal. Chroma syntax themes and heading source prefixes omitted.

use crate::ansi::{Color, Style};

pub(crate) struct Theme;

impl Theme {
    pub const DOCUMENT_MARGIN: usize = 2;
    pub const LIST_INDENT: usize = 2;
    pub const QUOTE_PREFIX: &'static str = "│ ";
    pub const ITEM_PREFIX: &'static str = "• ";
    pub const HEADING: Style = Style {
        bold: true,
        fg: Some(Color::Indexed(39)),
        ..Style::PLAIN
    };
    pub const H1: Style = Style {
        bold: true,
        fg: Some(Color::Indexed(228)),
        bg: Some(Color::Indexed(63)),
        ..Style::PLAIN
    };
    pub const CODE: Style = Style {
        fg: Some(Color::Indexed(203)),
        bg: Some(Color::Indexed(236)),
        ..Style::PLAIN
    };
    pub const CODE_BLOCK: Style = Style {
        fg: Some(Color::Indexed(244)),
        ..Style::PLAIN
    };
    pub const LINK: Style = Style {
        fg: Some(Color::Indexed(30)),
        underline: true,
        ..Style::PLAIN
    };
    pub const LINK_TEXT: Style = Style {
        fg: Some(Color::Indexed(35)),
        bold: true,
        ..Style::PLAIN
    };
}
