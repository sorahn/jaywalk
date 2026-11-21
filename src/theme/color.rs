//! Dependency-free OKLCH/sRGB math shared with the approved shell prototype.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color(pub u32);
impl Color {
    pub const fn rgb(value: u32) -> Self {
        Self((value << 8) | 255)
    }
    pub fn rgb_value(self) -> u32 {
        self.0 >> 8
    }
    pub fn channels(self) -> [f64; 3] {
        [24, 16, 8].map(|shift| ((self.0 >> shift) & 255) as f64 / 255.)
    }
    pub fn luminance(self) -> f64 {
        let [r, g, b] = self.channels().map(linear);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }
    pub fn contrast(self, other: Self) -> f64 {
        let (a, b) = (self.luminance(), other.luminance());
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    pub fn blend(self, foreground: Self, alpha: f64) -> Self {
        let a = self.channels();
        let b = foreground.channels();
        from_rgb(std::array::from_fn(|i| a[i] * (1. - alpha) + b[i] * alpha))
    }
    pub fn readable(self) -> Self {
        let light = Self::rgb(0xeef2f6);
        let dark = Self::rgb(0x17212b);
        let best = if self.contrast(light) > self.contrast(dark) {
            light
        } else {
            dark
        };
        if self.contrast(best) >= 4.5 {
            best
        } else if self.contrast(Self::rgb(0xffffff)) > self.contrast(Self::rgb(0)) {
            Self::rgb(0xffffff)
        } else {
            Self::rgb(0)
        }
    }
    pub fn muted(self, foreground: Self) -> Self {
        for step in 0..=35 {
            let candidate = self.blend(foreground, 0.65 + f64::from(step) * 0.01);
            if self.contrast(candidate) >= 4.5 {
                return candidate;
            }
        }
        foreground
    }
    pub fn lch(self) -> [f64; 3] {
        let [r, g, b] = self.channels().map(linear);
        let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
        let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
        let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
        let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
        let b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
        [
            0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
            a.hypot(b),
            b.atan2(a).to_degrees(),
        ]
    }
    pub fn from_lch(lightness: f64, chroma: f64, hue: f64) -> Self {
        let l = lightness.clamp(0.015, 0.995);
        let c = chroma.max(0.);
        let gamut = |rgb: [f64; 3]| rgb.iter().all(|v| (-1e-7..=1.0000001).contains(v));
        let mut rgb = raw_rgb(l, c, hue);
        if !gamut(rgb) {
            let (mut lo, mut hi) = (0., c);
            for _ in 0..22 {
                let mid = (lo + hi) / 2.;
                if gamut(raw_rgb(l, mid, hue)) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            rgb = raw_rgb(l, lo, hue);
        }
        from_rgb(rgb.map(|v| encoded(v.clamp(0., 1.)).clamp(0., 1.)))
    }
}
fn linear(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn encoded(x: f64) -> f64 {
    if x <= 0.0031308 {
        12.92 * x
    } else {
        1.055 * x.powf(1. / 2.4) - 0.055
    }
}
fn from_rgb(rgb: [f64; 3]) -> Color {
    Color::rgb(rgb.into_iter().fold(0, |hex, v| {
        (hex << 8) | (v.clamp(0., 1.) * 255.).round() as u32
    }))
}
fn raw_rgb(l: f64, c: f64, h: f64) -> [f64; 3] {
    let a = c * h.to_radians().cos();
    let b = c * h.to_radians().sin();
    let ll = (l + 0.3963377774 * a + 0.2158037573 * b).powi(3);
    let m = (l - 0.1055613458 * a - 0.0638541728 * b).powi(3);
    let s = (l - 0.0894841775 * a - 1.291485548 * b).powi(3);
    [
        4.0767416621 * ll - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * ll + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * ll - 0.7034186147 * m + 1.707614701 * s,
    ]
}
