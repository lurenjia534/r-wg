use gpui::{Hsla, SharedString};
use gpui_component::theme::{Theme, ThemeConfig, ThemeSet};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ThemeLintSeverity {
    Error,
    Warning,
    Info,
}

impl ThemeLintSeverity {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Error => "Error",
            Self::Warning => "Warning",
            Self::Info => "Info",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ThemeLintItem {
    pub(crate) severity: ThemeLintSeverity,
    pub(crate) title: &'static str,
    pub(crate) detail: SharedString,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ThemeLintCounts {
    pub(crate) errors: usize,
    pub(crate) warnings: usize,
    pub(crate) infos: usize,
}

impl ThemeLintCounts {
    pub(crate) fn add_item(&mut self, item: &ThemeLintItem) {
        match item.severity {
            ThemeLintSeverity::Error => self.errors += 1,
            ThemeLintSeverity::Warning => self.warnings += 1,
            ThemeLintSeverity::Info => self.infos += 1,
        }
    }

    pub(crate) fn add_items<'a>(&mut self, items: impl IntoIterator<Item = &'a ThemeLintItem>) {
        for item in items {
            self.add_item(item);
        }
    }

    pub(crate) fn add_counts(&mut self, other: Self) {
        self.errors += other.errors;
        self.warnings += other.warnings;
        self.infos += other.infos;
    }

    pub(crate) fn summary_label(self) -> String {
        let mut parts = Vec::new();
        if self.errors > 0 {
            parts.push(format_count(self.errors, "error"));
        }
        if self.warnings > 0 {
            parts.push(format_count(self.warnings, "warning"));
        }
        if parts.is_empty() {
            if self.infos > 0 {
                parts.push(format_count(self.infos, "note"));
            } else {
                parts.push("0 warnings".to_string());
            }
        }
        parts.join(", ")
    }
}

pub(crate) fn lint_theme_config(config: &ThemeConfig) -> Vec<ThemeLintItem> {
    let mut theme = Theme::default();
    theme.apply_config(&Rc::new(config.clone()));
    lint_theme(&ThemeLintSnapshot {
        background: theme.background,
        panel: theme.group_box,
        foreground: theme.foreground,
        muted_foreground: theme.muted_foreground,
        accent: theme.accent,
        success: theme.success,
        warning: theme.warning,
        info: theme.info,
        danger: theme.danger,
        chart_1: theme.chart_1,
        chart_2: theme.chart_2,
        chart_3: theme.chart_3,
        chart_4: theme.chart_4,
        chart_5: theme.chart_5,
        input_border: theme.input,
        sidebar: theme.sidebar,
        sidebar_foreground: theme.sidebar_foreground,
    })
}

pub(crate) fn lint_theme_set(theme_set: &ThemeSet) -> ThemeLintCounts {
    let mut counts = ThemeLintCounts::default();
    for theme in &theme_set.themes {
        let items = lint_theme_config(theme);
        counts.add_items(items.iter());
    }
    counts
}

struct ThemeLintSnapshot {
    background: Hsla,
    panel: Hsla,
    foreground: Hsla,
    muted_foreground: Hsla,
    accent: Hsla,
    success: Hsla,
    warning: Hsla,
    info: Hsla,
    danger: Hsla,
    chart_1: Hsla,
    chart_2: Hsla,
    chart_3: Hsla,
    chart_4: Hsla,
    chart_5: Hsla,
    input_border: Hsla,
    sidebar: Hsla,
    sidebar_foreground: Hsla,
}

fn lint_theme(preview: &ThemeLintSnapshot) -> Vec<ThemeLintItem> {
    let mut items = Vec::new();

    let body_contrast = contrast_ratio(preview.foreground, preview.background);
    if body_contrast < 4.5 {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Error,
            title: "Body text contrast is weak",
            detail: format!(
                "Foreground on background is {:.1}:1. Aim for at least 4.5:1.",
                body_contrast
            )
            .into(),
        });
    }

    let muted_contrast = contrast_ratio(preview.muted_foreground, preview.panel);
    if muted_contrast < 2.8 {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Warning,
            title: "Muted copy may wash out on panels",
            detail: format!(
                "Muted foreground on panel is {:.1}:1, which is low for labels and helper text.",
                muted_contrast
            )
            .into(),
        });
    }

    let input_boundary = contrast_ratio(preview.input_border, preview.background);
    if input_boundary < 1.25 && color_distance(preview.input_border, preview.background) < 0.09 {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Warning,
            title: "Input borders blend into the canvas",
            detail: format!(
                "Input border contrast is {:.2}:1 and the tone is close to the page background.",
                input_boundary
            )
            .into(),
        });
    }

    let semantic_pairs = [
        ("success", preview.success),
        ("warning", preview.warning),
        ("danger", preview.danger),
        ("info", preview.info),
    ];
    if let Some((name, distance)) = semantic_pairs
        .into_iter()
        .map(|(name, color)| (name, color_distance(preview.accent, color)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .filter(|(_, distance)| *distance < 0.11)
    {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Warning,
            title: "Accent is too close to a semantic state",
            detail: format!(
                "Accent and {name} are only {:.2} apart, which can blur status meaning.",
                distance
            )
            .into(),
        });
    }

    let charts = [
        preview.chart_1,
        preview.chart_2,
        preview.chart_3,
        preview.chart_4,
        preview.chart_5,
    ];
    let mut min_chart_distance = f32::MAX;
    for (index, color) in charts.iter().enumerate() {
        for other in charts.iter().skip(index + 1) {
            min_chart_distance = min_chart_distance.min(color_distance(*color, *other));
        }
    }
    if min_chart_distance < 0.1 {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Warning,
            title: "Chart series are too similar",
            detail: format!(
                "Closest chart colors are only {:.2} apart, so traffic lines may merge visually.",
                min_chart_distance
            )
            .into(),
        });
    }

    let sidebar_contrast = contrast_ratio(preview.sidebar_foreground, preview.sidebar);
    if sidebar_contrast < 3.2 {
        items.push(ThemeLintItem {
            severity: ThemeLintSeverity::Info,
            title: "Sidebar text sits near the contrast floor",
            detail: format!(
                "Sidebar foreground on sidebar background is {:.1}:1.",
                sidebar_contrast
            )
            .into(),
        });
    }

    items
}

fn format_count(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn contrast_ratio(foreground: Hsla, background: Hsla) -> f32 {
    let foreground = relative_luminance(foreground);
    let background = relative_luminance(background);
    let lighter = foreground.max(background);
    let darker = foreground.min(background);
    (lighter + 0.05) / (darker + 0.05)
}

fn relative_luminance(color: Hsla) -> f32 {
    let rgb = color.to_rgb();
    fn channel(value: f32) -> f32 {
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }

    0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b)
}

fn color_distance(a: Hsla, b: Hsla) -> f32 {
    let hue = (a.h - b.h).abs();
    let hue = hue.min(1.0 - hue);
    let saturation = (a.s - b.s).abs();
    let lightness = (a.l - b.l).abs();
    let alpha = (a.a - b.a).abs();
    (hue * 0.45) + (saturation * 0.2) + (lightness * 0.3) + (alpha * 0.05)
}
