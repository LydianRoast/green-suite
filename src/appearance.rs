// Copyright (c) 2026 Kevin R. Whiteman. All rights reserved.

//! Shared color and type roles for the native instrument workspace.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Id, Stroke, TextStyle, ThemePreference,
    Visuals, vec2,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scheme {
    #[default]
    Evergreen,
    Urbane,
}

impl Scheme {
    pub fn label(self) -> &'static str {
        match self {
            Self::Evergreen => "Evergreen",
            Self::Urbane => "Urbane",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Evergreen => "evergreen",
            Self::Urbane => "urbane",
        }
    }

    pub fn from_key(key: &str) -> Self {
        if key.trim().eq_ignore_ascii_case("urbane") {
            Self::Urbane
        } else {
            Self::Evergreen
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub green: Color32,
    pub ink: Color32,
    pub muted: Color32,
    pub line: Color32,
    pub paper: Color32,
    pub panel: Color32,
    pub action: Color32,
    pub selection: Color32,
    pub alternate: Color32,
    pub canvas: Color32,
    pub grid: Color32,
    /// Foreground for warnings and example-data labels, never a panel fill.
    pub warning: Color32,
    pub secondary: Color32,
    pub brand: Color32,
    pub heat_low: Color32,
    pub heat_high: Color32,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Palette {
    fn new(dark: bool, scheme: Scheme) -> Self {
        // Website tokens: https://evergreentelemetry.com/ (2026-10-22).
        // #00955A, #01613B, #51565A, #0F1728, #EDF0EF and white are site colors.
        // Urbane adds a muted complementary plum to the green, with warm paper
        // and walnut neutrals. Other shades adapt these roles for contrast.
        let brand = rgb(0x00955A);
        let action = rgb(0x01613B);
        match (scheme, dark) {
            (Scheme::Evergreen, false) => Self {
                green: action,
                ink: rgb(0x0F1728),
                muted: rgb(0x51565A),
                line: rgb(0x697E71),
                paper: Color32::WHITE,
                panel: rgb(0xEDF0EF),
                action,
                selection: rgb(0xCEE0D2),
                alternate: rgb(0xE1E9E4),
                canvas: rgb(0xE2E9E4),
                grid: rgb(0x91A99A),
                warning: rgb(0x774713),
                secondary: rgb(0x784A5D),
                brand,
                heat_low: rgb(0xE2ECDD),
                heat_high: rgb(0xA6CAB5),
            },
            (Scheme::Evergreen, true) => Self {
                green: rgb(0x82CEAD),
                ink: rgb(0xEDF0EF),
                muted: rgb(0xB1C0B8),
                line: rgb(0x74877A),
                paper: rgb(0x151D19),
                panel: rgb(0x1C2822),
                action,
                selection: rgb(0x304B3D),
                alternate: rgb(0x27382F),
                canvas: rgb(0x111C17),
                grid: rgb(0x758F7D),
                warning: rgb(0xFEC84B),
                secondary: rgb(0xD7B77A),
                brand,
                heat_low: rgb(0x243C2F),
                heat_high: rgb(0x426E55),
            },
            (Scheme::Urbane, false) => Self {
                green: action,
                ink: rgb(0x302B29),
                muted: rgb(0x635545),
                line: rgb(0x88745A),
                paper: rgb(0xF2E8D5),
                panel: rgb(0xE9DEC8),
                action,
                selection: rgb(0xDACAD2),
                alternate: rgb(0xE6D8C1),
                canvas: rgb(0xE0D4BF),
                grid: rgb(0x9F8F75),
                warning: rgb(0x774713),
                secondary: rgb(0x784A5D),
                brand,
                heat_low: rgb(0xDFE4D0),
                heat_high: rgb(0xA4B899),
            },
            (Scheme::Urbane, true) => Self {
                green: rgb(0x76C9A6),
                ink: rgb(0xF2E8D5),
                muted: rgb(0xC1B69F),
                line: rgb(0x978366),
                paper: rgb(0x262521),
                panel: rgb(0x32312B),
                action,
                selection: rgb(0x4C3941),
                alternate: rgb(0x3F3A31),
                canvas: rgb(0x211F1B),
                grid: rgb(0x8E805F),
                warning: rgb(0xD6B477),
                secondary: rgb(0xCBA3B3),
                brand,
                heat_low: rgb(0x363D31),
                heat_high: rgb(0x526040),
            },
        }
    }
}

pub fn palette(ctx: &egui::Context) -> Palette {
    let scheme = ctx.data(|data| {
        data.get_temp::<Scheme>(Id::new("green-suite-color-scheme"))
            .unwrap_or_default()
    });
    Palette::new(ctx.style().visuals.dark_mode, scheme)
}

pub fn apply(ctx: &egui::Context, dark: bool, scheme: Scheme) {
    ctx.set_theme(if dark {
        ThemePreference::Dark
    } else {
        ThemePreference::Light
    });
    ctx.data_mut(|data| data.insert_temp(Id::new("green-suite-color-scheme"), scheme));
    let colors = Palette::new(dark, scheme);
    let mut style = (*ctx.style()).clone();
    style.visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    style.visuals.override_text_color = Some(colors.ink);
    style.visuals.weak_text_color = Some(colors.muted);
    style.visuals.panel_fill = colors.panel;
    style.visuals.window_fill = colors.panel;
    style.visuals.extreme_bg_color = colors.paper;
    style.visuals.text_edit_bg_color = Some(colors.paper);
    style.visuals.code_bg_color = colors.alternate;
    style.visuals.faint_bg_color = colors.alternate;
    style.visuals.hyperlink_color = colors.green;
    style.visuals.warn_fg_color = colors.warning;
    style.visuals.error_fg_color = if dark { rgb(0xEDB6A6) } else { rgb(0x8D342E) };
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke = Stroke::new(1.0, colors.ink);
    style.visuals.window_corner_radius = CornerRadius::same(2);
    style.visuals.menu_corner_radius = CornerRadius::same(2);
    style.visuals.window_stroke = Stroke::new(1.0, colors.line);
    style.visuals.window_shadow = egui::epaint::Shadow::NONE;
    style.visuals.popup_shadow = egui::epaint::Shadow::NONE;
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.bg_fill = colors.alternate;
        widget.weak_bg_fill = colors.alternate;
        widget.bg_stroke = Stroke::new(1.0, colors.line);
        widget.fg_stroke = Stroke::new(1.4, colors.ink);
        widget.corner_radius = CornerRadius::same(2);
        widget.expansion = 0.0;
    }
    for widget in [
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.bg_fill = colors.selection;
        widget.weak_bg_fill = colors.selection;
        widget.bg_stroke = Stroke::new(1.0, colors.secondary);
    }
    style.visuals.widgets.noninteractive.bg_fill = colors.panel;
    style.visuals.widgets.noninteractive.weak_bg_fill = colors.panel;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.line);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.muted);
    style.visuals.widgets.noninteractive.corner_radius = CornerRadius::same(2);
    style.spacing.item_spacing = vec2(8.0, 8.0);
    style.spacing.button_padding = vec2(11.0, 7.0);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    let bold = |size| FontId::new(size, FontFamily::Name("Serif Bold".into()));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(14.0));
    style.text_styles.insert(TextStyle::Button, bold(13.0));
    style.text_styles.insert(TextStyle::Heading, bold(24.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::proportional(12.0));
    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(color: Color32) -> f64 {
        let linear = |channel: u8| {
            let value = channel as f64 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(color.r()) + 0.7152 * linear(color.g()) + 0.0722 * linear(color.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn color_scheme_keys_are_stable_and_unknown_keys_use_evergreen() {
        for scheme in [Scheme::Evergreen, Scheme::Urbane] {
            assert_eq!(Scheme::from_key(scheme.key()), scheme);
        }
        assert_eq!(Scheme::from_key(" Urbane\n"), Scheme::Urbane);
        assert_eq!(Scheme::from_key("unknown"), Scheme::Evergreen);
    }

    #[test]
    fn text_and_measurement_roles_keep_contrast_in_every_appearance() {
        for scheme in [Scheme::Evergreen, Scheme::Urbane] {
            for dark in [false, true] {
                let p = Palette::new(dark, scheme);
                for text in [p.ink, p.muted, p.green, p.secondary, p.warning] {
                    for surface in [p.paper, p.panel, p.selection, p.alternate] {
                        assert!(
                            contrast(text, surface) >= 4.5,
                            "{scheme:?}, dark={dark}: {text:?} on {surface:?}"
                        );
                    }
                }
                assert!(contrast(Color32::WHITE, p.action) >= 4.5);
                assert!(contrast(p.paper, p.green) >= 4.5);
                assert!(contrast(p.line, p.alternate) >= 3.0);
                for heat in [p.heat_low, p.heat_high] {
                    assert!(contrast(p.ink, heat) >= 4.5);
                    assert!(contrast(p.green, heat) >= 3.0);
                    assert!(contrast(p.secondary, heat) >= 3.0);
                }
            }
        }
    }
}
