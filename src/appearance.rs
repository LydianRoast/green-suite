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
        // The emblem and Evergreen actions retain observed website greens.
        // Surfaces are neutral black, white and gray. Saturated foreground
        // accents carry the scheme without tinted paper or pastel chart fills.
        let brand = rgb(0x00955A);
        let action = match scheme {
            Scheme::Evergreen => rgb(0x01613B),
            Scheme::Urbane => rgb(0x4B007D),
        };
        let accent = match (scheme, dark) {
            (Scheme::Evergreen, false) => rgb(0x004127),
            (Scheme::Evergreen, true) => rgb(0x00EF83),
            (Scheme::Urbane, false) => rgb(0x4B007D),
            (Scheme::Urbane, true) => rgb(0xFF00FF),
        };
        match dark {
            false => Self {
                green: accent,
                ink: Color32::BLACK,
                muted: rgb(0x303030),
                line: Color32::BLACK,
                paper: Color32::WHITE,
                panel: Color32::WHITE,
                action,
                selection: rgb(0xE0E0E0),
                alternate: rgb(0xF2F2F2),
                canvas: Color32::WHITE,
                grid: rgb(0x505050),
                warning: rgb(0x700000),
                secondary: match scheme {
                    Scheme::Evergreen => rgb(0x4B007D),
                    Scheme::Urbane => rgb(0x004127),
                },
                brand,
                heat_low: Color32::WHITE,
                heat_high: rgb(0xC6C6C6),
            },
            true => Self {
                green: accent,
                ink: Color32::WHITE,
                muted: rgb(0xDDDDDD),
                line: Color32::WHITE,
                paper: Color32::BLACK,
                panel: rgb(0x050505),
                action,
                selection: rgb(0x1A1A1A),
                alternate: rgb(0x101010),
                canvas: Color32::BLACK,
                grid: rgb(0x777777),
                warning: rgb(0xFFCC00),
                secondary: rgb(0xFFCC00),
                brand,
                heat_low: rgb(0x080808),
                heat_high: rgb(0x343434),
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
    style.visuals.error_fg_color = if dark { rgb(0xFF4444) } else { rgb(0x8B0000) };
    style.visuals.selection.bg_fill = colors.selection;
    style.visuals.selection.stroke = Stroke::new(2.0, colors.green);
    style.visuals.window_corner_radius = CornerRadius::ZERO;
    style.visuals.menu_corner_radius = CornerRadius::ZERO;
    style.visuals.window_stroke = Stroke::new(1.5, colors.line);
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
        widget.corner_radius = CornerRadius::ZERO;
        widget.expansion = 0.0;
    }
    for widget in [
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.bg_fill = colors.selection;
        widget.weak_bg_fill = colors.selection;
        widget.bg_stroke = Stroke::new(2.0, colors.green);
    }
    style.visuals.widgets.noninteractive.bg_fill = colors.panel;
    style.visuals.widgets.noninteractive.weak_bg_fill = colors.panel;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, colors.line);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, colors.muted);
    style.visuals.widgets.noninteractive.corner_radius = CornerRadius::ZERO;
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(12.0, 6.0);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    let bold = |size| FontId::new(size, FontFamily::Name("Serif Bold".into()));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::monospace(14.0));
    style.text_styles.insert(TextStyle::Button, bold(14.0));
    style.text_styles.insert(TextStyle::Heading, bold(22.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::monospace(12.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(13.0));
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
                for surface in [p.paper, p.panel, p.selection, p.alternate] {
                    assert!(contrast(p.ink, surface) >= 12.0);
                    assert!(contrast(p.muted, surface) >= 7.0);
                }
                for text in [p.ink, p.muted, p.green, p.secondary, p.warning] {
                    for surface in [p.paper, p.panel, p.selection, p.alternate] {
                        assert!(
                            contrast(text, surface) >= 4.5,
                            "{scheme:?}, dark={dark}: {text:?} on {surface:?}"
                        );
                    }
                }
                assert!(contrast(Color32::WHITE, p.action) >= 7.0);
                assert!(contrast(p.paper, p.green) >= 4.5);
                assert!(contrast(p.line, p.alternate) >= 7.0);
                assert!(contrast(p.grid, p.canvas) >= 3.0);
                for heat in [p.heat_low, p.heat_high] {
                    assert!(contrast(p.ink, heat) >= 7.0);
                    assert!(contrast(p.green, heat) >= 3.0);
                    assert!(contrast(p.secondary, heat) >= 3.0);
                }
            }
        }
    }

    #[test]
    fn interface_and_chart_surfaces_have_no_pastel_tints() {
        for scheme in [Scheme::Evergreen, Scheme::Urbane] {
            for dark in [false, true] {
                let p = Palette::new(dark, scheme);
                for surface in [
                    p.paper,
                    p.panel,
                    p.selection,
                    p.alternate,
                    p.canvas,
                    p.heat_low,
                    p.heat_high,
                ] {
                    assert_eq!(surface.r(), surface.g());
                    assert_eq!(surface.g(), surface.b());
                }
                assert_eq!(p.brand, rgb(0x00955A));
                if scheme == Scheme::Evergreen {
                    assert_eq!(p.action, rgb(0x01613B));
                }
            }
        }
    }
}
