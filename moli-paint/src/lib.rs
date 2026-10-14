//! PDF output and CPU rasterization for owned Moli paint snapshots.
//!
//! The implementation is intentionally a narrow adaptation of the AnyRender
//! screenshot path used by Blitz at commit
//! `d788124ab881f9bb537cb452ec1d837604a374a8`. The referenced Blitz and
//! AnyRender code is licensed under MIT OR Apache-2.0. No Blitz DOM or retained
//! layout state is copied into this crate.

pub mod pdf;
mod raster;

pub use moli_image::RgbaImage as RasterImage;
pub use raster::{
    CanvasClip, MAX_PAINT_GLYPHS, MAX_PAINT_LAYER_DEPTH, MAX_PAINT_OPERATIONS,
    MAX_TRANSIENT_RASTER_BYTES, PaintError, raster_canvas_clip_mask, raster_snapshot,
};
