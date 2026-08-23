//! The window: state, messages, update, view.
//!
//! Deliberately thin. Everything that decides anything lives in
//! [`crate::pipeline`], [`crate::color`], and [`crate::settings`]; this module
//! moves their results onto the screen and routes user input back. That split
//! is what lets `--headless` exercise the same behaviour with no display.
//!
//! Work runs off the UI thread via [`iced::Task::perform`], one source at a
//! time, so a large RAW cannot freeze the window.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::{
    Space, button, checkbox, column, container, image, pick_list, row, rule, scrollable, slider,
    text,
};
use iced::{Alignment, Element, Fill, Length, Task, Theme};

use rawshift_image::formats::export::{
    BitDepth, JpegSubsampling, PngCompressionLevel, PngFilterStrategy, PngFilterType, WebPMode,
};

use crate::pipeline::{self, RawSettings, Source, Variant};
use crate::render::{self, Preview};
use crate::settings::{EncodeSettings, FormatConfig};

/// Launch the window.
pub fn run(initial: Vec<PathBuf>) -> iced::Result {
    iced::application(
        move || {
            let state = State::default();
            let task = if initial.is_empty() {
                Task::none()
            } else {
                Task::done(Message::FilesChosen(initial.clone()))
            };
            (state, task)
        },
        State::update,
        State::view,
    )
    .title("rawshift gallery")
    // Annotated so the closure infers as higher-ranked over the borrow.
    .theme(|_state: &State| Theme::Dark)
    .run()
}

/// One loaded source and its round-trip results.
struct Row {
    source: Source,
    preview: Preview,
    variants: Vec<Variant>,
    /// Previews for the round-tripped variants, indexed alongside `variants`.
    variant_previews: Vec<Option<Preview>>,
}

#[derive(Default)]
pub struct State {
    settings: EncodeSettings,
    raw: RawSettings,
    rows: Vec<Row>,
    /// Paths still waiting to be processed, so the UI can show progress and
    /// stay responsive between them.
    queue: Vec<PathBuf>,
    /// Files that could not even be decoded, shown as their own placeholders.
    rejected: Vec<(PathBuf, String)>,
    busy: bool,
    banner: String,
}

/// The outcome of processing one source off-thread.
///
/// `Arc` because `Message` must be `Clone` while `Source` holds a full decoded
/// image — cloning that per message would copy the pixels.
type Processed = Arc<Result<(Source, Vec<Variant>), (PathBuf, String)>>;

#[derive(Debug, Clone)]
pub enum Message {
    Browse,
    FilesChosen(Vec<PathBuf>),
    /// One source finished processing off-thread.
    SourceDone(Processed),
    Clear,
    Rerun,
    ToggleFormat(usize, bool),
    ToggleExpanded(usize),
    ConfigChanged(usize, ConfigEdit),
    BitDepthChanged(BitDepth),
    EmbedExif(bool),
    EmbedIcc(bool),
    EmbedXmp(bool),
    RawGamma(f32),
}

/// A single parameter edit, one variant per control in the settings panel.
#[derive(Debug, Clone)]
pub enum ConfigEdit {
    PngCompression(PngCompressionLevel),
    PngFilter(PngFilterStrategy),
    PngAutoReduce(bool),
    JpegQuality(u8),
    JpegSubsampling(JpegSubsampling),
    JpegProgressive(bool),
    JpegRestart(u16),
    WebpMode(WebPMode),
    WebpQuality(u8),
    AvifLossless(bool),
    AvifQuality(u8),
    JxlLossless(bool),
    JxlDistance(f32),
    JxlEffort(u8),
    JxlContainer(bool),
}

