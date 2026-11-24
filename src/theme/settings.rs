use super::roles::Role;
use crate::settings::ColorSetting;
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(untagged, deny_unknown_fields)]
pub enum Number {
    Shared(f64),
    Modes { light: f64, dark: f64 },
}
impl Number {
    pub fn resolve(self, dark: bool) -> f64 {
        match self {
            Self::Shared(v) => v,
            Self::Modes { light, dark: d } => {
                if dark {
                    d
                } else {
                    light
                }
            }
        }
    }
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
pub enum Algorithm {
    #[serde(rename = "tonal-v1")]
    TonalV1,
}
/// Custom seeds retain tonal-v1; code themes use viewer-contrast-v1.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Custom,
    CodeTheme,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ThemeSettings {
    pub shell_auto: ShellAuto,
    pub overrides: BTreeMap<Role, ColorSetting>,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ShellAuto {
    pub source: Source,
    pub enabled: bool,
    pub algorithm: Algorithm,
    pub seed: ColorSetting,
    pub layer_strength: Number,
    pub hue_shift: Number,
    pub chroma_scale: Number,
    pub tint_strength: Number,
    pub contrast_spread: Number,
    pub lightness_shift: Number,
    pub seed_lightness_influence: Number,
    #[serde(default)]
    pub roles: BTreeMap<Role, Adjustment>,
}
#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Adjustment {
    pub lightness_shift: Number,
    pub hue_shift: Number,
    pub chroma_scale: Number,
}
impl Default for Adjustment {
    fn default() -> Self {
        Self {
            lightness_shift: Number::Shared(0.),
            hue_shift: Number::Shared(0.),
            chroma_scale: Number::Shared(1.),
        }
    }
}
fn bounded(name: &str, value: Number, min: f64, max: f64) -> Result<(), String> {
    for dark in [false, true] {
        let v = value.resolve(dark);
        if !v.is_finite() || !(min..=max).contains(&v) {
            return Err(format!("{name} must be finite and between {min} and {max}"));
        }
    }
    Ok(())
}
impl ThemeSettings {
    pub fn validate(&self) -> Result<(), String> {
        let r = &self.shell_auto;
        for (name, n, min, max) in [
            ("layer_strength", r.layer_strength, 0., 2.5),
            ("hue_shift", r.hue_shift, -180., 180.),
            ("chroma_scale", r.chroma_scale, 0., 3.),
            ("tint_strength", r.tint_strength, 0., 1.),
            ("contrast_spread", r.contrast_spread, 0.2, 2.5),
            ("lightness_shift", r.lightness_shift, -0.15, 0.15),
            (
                "seed_lightness_influence",
                r.seed_lightness_influence,
                0.,
                1.,
            ),
        ] {
            bounded(&format!("theme.shell_auto.{name}"), n, min, max)?;
        }
        for (role, a) in &r.roles {
            if !matches!(role, Role::Surface(_, super::Part::Background)) {
                return Err(format!(
                    "theme.shell_auto.roles.{role}: adjustments require a surface background role"
                ));
            }
            bounded(
                &format!("{role}.lightness_shift"),
                a.lightness_shift,
                -0.2,
                0.2,
            )?;
            bounded(&format!("{role}.hue_shift"), a.hue_shift, -180., 180.)?;
            bounded(&format!("{role}.chroma_scale"), a.chroma_scale, 0., 3.)?;
        }
        Ok(())
    }
}
