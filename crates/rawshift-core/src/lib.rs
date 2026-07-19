//! Shared core types for the rawshift image/video processing libraries.
//!
//! This crate holds pure, stateless data structures with no decoding logic:
//! geometry ([`image::Dimensions`], [`image::Point`], [`image::Rect`]), pixel
//! vocabulary (re-exported from `gamut-core`), CFA patterns, the raw sensor
//! container, and the format-agnostic [`metadata`] model.
//!
//! Generic image primitives come from [gamut](https://github.com/justin13888/gamut)
//! and are re-exported here rather than reimplemented (see the workspace
//! upstream-first policy): [`Dimensions`], the sealed [`Pixel`]/[`Sample`]
//! traits with their marker types ([`Rgb16`], [`Rgba8`], …), the [`ImageBuf`]/
//! [`ImageRef`] containers, [`BitDepth`], and the CICP code-point enums behind
//! [`ColorDescription`]. rawshift-core adds only what gamut deliberately does
//! not model: the sensor vocabulary and the metadata model.
//!
//! # Charter
//!
//! This crate exists to hold the vocabulary that **both** the image and video
//! libraries need, so neither has to depend on the other. That is its only
//! justification for existing: a type used by exactly one side belongs in that
//! side's crate, not here.
//!
//! Video is parked for v1 and `rawshift-video` currently depends on nothing, so
//! the split is forward-looking rather than load-bearing today. Recording it
//! now keeps the boundary from drifting into "everything shared-ish lives in
//! core". The division is:
//!
//! **Genuinely video-shared** — media-agnostic, and video will consume these
//! as-is:
//!
//! - Geometry — [`image::Dimensions`], [`image::Point`], [`image::Rect`].
//! - Codec descriptors — [`codec::CodecId`], [`codec::CodecInfo`],
//!   [`codec::CodecDirection`]. A codec registry spans stills and video.
//! - Metadata model — [`metadata::ImageMetadata`] and its
//!   [`metadata::MetadataEntry`]/[`metadata::MetadataKey`]/[`metadata::MetadataValue`]/[`metadata::MetadataNamespace`]
//!   parts, plus [`codec::MetadataEmbedOptions`]. EXIF/XMP semantics are the
//!   same for a video file as for a still.
//! - Rationals — [`metadata::URational`], [`metadata::SRational`]. These are the
//!   EXIF/TIFF wire representation, shared with the metadata model above.
//! - Color and bit depth descriptors ([`ColorDescription`], [`BitDepth`]),
//!   which describe a decoded frame regardless of whether it came from a still
//!   or a video track.
//!
//! **Stills-only** — present here for historical reasons, and not part of the
//! shared vocabulary. These describe a Bayer/X-Trans sensor mosaic, a concept
//! with no video analogue:
//!
//! - [`image::RawImage`] and [`image::RawImageBuilder`].
//! - [`image::CfaPattern`], [`image::XTransPattern`],
//!   [`image::white_level_from_bit_depth`].
#![forbid(unsafe_code)]

pub mod codec;
pub mod color;
pub mod image;
pub mod metadata;

pub use codec::{CodecDirection, CodecId, CodecInfo, MetadataEmbedOptions};
pub use color::{BitDepth, ColorDescription, ColourPrimaries, TransferCharacteristics};
pub use image::{
    CfaPattern, Dimensions, Point, RawImage, RawImageBuilder, Rect, XTransPattern,
    white_level_from_bit_depth,
};
pub use metadata::{
    ExtractMetadata, ImageMetadata, MetadataEntry, MetadataKey, MetadataNamespace, MetadataValue,
};

// Pixel vocabulary, re-exported from gamut-core. `Pixel` and `Sample` are
// sealed upstream: rawshift cannot (and should not) add pixel formats — an f32
// working format stays transform-internal in rawshift-image.
pub use gamut_core::{
    Bilevel, Cmyk8, ColorModel, Gray8, Gray16, GrayAlpha8, GrayAlpha16, ImageBuf, ImageRef,
    Indexed8, Pixel, PixelFormat, Rgb8, Rgb16, Rgba8, Rgba16, Sample,
};