impl State {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Browse => {
                return Task::perform(pick_files(), Message::FilesChosen);
            }
            Message::FilesChosen(paths) => {
                if paths.is_empty() {
                    return Task::none();
                }
                self.queue.extend(paths);
                return self.pump();
            }
            Message::SourceDone(result) => {
                self.busy = false;
                match Arc::try_unwrap(result) {
                    Ok(Ok((source, variants))) => self.push_row(source, variants),
                    Ok(Err((path, error))) => self.rejected.push((path, error)),
                    // Another handle exists only if this message were cloned,
                    // which the runtime does not do; recover rather than panic.
                    Err(shared) => match &*shared {
                        Ok((source, variants)) => {
                            self.push_row(source.clone(), variants.clone());
                        }
                        Err((path, error)) => self.rejected.push((path.clone(), error.clone())),
                    },
                }
                return self.pump();
            }
            Message::Clear => {
                self.rows.clear();
                self.rejected.clear();
                self.queue.clear();
                pipeline::clean_scratch();
            }
            Message::Rerun => {
                let paths: Vec<PathBuf> = self
                    .rows
                    .iter()
                    .map(|r| r.source.path.clone())
                    .chain(self.rejected.iter().map(|(p, _)| p.clone()))
                    .collect();
                self.rows.clear();
                self.rejected.clear();
                self.queue = paths;
                return self.pump();
            }
            Message::ToggleFormat(index, enabled) => {
                if let Some(format) = self.settings.formats.get_mut(index) {
                    format.enabled = enabled;
                    if enabled {
                        format.expanded = true;
                    }
                }
            }
            Message::ToggleExpanded(index) => {
                if let Some(format) = self.settings.formats.get_mut(index) {
                    format.expanded = !format.expanded;
                }
            }
            Message::ConfigChanged(index, edit) => {
                if let Some(format) = self.settings.formats.get_mut(index) {
                    apply_edit(&mut format.config, edit);
                }
            }
            Message::BitDepthChanged(depth) => self.settings.common.bit_depth = depth,
            Message::EmbedExif(v) => self.settings.common.metadata.embed_exif = v,
            Message::EmbedIcc(v) => self.settings.common.metadata.embed_icc = v,
            Message::EmbedXmp(v) => self.settings.common.metadata.embed_xmp = v,
            Message::RawGamma(gamma) => self.raw.gamma = gamma,
        }
        Task::none()
    }

    fn push_row(&mut self, source: Source, variants: Vec<Variant>) {
        let preview = render::preview(&source.image, &source.metadata);
        let variant_previews = variants
            .iter()
            .map(|v| {
                v.outcome
                    .as_ref()
                    .ok()
                    .map(|ok| render::preview(&ok.image, &source.metadata))
            })
            .collect();
        self.rows.push(Row {
            source,
            preview,
            variants,
            variant_previews,
        });
    }

    /// Start the next queued file, if the worker is free.
    ///
    /// One at a time rather than a fan-out: the encoders are already
    /// internally parallel (rayon), and processing several large sources at
    /// once would compete for the same cores while making the UI's progress
    /// report meaningless.
    fn pump(&mut self) -> Task<Message> {
        if self.busy || self.queue.is_empty() {
            return Task::none();
        }
        let path = self.queue.remove(0);
        let configs = self.settings.selected();
        let raw = self.raw;
        self.busy = true;
        Task::perform(
            async move {
                Arc::new(
                    tokio::task::spawn_blocking(move || process(path.clone(), &configs, raw))
                        .await
                        .unwrap_or_else(|e| {
                            Err((PathBuf::from("<worker>"), format!("worker panicked: {e}")))
                        }),
                )
            },
            Message::SourceDone,
        )
    }

    fn view(&self) -> Element<'_, Message> {
        let banner = if self.banner.is_empty() {
            crate::backend_banner()
        } else {
            self.banner.clone()
        };

        let header = column![
            row![
                text("rawshift gallery").size(22),
                Space::new().width(Fill),
                button("Add images…").on_press(Message::Browse),
                button("Re-run").on_press(Message::Rerun),
                button("Clear").on_press(Message::Clear),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(banner).size(12),
            text(format!(
                "intermediates: {}",
                pipeline::scratch_dir().display()
            ))
            .size(11),
        ]
        .spacing(4);

        let status = if self.busy || !self.queue.is_empty() {
            text(format!("working… {} queued", self.queue.len())).size(12)
        } else if self.rows.is_empty() && self.rejected.is_empty() {
            text("No images loaded. Use “Add images…” to pick some.").size(13)
        } else {
            text(format!(
                "{} image(s), {} rejected",
                self.rows.len(),
                self.rejected.len()
            ))
            .size(12)
        };

        let body = row![
            container(scrollable(self.settings_panel()).height(Fill))
                .width(Length::Fixed(320.0))
                .padding(8),
            rule::vertical(1),
            container(scrollable(self.results()).height(Fill).width(Fill)).padding(8),
        ];

        column![
            container(header).padding(10),
            rule::horizontal(1),
            container(status).padding([4, 10]),
            body,
        ]
        .into()
    }

    fn settings_panel(&self) -> Element<'_, Message> {
        let mut panel = column![
            text("Shared").size(16),
            row![text("bit depth").size(12), Space::new().width(Fill), {
                let (options, selected) = choices(BIT_DEPTHS, &self.settings.common.bit_depth);
                pick_list(options, selected, |c: Choice<BitDepth>| {
                    Message::BitDepthChanged(c.value)
                })
                .text_size(12)
            },]
            .align_y(Alignment::Center),
            checkbox(self.settings.common.metadata.embed_exif)
                .label("embed EXIF")
                .on_toggle(Message::EmbedExif)
                .size(14)
                .text_size(12),
            checkbox(self.settings.common.metadata.embed_icc)
                .label("embed ICC")
                .on_toggle(Message::EmbedIcc)
                .size(14)
                .text_size(12),
            checkbox(self.settings.common.metadata.embed_xmp)
                .label("embed XMP")
                .on_toggle(Message::EmbedXmp)
                .size(14)
                .text_size(12),
            rule::horizontal(1),
            text("RAW development").size(16),
            row![
                text(format!("gamma {:.2}", self.raw.gamma)).size(12),
                Space::new().width(Fill),
                slider(1.0..=3.0, self.raw.gamma, Message::RawGamma).step(0.05),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            rule::horizontal(1),
            text("Output formats").size(16),
        ]
        .spacing(6);

        for (index, format) in self.settings.formats.iter().enumerate() {
            panel = panel.push(rule::horizontal(1));
            panel = panel.push(
                row![
                    checkbox(format.enabled)
                        .label(format.config.format().name())
                        .on_toggle(move |v| Message::ToggleFormat(index, v))
                        .size(15),
                    Space::new().width(Fill),
                    button(text(if format.expanded { "−" } else { "+" }).size(12))
                        .on_press(Message::ToggleExpanded(index))
                        .padding([0, 6]),
                ]
                .align_y(Alignment::Center),
            );
            if format.expanded {
                panel =
                    panel.push(container(config_controls(index, &format.config)).padding([0, 12]));
            }
        }

        panel.spacing(6).into()
    }

    fn results(&self) -> Element<'_, Message> {
        let mut out = column![].spacing(16);

        for (path, error) in &self.rejected {
            out = out.push(placeholder_card(
                &format!("{}", path.display()),
                error,
                240.0,
            ));
        }

        for row_data in &self.rows {
            out = out.push(self.result_row(row_data));
        }

        out.into()
    }

    fn result_row<'a>(&'a self, row_data: &'a Row) -> Element<'a, Message> {
        let source = &row_data.source;
        let mut colour = column![].spacing(1);
        for (key, value) in crate::color::describe(&row_data.preview.report) {
            colour = colour.push(text(format!("{key}: {value}")).size(10));
        }

        let mut strip = row![
            column![
                text("source").size(12),
                image(row_data.preview.handle.clone())
                    .width(Length::Fixed(220.0))
                    .height(Length::Fixed(180.0)),
                text(crate::headless::summarise(source)).size(10),
                text(format!(
                    "{}×{}",
                    row_data.preview.source_size.0, row_data.preview.source_size.1
                ))
                .size(10),
                colour,
            ]
            .spacing(3)
            .width(Length::Fixed(230.0))
        ]
        .spacing(10);

        for (variant, preview) in row_data.variants.iter().zip(&row_data.variant_previews) {
            strip = strip.push(variant_cell(variant, preview.as_ref()));
        }

        column![
            text(source.name()).size(15),
            scrollable(strip).direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::new()
            )),
        ]
        .spacing(4)
        .into()
    }
}

