//! The encode-settings model.
//!
//! One entry per output format rawshift can encode, each holding that format's
//! real configuration struct from
//! [`rawshift_image::formats::export`] — not a parallel copy of it. The UI
//! mutates those structs directly, so every parameter the library exposes is
//! reachable from the window and none can silently drift out of sync with the
//! library's defaults.
//!
//! Deliberately free of `iced` types: the headless runner and the unit tests
//! drive this module directly.

use rawshift_image::formats::export::{
    AvifEncodeConfig, CommonEncodeOptions, EncodeOptions, JpegEncodeConfig, JxlEncodeConfig,
    OutputFormat, PngEncodeConfig, WebpEncodeConfig,
};

/// One format's configuration, as the library models it.
///
/// Mirrors the [`EncodeOptions`] variants rather than wrapping them, so the
/// enum arms track the library's own feature gating.
#[derive(Debug, Clone, PartialEq)]
pub enum FormatConfig {
    Png(PngEncodeConfig),
    Jpeg(JpegEncodeConfig),
    WebP(WebpEncodeConfig),
    Avif(AvifEncodeConfig),
    Jxl(JxlEncodeConfig),
}

impl FormatConfig {
    /// The output format this configuration produces.
    pub fn format(&self) -> OutputFormat {
        match self {
            FormatConfig::Png(_) => OutputFormat::Png,
            FormatConfig::Jpeg(_) => OutputFormat::Jpeg,
            FormatConfig::WebP(_) => OutputFormat::WebP,
            FormatConfig::Avif(_) => OutputFormat::Avif,
            FormatConfig::Jxl(_) => OutputFormat::Jxl,
        }
    }

    /// Whether this configuration is a mathematically lossless one, so the
    /// round trip can be asserted bit-exact rather than merely close.
    ///
    /// PNG is lossless by construction. The others carry an explicit mode.
    pub fn is_lossless(&self) -> bool {
        match self {
            FormatConfig::Png(_) => true,
            FormatConfig::Jpeg(_) => false,
            FormatConfig::WebP(cfg) => {
                matches!(
                    cfg.mode,
                    rawshift_image::formats::export::WebPMode::Lossless
                )
            }
            FormatConfig::Avif(cfg) => cfg.lossless,
            FormatConfig::Jxl(cfg) => cfg.lossless,
        }
    }

    /// The encoder-agnostic options in effect, for the UI to edit.
    pub fn common_mut(&mut self) -> &mut CommonEncodeOptions {
        match self {
            FormatConfig::Png(cfg) => &mut cfg.common,
            FormatConfig::Jpeg(cfg) => &mut cfg.common,
            FormatConfig::WebP(cfg) => &mut cfg.common,
            FormatConfig::Avif(cfg) => &mut cfg.common,
            FormatConfig::Jxl(cfg) => &mut cfg.common,
        }
    }

    /// Build the library's own options value for an encode.
    pub fn to_encode_options(&self) -> EncodeOptions {
        match self.clone() {
            FormatConfig::Png(cfg) => EncodeOptions::Png(cfg),
            FormatConfig::Jpeg(cfg) => EncodeOptions::Jpeg(cfg),
            FormatConfig::WebP(cfg) => EncodeOptions::WebP(cfg),
            FormatConfig::Avif(cfg) => EncodeOptions::Avif(cfg),
            FormatConfig::Jxl(cfg) => EncodeOptions::Jxl(cfg),
        }
    }

    /// A one-line summary of the settings actually in force, for the results
    /// grid and the headless table.
    ///
    /// Only parameters the encoder will honour in the current mode appear —
    /// quoting a JXL distance next to `lossless` would misrepresent what ran.
    pub fn summary(&self) -> String {
        match self {
            FormatConfig::Png(cfg) => format!(
                "{:?} compression, {:?} filter{}",
                cfg.compression,
                cfg.filter,
                if cfg.auto_reduce { ", auto-reduce" } else { "" }
            ),
            FormatConfig::Jpeg(cfg) => format!(
                "q{} {:?}{}{}",
                cfg.quality,
                cfg.subsampling,
                if cfg.progressive { " progressive" } else { "" },
                if cfg.restart_interval > 0 {
                    format!(" restart={}", cfg.restart_interval)
                } else {
                    String::new()
                }
            ),
            FormatConfig::WebP(cfg) => match cfg.mode {
                rawshift_image::formats::export::WebPMode::Lossless => "lossless".to_owned(),
                rawshift_image::formats::export::WebPMode::Lossy => {
                    format!("lossy q{}", cfg.quality)
                }
            },
            FormatConfig::Avif(cfg) => {
                if cfg.lossless {
                    "lossless".to_owned()
                } else {
                    format!("lossy q{}", cfg.quality)
                }
            }
            FormatConfig::Jxl(cfg) => {
                let mode = if cfg.lossless {
                    "lossless".to_owned()
                } else {
                    format!("distance {:.2}", cfg.distance)
                };
                format!(
                    "{mode}, effort {}{}",
                    cfg.effort,
                    if cfg.use_container { ", container" } else { "" }
                )
            }
        }
    }
}

