//! Everything related to text, fonts, text layout, cursors etc.

pub mod cursor;
mod emoji;
mod font;
mod fonts;
mod index;
mod text_layout;
mod text_layout_types;

pub use {
    fonts::{
        FontData, FontDefinitions, FontFamily, FontId, FontInsert, FontPriority, FontTweak,
        FontVariationAxis, Fonts, FontsImpl, FontsView, HintingTarget, InsertFontFamily,
        SmoothHinting,
    },
    index::{ByteIndex, ByteRange, ByteRangeExt, CharIndex, CharRange, CharRangeExt},
    text_layout::*,
    text_layout_types::*,
};

/// Suggested character to use to replace those in password text fields.
pub const PASSWORD_REPLACEMENT_CHAR: char = '•';

/// Controls how we render text
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub struct TextOptions {
    /// Maximum size of the font texture.
    pub max_texture_side: usize,

    /// Controls how to convert glyph colors when writing to the font atlas.
    pub color_transfer_function: crate::FontColorTransferFunction,

    /// Whether to enable font hinting
    ///
    /// (round some font coordinates to pixels for sharper text).
    ///
    /// Default is `true`.
    pub font_hinting: bool,

    /// Enable sub-pixel binning for glyphs.
    ///
    /// Sub-pixel binning renders each glyph at up to four fractional horizontal offsets,
    /// giving more even kerning at the cost of more atlas space.
    ///
    /// It also lead to text looking more blurry.
    ///
    /// This is always disabled for CJK characters (which have too many unique glyphs).
    ///
    /// Can be overridden per font with [`FontTweak::subpixel_binning`].
    ///
    /// Default: `true`.
    pub subpixel_binning: bool,

    /// How glyph outlines become pixel coverage.
    ///
    /// Default: [`GlyphRasterization::Symmetric`].
    pub glyph_rasterization: GlyphRasterization,
}

impl Default for TextOptions {
    fn default() -> Self {
        Self {
            max_texture_side: 2048, // Small but portable
            color_transfer_function: crate::FontColorTransferFunction::default(),
            font_hinting: true,
            subpixel_binning: true,
            glyph_rasterization: GlyphRasterization::default(),
        }
    }
}

/// How glyph outlines become pixel coverage, see [`TextOptions::glyph_rasterization`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
pub enum GlyphRasterization {
    /// Antialiased in both directions: a pixel's coverage is the share of its area the
    /// glyph covers.
    #[default]
    Symmetric,

    /// Antialiased horizontally only, as `DirectWrite`'s natural rendering mode draws text on
    /// Windows (Chromium uses it for fonts whose `gasp` table does not ask for symmetric
    /// smoothing): each pixel row samples the outline once, along its middle, so horizontal
    /// edges and the tops and bottoms of round letters fall on whole rows, and each row's
    /// coverage is spread over the neighbouring sub-pixels by the `ClearType` filter and averaged
    /// to gray. Text looks crisper, with the same weight.
    HorizontalOnly,
}