/// One results-grid cell: the round-tripped image, or the error in its place.
fn variant_cell<'a>(variant: &'a Variant, preview: Option<&'a Preview>) -> Element<'a, Message> {
    let title = text(format!("{} — {}", variant.format.name(), variant.summary)).size(11);

    match (&variant.outcome, preview) {
        (Ok(ok), Some(preview)) => {
            let verdict = match ok.comparison {
                Some(c) if c.bit_exact => text("bit-exact ✓").size(11),
                Some(c) => text(format!(
                    "PSNR {:.2} dB · max Δ {}{}",
                    c.psnr_db.unwrap_or(f64::NAN),
                    c.max_delta,
                    // Naming the expectation makes a lossy result legible as
                    // "as designed" rather than "close enough".
                    if variant.lossless {
                        ""
                    } else {
                        " (lossy by design)"
                    }
                ))
                .size(11),
                None => text("dimensions differ").size(11),
            };
            let mut cell = column![
                title,
                image(preview.handle.clone())
                    .width(Length::Fixed(220.0))
                    .height(Length::Fixed(180.0)),
                text(format!(
                    "{} · encode {:.1} ms · decode {:.1} ms",
                    human_bytes(ok.encoded_bytes),
                    ok.encode_ms,
                    ok.decode_ms
                ))
                .size(10),
                // The intermediate really is a file; naming it lets the
                // viewer open it in another tool and check independently.
                text(format!(
                    "wrote {}",
                    ok.temp_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ))
                .size(9),
                verdict,
            ]
            .spacing(3);
            if ok.lossless_violated {
                cell = cell.push(
                    text("LOSSLESS VIOLATED — this configuration promised an exact round trip")
                        .size(10),
                );
            }
            container(cell).width(Length::Fixed(230.0)).into()
        }
        (Err(error), _) => container(placeholder_card(
            &format!("{} — {}", variant.format.name(), variant.summary),
            error,
            180.0,
        ))
        .width(Length::Fixed(230.0))
        .into(),
        // A success with no preview cannot occur (they are built together),
        // but rendering something is better than unwrapping.
        (Ok(_), None) => container(column![title, text("no preview").size(10)])
            .width(Length::Fixed(230.0))
            .into(),
    }
}

