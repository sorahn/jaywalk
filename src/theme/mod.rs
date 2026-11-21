//! Immutable resolved shell colors. Parsing and generation happen at settings
//! resolution, not in paint; renderers consume semantic roles only.
pub mod color;
pub mod roles;
mod settings;
pub use color::Color;
pub use roles::{Part, Role, Surface};
pub use settings::{Adjustment, Algorithm, Number, ShellAuto, ThemeSettings};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    BuiltIn,
    Generated,
    Override,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResolvedColor {
    pub color: Color,
    pub origin: Origin,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Shell {
    colors: BTreeMap<Role, ResolvedColor>,
    pub warnings: Vec<String>,
}
impl Shell {
    pub fn get(&self, role: Role) -> ResolvedColor {
        *self
            .colors
            .get(&role)
            .expect("the resolved shell contains every declared role")
    }
    pub fn surface(&self, surface: Surface, part: Part) -> Color {
        self.get(Role::Surface(surface, part)).color
    }
    pub fn colors(&self) -> impl Iterator<Item = (&Role, &ResolvedColor)> {
        self.colors.iter()
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedThemes {
    pub light: Arc<Shell>,
    pub dark: Arc<Shell>,
}
impl ResolvedThemes {
    pub fn shell(&self, dark: bool) -> &Arc<Shell> {
        if dark { &self.dark } else { &self.light }
    }
}
pub fn resolve(settings: &ThemeSettings) -> ResolvedThemes {
    ResolvedThemes {
        light: Arc::new(resolve_mode(settings, false)),
        dark: Arc::new(resolve_mode(settings, true)),
    }
}

fn reference(surface: Surface, dark: bool) -> Color {
    let hex = if dark {
        match surface {
            Surface::Window
            | Surface::Header
            | Surface::Footer
            | Surface::HistoryFooter
            | Surface::FilesFooter => 0x3b414d,
            Surface::History | Surface::HistoryHeader | Surface::Floating => 0x2f343e,
            Surface::Files | Surface::FilesHeader => 0x323741,
            Surface::Viewer | Surface::ViewerHeader => 0x282c33,
        }
    } else {
        match surface {
            Surface::Window
            | Surface::Header
            | Surface::Footer
            | Surface::HistoryFooter
            | Surface::FilesFooter => 0xdcdcdd,
            Surface::History | Surface::HistoryHeader | Surface::Floating => 0xebebec,
            Surface::Files | Surface::FilesHeader => 0xe7e7e9,
            Surface::Viewer | Surface::ViewerHeader => 0xfafafa,
        }
    };
    Color::rgb(hex)
}
fn layer(surface: Surface) -> f64 {
    match surface {
        Surface::Window | Surface::Viewer | Surface::ViewerHeader => 0.,
        Surface::History => 0.04,
        Surface::Files => 0.02,
        Surface::Header | Surface::Footer | Surface::HistoryFooter | Surface::FilesFooter => 0.075,
        Surface::HistoryHeader | Surface::Floating => 0.05,
        Surface::FilesHeader => 0.03,
    }
}
fn generated(surface: Surface, r: &ShellAuto, dark: bool) -> Color {
    // Port of preferredFoundation/matrixCell + palette from the shell lab. The
    // gallery's layer multiplier is independent of recipe contrast. Calibration
    // stays derived from anchors, rather than becoming fixed final RGB overrides.
    let default_seed = Color::rgb(if dark { 0x111111 } else { 0x585858 });
    let [seed_l, seed_c, seed_h] = Color::rgb(r.seed.resolve(dark).value()).lch();
    let reference_l = Color::rgb(0x3b414d).lch()[0];
    let anchor_l = reference(Surface::Window, dark).lch()[0];
    let role_l = reference(surface, dark).lch()[0];
    let default_base = if dark {
        Color::rgb(0x111111).lch()[0]
    } else {
        0.98
    };
    let direction = if dark { 1. } else { -1. };
    let calibration = default_base + direction * layer(surface) * r.layer_strength.resolve(dark)
        - (default_seed.lch()[0] + role_l - reference_l);
    let base = anchor_l
        + (seed_l - reference_l) * r.seed_lightness_influence.resolve(dark)
        + r.lightness_shift.resolve(dark);
    let adjustment = r
        .roles
        .get(&Role::Surface(surface, Part::Background))
        .copied()
        .unwrap_or_default();
    Color::from_lch(
        base + (role_l - anchor_l) * r.contrast_spread.resolve(dark)
            + calibration
            + adjustment.lightness_shift.resolve(dark),
        seed_c
            * r.chroma_scale.resolve(dark)
            * r.tint_strength.resolve(dark)
            * if dark { 1. } else { 0.6 }
            * adjustment.chroma_scale.resolve(dark),
        seed_h + r.hue_shift.resolve(dark) + adjustment.hue_shift.resolve(dark),
    )
}
fn built_in(surface: Surface, dark: bool) -> Color {
    let hex = if dark {
        match surface {
            Surface::Window | Surface::Viewer | Surface::ViewerHeader => 0x111111,
            Surface::History => 0x1e1e1e,
            Surface::Files => 0x171717,
            Surface::Header | Surface::Footer | Surface::HistoryFooter | Surface::FilesFooter => {
                0x292929
            }
            Surface::HistoryHeader | Surface::Floating => 0x212121,
            Surface::FilesHeader => 0x1a1a1a,
        }
    } else {
        match surface {
            Surface::Window | Surface::Viewer | Surface::ViewerHeader => 0xf7f7f6,
            Surface::History => 0xe4e4e4,
            Surface::Files => 0xeeeeed,
            Surface::Header | Surface::Footer | Surface::HistoryFooter | Surface::FilesFooter => {
                0xd4d4d4
            }
            Surface::HistoryHeader | Surface::Floating => 0xe0e0df,
            Surface::FilesHeader => 0xe9e9e8,
        }
    };
    Color::rgb(hex)
}
fn resolve_mode(settings: &ThemeSettings, dark: bool) -> Shell {
    let r = &settings.shell_auto;
    let origin = if r.enabled {
        Origin::Generated
    } else {
        Origin::BuiltIn
    };
    let mut colors = BTreeMap::new();
    let override_color = |role: Role, default: Color| {
        settings
            .overrides
            .get(&role)
            .map(|c| ResolvedColor {
                color: Color::rgb(c.resolve(dark).value()),
                origin: Origin::Override,
            })
            .unwrap_or(ResolvedColor {
                color: default,
                origin,
            })
    };
    let seed = if r.enabled {
        Color::rgb(r.seed.resolve(dark).value())
    } else {
        Color::rgb(if dark { 0x111111 } else { 0x585858 })
    };
    let lch = seed.lch();
    let border = Color::from_lch(
        if dark { 0.46 } else { 0.76 },
        lch[1] * 0.4,
        lch[2]
            + if r.enabled {
                r.hue_shift.resolve(dark)
            } else {
                0.
            },
    );
    for surface in Surface::ALL {
        let bg_role = Role::Surface(surface, Part::Background);
        let bg = override_color(
            bg_role,
            if r.enabled {
                generated(surface, r, dark)
            } else {
                built_in(surface, dark)
            },
        );
        let fg = override_color(
            Role::Surface(surface, Part::Foreground),
            bg.color.readable(),
        );
        colors.insert(bg_role, bg);
        colors.insert(Role::Surface(surface, Part::Foreground), fg);
        colors.insert(
            Role::Surface(surface, Part::MutedForeground),
            override_color(
                Role::Surface(surface, Part::MutedForeground),
                bg.color.muted(fg.color),
            ),
        );
        colors.insert(
            Role::Surface(surface, Part::Border),
            override_color(Role::Surface(surface, Part::Border), border),
        );
    }
    let window = colors[&Role::Surface(Surface::Window, Part::Background)].color;
    let foreground = colors[&Role::Surface(Surface::Window, Part::Foreground)].color;
    let graph = if dark {
        [0x5b9bd5, 0x7dc67c, 0xd9a066, 0xb18add, 0x66c4c7]
    } else {
        [0x0969da, 0x1a7f37, 0x9a6700, 0x8250df, 0x0e7490]
    };
    for (index, hex) in graph.into_iter().enumerate() {
        colors.insert(
            Role::GraphLane(index as u8),
            override_color(Role::GraphLane(index as u8), Color::rgb(hex)),
        );
    }
    for (role, hex) in [
        (
            Role::EvolutionCurrent,
            if dark { 0x9dcaff } else { 0x0969da },
        ),
        (Role::EvolutionMuted, if dark { 0x7797bd } else { 0x57606a }),
        (Role::Success, if dark { 0x7dc67c } else { 0x22863a }),
        (Role::Warning, if dark { 0xe5c07b } else { 0x8a6116 }),
        (Role::Error, if dark { 0xe58a83 } else { 0xc12e2a }),
        (Role::Info, if dark { 0x86b4ed } else { 0x0969da }),
        (Role::Renamed, if dark { 0x86b4ed } else { 0x0969da }),
        (Role::Copied, if dark { 0x86b4ed } else { 0x0969da }),
    ] {
        colors.insert(role, override_color(role, Color::rgb(hex)));
    }
    for (role, color) in [
        (Role::Backdrop, Color(0x00000070)),
        (Role::ControlBackground, window),
        (Role::ControlHover, window.blend(foreground, 0.08)),
        (Role::ControlForeground, foreground),
    ] {
        colors.insert(role, override_color(role, color));
    }
    let mut warnings = Vec::new();
    for surface in Surface::ALL {
        let bg = colors[&Role::Surface(surface, Part::Background)].color;
        for part in [Part::Foreground, Part::MutedForeground] {
            let role = Role::Surface(surface, part);
            let contrast = colors[&role].color.contrast(bg);
            if contrast < 4.5 {
                warnings.push(format!(
                    "{role} has {contrast:.2}:1 contrast on {}.background",
                    surface.path()
                ));
            }
        }
    }
    Shell { colors, warnings }
}

#[cfg(test)]
mod tests;