/// One row of the settings panel: a format, whether it is selected, and its
/// parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct FormatSettings {
    pub enabled: bool,
    pub config: FormatConfig,
    /// Whether the panel section is expanded in the UI.
    pub expanded: bool,
}

/// Every encodable format, with the shared options applied across all of them.
#[derive(Debug, Clone, PartialEq)]
pub struct EncodeSettings {
    /// Applied to every format before encoding, so bit depth and metadata
    /// embedding are set once rather than per format.
    pub common: CommonEncodeOptions,
    pub formats: Vec<FormatSettings>,
}

impl Default for EncodeSettings {
    /// Every format the build can encode, at the library's own defaults.
    ///
    /// PNG and JPEG start enabled: one lossless path and one lossy path, so a
    /// first run shows both a bit-exact result and a PSNR without the user
    /// having to choose anything.
    fn default() -> Self {
        let mut formats = Vec::new();
        let mut push = |config: FormatConfig, enabled: bool| {
            formats.push(FormatSettings {
                enabled,
                config,
                expanded: enabled,
            });
        };
        push(FormatConfig::Png(PngEncodeConfig::default()), true);
        push(FormatConfig::Jpeg(JpegEncodeConfig::default()), true);
        push(FormatConfig::WebP(WebpEncodeConfig::lossy()), false);
        push(FormatConfig::Avif(AvifEncodeConfig::default()), false);
        push(FormatConfig::Jxl(JxlEncodeConfig::default()), false);
        Self {
            common: CommonEncodeOptions::default(),
            formats,
        }
    }
}

impl EncodeSettings {
    /// The selected formats, with [`Self::common`] applied to each.
    ///
    /// Returned in panel order so the results grid columns are stable across
    /// runs — a column that moved between runs would make the side-by-side
    /// comparison useless.
    pub fn selected(&self) -> Vec<FormatConfig> {
        self.formats
            .iter()
            .filter(|f| f.enabled)
            .map(|f| {
                let mut config = f.config.clone();
                *config.common_mut() = self.common;
                config
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_image::formats::export::{BitDepth, WebPMode};

    #[test]
    fn defaults_enable_one_lossless_and_one_lossy_format() {
        let settings = EncodeSettings::default();
        let selected = settings.selected();
        assert_eq!(selected.len(), 2);
        assert!(selected.iter().any(|c| c.is_lossless()));
        assert!(selected.iter().any(|c| !c.is_lossless()));
    }

    #[test]
    fn common_options_override_each_format() {
        let mut settings = EncodeSettings::default();
        settings.common.bit_depth = BitDepth::Eight;
        for config in settings.selected() {
            let mut config = config;
            assert_eq!(config.common_mut().bit_depth, BitDepth::Eight);
        }
    }

    #[test]
    fn selected_preserves_panel_order() {
        let mut settings = EncodeSettings::default();
        for f in &mut settings.formats {
            f.enabled = true;
        }
        let order: Vec<_> = settings.selected().iter().map(|c| c.format()).collect();
        assert_eq!(
            order,
            vec![
                OutputFormat::Png,
                OutputFormat::Jpeg,
                OutputFormat::WebP,
                OutputFormat::Avif,
                OutputFormat::Jxl,
            ]
        );
    }

    #[test]
    fn losslessness_tracks_the_mode_flag() {
        assert!(FormatConfig::Png(PngEncodeConfig::default()).is_lossless());
        assert!(!FormatConfig::Jpeg(JpegEncodeConfig::default()).is_lossless());
        assert!(FormatConfig::WebP(WebpEncodeConfig::lossless()).is_lossless());
        assert!(!FormatConfig::WebP(WebpEncodeConfig::lossy()).is_lossless());
    }

    #[test]
    fn summary_omits_parameters_the_mode_ignores() {
        let lossless = FormatConfig::Jxl(JxlEncodeConfig::default());
        assert!(lossless.summary().contains("lossless"));
        assert!(
            !lossless.summary().contains("distance"),
            "a lossless JXL must not quote a Butteraugli distance"
        );

        let lossy = FormatConfig::WebP(WebpEncodeConfig {
            mode: WebPMode::Lossy,
            quality: 42,
            ..WebpEncodeConfig::lossy()
        });
        assert!(lossy.summary().contains("q42"));
    }
}