/// The empty image placeholder carrying an error message.
fn placeholder_card<'a>(title: &str, error: &str, height: f32) -> Element<'a, Message> {
    column![
        text(title.to_owned()).size(11),
        container(
            container(text(error.to_owned()).size(11))
                .padding(8)
                .center_x(Fill)
                .center_y(Fill)
        )
        .width(Length::Fixed(220.0))
        .height(Length::Fixed(height))
        .style(container::bordered_box),
    ]
    .spacing(3)
    .into()
}

/// A pick-list entry: a library value plus the label to show for it.
///
/// `pick_list` requires `Display`, and the library's configuration enums are
/// plain data that deliberately do not implement it — a human-readable name is
/// a presentation concern, and adding one upstream would put UI wording in a
/// public API. Wrapping keeps the label in the view where it belongs.
#[derive(Debug, Clone, PartialEq)]
struct Choice<T> {
    value: T,
    label: &'static str,
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label)
    }
}

/// Build a pick list from a value/label table, selecting the current value.
///
/// Returns `None` for the selection when `current` is not in the table, which
/// leaves the control blank rather than silently showing the wrong entry —
/// that would happen only if a `#[non_exhaustive]` library enum grew a variant
/// the panel has not been taught about, and a blank control is the honest
/// signal for it.
fn choices<T: Clone + PartialEq>(
    table: &[(T, &'static str)],
    current: &T,
) -> (Vec<Choice<T>>, Option<Choice<T>>) {
    let options: Vec<Choice<T>> = table
        .iter()
        .map(|(value, label)| Choice {
            value: value.clone(),
            label,
        })
        .collect();
    let selected = options.iter().find(|c| &c.value == current).cloned();
    (options, selected)
}

/// The parameter controls for one format — every field the config exposes.
fn config_controls(index: usize, config: &FormatConfig) -> Element<'_, Message> {
    let edit = move |e: ConfigEdit| Message::ConfigChanged(index, e);
    match config {
        FormatConfig::Png(cfg) => column![
            labelled("compression", {
                let (options, selected) = choices(PNG_COMPRESSION, &cfg.compression);
                pick_list(options, selected, move |c: Choice<PngCompressionLevel>| {
                    edit(ConfigEdit::PngCompression(c.value))
                })
                .text_size(11)
                .into()
            },),
            labelled("filter", {
                let (options, selected) = choices(PNG_FILTERS, &cfg.filter);
                pick_list(options, selected, move |c: Choice<PngFilterStrategy>| {
                    edit(ConfigEdit::PngFilter(c.value))
                })
                .text_size(11)
                .into()
            },),
            checkbox(cfg.auto_reduce)
                .label("auto-reduce colour type")
                .on_toggle(move |v| edit(ConfigEdit::PngAutoReduce(v)))
                .size(13)
                .text_size(11),
        ]
        .spacing(4)
        .into(),

        FormatConfig::Jpeg(cfg) => column![
            labelled(
                &format!("quality {}", cfg.quality),
                slider(1..=100u8, cfg.quality, move |v| edit(
                    ConfigEdit::JpegQuality(v)
                ))
                .into(),
            ),
            labelled("subsampling", {
                let (options, selected) = choices(JPEG_SUBSAMPLING, &cfg.subsampling);
                pick_list(options, selected, move |c: Choice<JpegSubsampling>| {
                    edit(ConfigEdit::JpegSubsampling(c.value))
                })
                .text_size(11)
                .into()
            },),
            checkbox(cfg.progressive)
                .label("progressive (SOF2)")
                .on_toggle(move |v| edit(ConfigEdit::JpegProgressive(v)))
                .size(13)
                .text_size(11),
            labelled(
                &format!("restart interval {}", cfg.restart_interval),
                slider(0..=64u16, cfg.restart_interval, move |v| edit(
                    ConfigEdit::JpegRestart(v)
                ))
                .into(),
            ),
        ]
        .spacing(4)
        .into(),

        FormatConfig::WebP(cfg) => {
            let lossless = matches!(cfg.mode, WebPMode::Lossless);
            let mut controls = column![
                checkbox(lossless)
                    .label("lossless")
                    .on_toggle(move |v| edit(ConfigEdit::WebpMode(if v {
                        WebPMode::Lossless
                    } else {
                        WebPMode::Lossy
                    })))
                    .size(13)
                    .text_size(11),
            ]
            .spacing(4);
            // Quality is ignored in lossless mode, so it is not offered there
            // rather than shown as a control with no effect.
            if !lossless {
                controls = controls.push(labelled(
                    &format!("quality {}", cfg.quality),
                    slider(0..=100u8, cfg.quality, move |v| {
                        edit(ConfigEdit::WebpQuality(v))
                    })
                    .into(),
                ));
            }
            controls.into()
        }

        FormatConfig::Avif(cfg) => {
            let mut controls = column![
                checkbox(cfg.lossless)
                    .label("lossless")
                    .on_toggle(move |v| edit(ConfigEdit::AvifLossless(v)))
                    .size(13)
                    .text_size(11),
            ]
            .spacing(4);
            if !cfg.lossless {
                controls = controls.push(labelled(
                    &format!("quality {}", cfg.quality),
                    slider(0..=100u8, cfg.quality, move |v| {
                        edit(ConfigEdit::AvifQuality(v))
                    })
                    .into(),
                ));
            }
            controls.into()
        }

        FormatConfig::Jxl(cfg) => {
            let mut controls = column![
                checkbox(cfg.lossless)
                    .label("lossless")
                    .on_toggle(move |v| edit(ConfigEdit::JxlLossless(v)))
                    .size(13)
                    .text_size(11),
            ]
            .spacing(4);
            if !cfg.lossless {
                controls = controls.push(labelled(
                    &format!("distance {:.2}", cfg.distance),
                    slider(0.1..=25.0f32, cfg.distance, move |v| {
                        edit(ConfigEdit::JxlDistance(v))
                    })
                    .step(0.1)
                    .into(),
                ));
            }
            controls = controls.push(labelled(
                &format!("effort {}", cfg.effort),
                slider(1..=10u8, cfg.effort, move |v| {
                    edit(ConfigEdit::JxlEffort(v))
                })
                .into(),
            ));
            controls = controls.push(
                checkbox(cfg.use_container)
                    .label("force ISO BMFF container")
                    .on_toggle(move |v| edit(ConfigEdit::JxlContainer(v)))
                    .size(13)
                    .text_size(11),
            );
            controls.into()
        }
    }
}

/// Every PNG filter strategy, including each fixed filter type, so the panel
/// exposes the whole enum rather than only its simple arms.
const PNG_FILTERS: &[(PngFilterStrategy, &str)] = &[
    (PngFilterStrategy::None, "none (fastest)"),
    (PngFilterStrategy::Fixed(PngFilterType::None), "fixed: none"),
    (PngFilterStrategy::Fixed(PngFilterType::Sub), "fixed: sub"),
    (PngFilterStrategy::Fixed(PngFilterType::Up), "fixed: up"),
    (
        PngFilterStrategy::Fixed(PngFilterType::Average),
        "fixed: average",
    ),
    (
        PngFilterStrategy::Fixed(PngFilterType::Paeth),
        "fixed: paeth",
    ),
    (PngFilterStrategy::MinSumAbs, "min sum of absolutes"),
    (PngFilterStrategy::BruteForce, "brute force (smallest)"),
];

const PNG_COMPRESSION: &[(PngCompressionLevel, &str)] = &[
    (PngCompressionLevel::Store, "store (uncompressed)"),
    (PngCompressionLevel::Fast, "fast"),
    (PngCompressionLevel::Default, "default"),
    (PngCompressionLevel::Best, "best (zopfli)"),
];

const JPEG_SUBSAMPLING: &[(JpegSubsampling, &str)] = &[
    (JpegSubsampling::Yuv420, "4:2:0"),
    (JpegSubsampling::Yuv422, "4:2:2"),
    (JpegSubsampling::Yuv444, "4:4:4"),
];

/// Output bit depths. Ten and Twelve are offered even though several encoders
/// reject them — the resulting `UnsupportedBitDepth` error appearing in that
/// format's placeholder is exactly the kind of encoder boundary this gallery
/// exists to make visible.
const BIT_DEPTHS: &[(BitDepth, &str)] = &[
    (BitDepth::Eight, "8-bit"),
    (BitDepth::Ten, "10-bit"),
    (BitDepth::Twelve, "12-bit"),
    (BitDepth::Sixteen, "16-bit"),
];

fn labelled<'a>(label: &str, control: Element<'a, Message>) -> Element<'a, Message> {
    column![text(label.to_owned()).size(11), control]
        .spacing(2)
        .into()
}

