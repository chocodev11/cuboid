//! Typography roles of the UI v2 specification (section 2). The roles are the only way to get a
//! font: code outside the kit never builds a `FontId` or sets a size.

use egui::{Color32, FontFamily, FontId, RichText};

use crate::fonts::{MEDIUM, SEMIBOLD};
use crate::metrics::LINE_HEIGHT;
use crate::theme::Tokens;

/// A text role: size, weight and colour together.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Type {
    /// 11 regular, weak: metadata, hints, badges, shortcut chips.
    Caption,
    /// 12 regular: default UI text, values, list rows.
    Body,
    /// 12 regular, weak: property labels.
    Label,
    /// 12 medium, strong: emphasised values, selected row titles.
    Strong,
    /// 13 semibold, strong: panel headers, section titles, card titles.
    Title,
    /// 16 semibold, strong: dialog titles, empty-state headlines.
    Heading,
    /// 20 semibold, strong: the start screen headline only.
    Display,
}

/// Font weight of a role; each weight is a family of its own.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Weight {
    /// 400.
    Regular,
    /// 500.
    Medium,
    /// 600.
    SemiBold,
}

/// Which colour token a role is drawn with.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum TextColor {
    /// `text_weak`.
    Weak,
    /// `text`.
    Normal,
    /// `text_strong`.
    Strong,
}

impl Type {
    /// Every role, smallest first.
    pub const ALL: [Type; 7] = [
        Type::Caption,
        Type::Body,
        Type::Label,
        Type::Strong,
        Type::Title,
        Type::Heading,
        Type::Display,
    ];

    /// Size in points.
    pub const fn size(self) -> f32 {
        match self {
            Type::Caption => 11.0,
            Type::Body | Type::Label | Type::Strong => 12.0,
            Type::Title => 13.0,
            Type::Heading => 16.0,
            Type::Display => 20.0,
        }
    }

    /// Weight of the font.
    pub const fn weight(self) -> Weight {
        match self {
            Type::Caption | Type::Body | Type::Label => Weight::Regular,
            Type::Strong => Weight::Medium,
            Type::Title | Type::Heading | Type::Display => Weight::SemiBold,
        }
    }

    /// The colour token of the role.
    pub const fn color_role(self) -> TextColor {
        match self {
            Type::Caption | Type::Label => TextColor::Weak,
            Type::Body => TextColor::Normal,
            Type::Strong | Type::Title | Type::Heading | Type::Display => TextColor::Strong,
        }
    }

    /// Line height in points, 1.35 times the size.
    pub fn line_height(self) -> f32 {
        self.size() * LINE_HEIGHT
    }

    /// The font of the role.
    pub fn font(self) -> FontId {
        let family = match self.weight() {
            Weight::Regular => FontFamily::Proportional,
            Weight::Medium => MEDIUM.clone(),
            Weight::SemiBold => SEMIBOLD.clone(),
        };
        FontId::new(self.size(), family)
    }

    /// The colour of the role in a theme.
    pub fn color(self, tokens: &Tokens) -> Color32 {
        match self.color_role() {
            TextColor::Weak => tokens.text_weak,
            TextColor::Normal => tokens.text,
            TextColor::Strong => tokens.text_strong,
        }
    }
}

/// Text in a role. The colour follows the role through the style the theme installs (weak,
/// normal or strong text), so a label reads correctly on any surface; call [`Type::color`] for
/// painter calls.
pub fn text(role: Type, text: impl Into<String>) -> RichText {
    let rich = RichText::new(text)
        .font(role.font())
        .line_height(Some(role.line_height()));
    match role.color_role() {
        TextColor::Weak => rich.weak(),
        TextColor::Normal => rich,
        TextColor::Strong => rich.strong(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;

    #[test]
    fn roles_match_the_specification_table() {
        let table = [
            (Type::Caption, 11.0, Weight::Regular, TextColor::Weak),
            (Type::Body, 12.0, Weight::Regular, TextColor::Normal),
            (Type::Label, 12.0, Weight::Regular, TextColor::Weak),
            (Type::Strong, 12.0, Weight::Medium, TextColor::Strong),
            (Type::Title, 13.0, Weight::SemiBold, TextColor::Strong),
            (Type::Heading, 16.0, Weight::SemiBold, TextColor::Strong),
            (Type::Display, 20.0, Weight::SemiBold, TextColor::Strong),
        ];
        assert_eq!(table.len(), Type::ALL.len());
        for (role, size, weight, color) in table {
            assert!((role.size() - size).abs() < f32::EPSILON, "{role:?}");
            assert_eq!(role.weight(), weight, "{role:?}");
            assert_eq!(role.color_role(), color, "{role:?}");
        }
    }

    #[test]
    fn line_height_is_one_and_a_third_of_the_size() {
        for role in Type::ALL {
            assert!(
                (role.line_height() / role.size() - 1.35).abs() < 1e-5,
                "{role:?}"
            );
        }
    }

    #[test]
    fn each_weight_has_its_own_family() {
        assert_eq!(Type::Body.font().family, FontFamily::Proportional);
        assert_eq!(Type::Strong.font().family, *MEDIUM);
        assert_eq!(Type::Title.font().family, *SEMIBOLD);
        assert_ne!(Type::Strong.font().family, Type::Title.font().family);
        assert_eq!(Type::Heading.font().family, Type::Display.font().family);
    }

    #[test]
    fn role_colours_come_from_the_tokens() {
        let t = Theme::default().tokens(true);
        assert_eq!(Type::Caption.color(&t), t.text_weak);
        assert_eq!(Type::Body.color(&t), t.text);
        assert_eq!(Type::Title.color(&t), t.text_strong);
    }
}