fn apply_edit(config: &mut FormatConfig, edit: ConfigEdit) {
    match (config, edit) {
        (FormatConfig::Png(cfg), ConfigEdit::PngCompression(v)) => cfg.compression = v,
        (FormatConfig::Png(cfg), ConfigEdit::PngFilter(v)) => cfg.filter = v,
        (FormatConfig::Png(cfg), ConfigEdit::PngAutoReduce(v)) => cfg.auto_reduce = v,
        (FormatConfig::Jpeg(cfg), ConfigEdit::JpegQuality(v)) => cfg.quality = v,
        (FormatConfig::Jpeg(cfg), ConfigEdit::JpegSubsampling(v)) => cfg.subsampling = v,
        (FormatConfig::Jpeg(cfg), ConfigEdit::JpegProgressive(v)) => cfg.progressive = v,
        (FormatConfig::Jpeg(cfg), ConfigEdit::JpegRestart(v)) => cfg.restart_interval = v,
        (FormatConfig::WebP(cfg), ConfigEdit::WebpMode(v)) => cfg.mode = v,
        (FormatConfig::WebP(cfg), ConfigEdit::WebpQuality(v)) => cfg.quality = v,
        (FormatConfig::Avif(cfg), ConfigEdit::AvifLossless(v)) => cfg.lossless = v,
        (FormatConfig::Avif(cfg), ConfigEdit::AvifQuality(v)) => cfg.quality = v,
        (FormatConfig::Jxl(cfg), ConfigEdit::JxlLossless(v)) => cfg.lossless = v,
        (FormatConfig::Jxl(cfg), ConfigEdit::JxlDistance(v)) => cfg.distance = v,
        (FormatConfig::Jxl(cfg), ConfigEdit::JxlEffort(v)) => cfg.effort = v,
        (FormatConfig::Jxl(cfg), ConfigEdit::JxlContainer(v)) => cfg.use_container = v,
        // A control can only be built from its own config variant, so a
        // mismatch would mean the panel and the model disagreed.
        (config, edit) => {
            tracing::warn!(?edit, "edit did not match {:?}", config.format());
        }
    }
}

/// Decode one source and round-trip it through every selected format.
///
/// Runs on a blocking worker; returns everything the UI needs so the update
/// handler does no image work of its own.
fn process(
    path: PathBuf,
    configs: &[FormatConfig],
    raw: RawSettings,
) -> Result<(Source, Vec<Variant>), (PathBuf, String)> {
    let source = pipeline::load_source(&path, raw).map_err(|e| (path.clone(), e))?;
    let variants = configs
        .iter()
        .map(|config| pipeline::round_trip(&source, config))
        .collect();
    Ok((source, variants))
}

/// The native file picker.
async fn pick_files() -> Vec<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Choose images")
        .pick_files()
        .await
        .map(|files| files.into_iter().map(|f| f.path().to_path_buf()).collect())
        .unwrap_or_default()
}

fn human_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KIB {
        format!("{bytes:.0} B")
    } else if bytes < KIB * KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{:.1} MiB", bytes / (KIB * KIB))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rawshift_image::formats::export::{JpegEncodeConfig, PngEncodeConfig};

    #[test]
    fn edits_reach_their_own_format() {
        let mut config = FormatConfig::Jpeg(JpegEncodeConfig::default());
        apply_edit(&mut config, ConfigEdit::JpegQuality(42));
        let FormatConfig::Jpeg(cfg) = &config else {
            panic!("variant changed");
        };
        assert_eq!(cfg.quality, 42);
    }

    #[test]
    fn a_mismatched_edit_leaves_the_config_untouched() {
        let mut config = FormatConfig::Png(PngEncodeConfig::default());
        let before = config.clone();
        apply_edit(&mut config, ConfigEdit::JpegQuality(1));
        assert_eq!(config, before);
    }

    #[test]
    fn png_filter_list_covers_every_fixed_filter() {
        // Five fixed filters plus None, MinSumAbs, BruteForce.
        assert_eq!(PNG_FILTERS.len(), 8);
        for filter in [
            PngFilterType::None,
            PngFilterType::Sub,
            PngFilterType::Up,
            PngFilterType::Average,
            PngFilterType::Paeth,
        ] {
            assert!(
                PNG_FILTERS
                    .iter()
                    .any(|(v, _)| *v == PngFilterStrategy::Fixed(filter)),
                "{filter:?} is not offered in the settings panel"
            );
        }
    }

    #[test]
    fn toggling_a_format_expands_it() {
        let mut state = State::default();
        state.settings.formats[3].enabled = false;
        state.settings.formats[3].expanded = false;
        let _ = state.update(Message::ToggleFormat(3, true));
        assert!(state.settings.formats[3].enabled);
        assert!(state.settings.formats[3].expanded);
    }

    #[test]
    fn shared_settings_reach_the_model() {
        let mut state = State::default();
        let _ = state.update(Message::BitDepthChanged(BitDepth::Eight));
        let _ = state.update(Message::EmbedIcc(false));
        assert_eq!(state.settings.common.bit_depth, BitDepth::Eight);
        assert!(!state.settings.common.metadata.embed_icc);
    }

    #[test]
    fn clear_empties_every_collection() {
        let mut state = State::default();
        state.rejected.push((PathBuf::from("a"), "bad".into()));
        state.queue.push(PathBuf::from("b"));
        let _ = state.update(Message::Clear);
        assert!(state.rejected.is_empty());
        assert!(state.queue.is_empty());
        assert!(state.rows.is_empty());
    }
}
