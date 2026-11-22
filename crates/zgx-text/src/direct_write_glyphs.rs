//! Glyphs drawn by DirectWrite (Windows), as the system draws its own text,
//! Chromium its pages' and GPUI (`gpui_windows::direct_write`) its views':
//! each glyph's outline at the size and quarter-pixel offset its cache key
//! names, in the rendering mode the face recommends (Segoe UI's hinting,
//! symmetric smoothing for faces without any), as a coverage mask for the
//! atlas. Its coverage is the plain coverage DirectWrite's own blending
//! starts from; `draw.wgsl` corrects it for the text's colour with the
//! system's gamma and contrast (`text_contrast`), as Direct2D does.
//!
//! Faces with colour glyphs (COLR, CBDT, sbix, SVG) stay swash's, whose
//! colour outlines the atlas takes as they are.
use cosmic_text::{CacheKey, CacheKeyFlags, FontSystem, Placement, SwashContent, SwashImage};
use std::{cell::RefCell, collections::HashMap};
use windows::Win32::Graphics::DirectWrite::*;
use windows::core::Interface;

/// The factory and the loader its in-memory font files come from.
struct Writer {
    factory: IDWriteFactory6,
    loader: IDWriteInMemoryFontFileLoader,
}

/// A face at one weight and optical size.
type FaceKey = (cosmic_text::fontdb::ID, u16, u16, bool);

#[derive(Default)]
struct Cache {
    /// Each face's font resource (None: DirectWrite can't read it).
    resources: HashMap<cosmic_text::fontdb::ID, Option<IDWriteFontResource>>,
    /// Faces made from them at a weight, optical size and slant.
    faces: HashMap<FaceKey, Option<IDWriteFontFace5>>,
}

thread_local! {
    static WRITER: Option<Writer> = writer();
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

fn writer() -> Option<Writer> {
    // SAFETY: the factory and loader are COM objects owned here; the
    // loader is registered once with the factory that made it.
    unsafe {
        let factory: IDWriteFactory6 = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED).ok()?;
        let loader = factory.CreateInMemoryFontFileLoader().ok()?;
        factory.RegisterFontFileLoader(&loader).ok()?;
        Some(Writer { factory, loader })
    }
}

/// Whether the system draws glyphs here: a DirectWrite factory came up.
pub fn available() -> bool {
    WRITER.with(Option::is_some)
}

/// The system's text gamma and enhanced contrast for grayscale text, as
/// `IDWriteRenderingParams1` gives them (the ClearType Text Tuner's), for
/// the renderer's blending: (1.8, 1.0) unless the user tuned them.
pub fn rendering_params() -> Option<(f32, f32)> {
    WRITER.with(|writer| {
        let writer = writer.as_ref()?;
        // SAFETY: the parameters are a COM object owned here.
        unsafe {
            let params: IDWriteRenderingParams1 = writer.factory.CreateRenderingParams().ok()?.cast().ok()?;
            Some((params.GetGamma(), params.GetGrayscaleEnhancedContrast()))
        }
    })
}

/// Whether face `font` has colour glyphs, which swash draws.
fn colour(font: &cosmic_text::Font) -> bool {
    let face = font.as_swash();
    [*b"COLR", *b"CBDT", *b"sbix", *b"SVG "]
        .into_iter()
        .any(|tag| face.table(u32::from_be_bytes(tag)).is_some())
}

/// `key`'s glyph through DirectWrite: None for a colour face, or where
/// DirectWrite can't read the face or draw the glyph (swash draws it then).
pub(crate) fn glyph_image(fonts: &mut FontSystem, key: CacheKey) -> Option<SwashImage> {
    if key.flags.contains(CacheKeyFlags::PIXEL_FONT) {
        return None;
    }
    let font = fonts.get_font(key.font_id, key.font_weight)?;
    if colour(&font) {
        return None;
    }
    let face = WRITER.with(|writer| {
        let writer = writer.as_ref()?;
        CACHE.with(|cache| face_for(&mut cache.borrow_mut(), writer, fonts, key))
    })?;
    WRITER.with(|writer| draw(writer.as_ref()?, &face, key))
}

fn face_for(cache: &mut Cache, writer: &Writer, fonts: &mut FontSystem, key: CacheKey) -> Option<IDWriteFontFace5> {
    let italic = key.flags.contains(CacheKeyFlags::FAKE_ITALIC);
    let slot = (key.font_id, key.font_weight.0, key.font_optical, italic);
    if let Some(face) = cache.faces.get(&slot) {
        return face.clone();
    }
    let resource = match cache.resources.get(&key.font_id) {
        Some(resource) => resource.clone(),
        None => {
            let made = resource_for(writer, fonts, key.font_id);
            cache.resources.insert(key.font_id, made.clone());
            made
        }
    };
    let face = resource.and_then(|resource| {
        // SAFETY: the resource is owned here; the axis values outlive the call.
        unsafe {
            let count = resource.GetFontAxisCount() as usize;
            let mut axes = vec![DWRITE_FONT_AXIS_VALUE::default(); count];
            if count > 0 {
                resource.GetDefaultFontAxisValues(&mut axes).ok()?;
            }
            for axis in &mut axes {
                if axis.axisTag == DWRITE_FONT_AXIS_TAG_WEIGHT {
                    axis.value = f32::from(key.font_weight.0);
                } else if axis.axisTag == DWRITE_FONT_AXIS_TAG_OPTICAL_SIZE && key.font_optical > 0 {
                    axis.value = f32::from(key.font_optical);
                }
            }
            let simulations = if italic {
                DWRITE_FONT_SIMULATIONS_OBLIQUE
            } else {
                DWRITE_FONT_SIMULATIONS_NONE
            };
            resource.CreateFontFace(simulations, &axes).ok()
        }
    });
    cache.faces.insert(slot, face.clone());
    face
}

/// Face `id`'s font resource, from its bytes (copied by DirectWrite).
fn resource_for(writer: &Writer, fonts: &mut FontSystem, id: cosmic_text::fontdb::ID) -> Option<IDWriteFontResource> {
    let index = fonts.db().face(id)?.index;
    let font = fonts.get_font(id, cosmic_text::fontdb::Weight::NORMAL)?;
    let data = font.data();
    // SAFETY: with no owner object, DirectWrite copies the bytes before
    // returning, so they need not outlive the call.
    unsafe {
        let file = writer
            .loader
            .CreateInMemoryFontFileReference(
                &writer.factory,
                data.as_ptr().cast(),
                u32::try_from(data.len()).ok()?,
                None,
            )
            .ok()?;
        writer.factory.CreateFontResource(&file, index).ok()
    }
}

/// `key`'s glyph of `face`, as a coverage mask placed from its pen position.
fn draw(writer: &Writer, face: &IDWriteFontFace5, key: CacheKey) -> Option<SwashImage> {
    let size = f32::from_bits(key.font_size_bits);
    let glyph = u16::try_from(key.glyph_id).ok()?;
    let advance = 0f32;
    let offset = DWRITE_GLYPH_OFFSET::default();
    let face0: IDWriteFontFace = face.cast().ok()?;
    let run = DWRITE_GLYPH_RUN {
        fontFace: std::mem::ManuallyDrop::new(Some(face0)),
        fontEmSize: size,
        glyphCount: 1,
        glyphIndices: &glyph,
        glyphAdvances: &advance,
        glyphOffsets: &offset,
        isSideways: false.into(),
        bidiLevel: 0,
    };
    // SAFETY: the run borrows locals that outlive each call; its face is
    // released below, the run holding it in a `ManuallyDrop`.
    let image = unsafe {
        let mut mode = DWRITE_RENDERING_MODE1::default();
        let mut grid = DWRITE_GRID_FIT_MODE::default();
        let recommended = face.GetRecommendedRenderingMode(
            size,
            96.,
            96.,
            None,
            false,
            DWRITE_OUTLINE_THRESHOLD_ANTIALIASED,
            DWRITE_MEASURING_MODE_NATURAL,
            None,
            &mut mode,
            &mut grid,
        );
        if recommended.is_err() || mode == DWRITE_RENDERING_MODE1_OUTLINE || mode == DWRITE_RENDERING_MODE1_ALIASED {
            mode = DWRITE_RENDERING_MODE1_NATURAL_SYMMETRIC;
        }
        let analysis = writer.factory.CreateGlyphRunAnalysis(
            &run,
            None,
            mode,
            DWRITE_MEASURING_MODE_NATURAL,
            grid,
            DWRITE_TEXT_ANTIALIAS_MODE_GRAYSCALE,
            key.x_bin.as_float(),
            key.y_bin.as_float(),
        );
        analysis.ok().and_then(|analysis| mask(&analysis))
    };
    drop(std::mem::ManuallyDrop::into_inner(run.fontFace));
    image
}

/// The analysis' grayscale coverage, swash's way up: `top` above the
/// baseline, rows from the top.
///
/// # Safety
/// `analysis` is a live glyph run analysis.
unsafe fn mask(analysis: &IDWriteGlyphRunAnalysis) -> Option<SwashImage> {
    // SAFETY: the bounds and the texture are read into memory owned here.
    unsafe {
        let bounds = analysis.GetAlphaTextureBounds(DWRITE_TEXTURE_ALIASED_1x1).ok()?;
        let width = u32::try_from(bounds.right - bounds.left).unwrap_or(0);
        let height = u32::try_from(bounds.bottom - bounds.top).unwrap_or(0);
        let mut data = vec![0u8; width as usize * height as usize];
        if !data.is_empty() {
            analysis.CreateAlphaTexture(DWRITE_TEXTURE_ALIASED_1x1, &bounds, &mut data).ok()?;
        }
        let mut image = SwashImage::new();
        image.content = SwashContent::Mask;
        image.placement = Placement {
            left: bounds.left,
            top: -bounds.top,
            width,
            height,
        };
        image.data = data;
        Some(image)
    }
}

#[cfg(test)]
mod tests {
    use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};

    #[test]
    fn a_glyph_sits_where_swash_puts_it_with_as_much_ink() {
        if !super::available() {
            return;
        }
        let mut fonts = FontSystem::new();
        let mut swash = SwashCache::new();
        let mut buffer = Buffer::new(&mut fonts, Metrics::new(13., 18.));
        buffer.set_text("Hamburgefonstiv", &Attrs::new().family(Family::SansSerif), Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut fonts, false);
        let mut checked = 0;
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let key = glyph.physical((0., 0.), 1.).cache_key;
                let (Some(ours), Some(theirs)) = (
                    super::glyph_image(&mut fonts, key),
                    swash.get_image_uncached(&mut fonts, key),
                ) else {
                    continue;
                };
                if theirs.placement.width == 0 {
                    continue;
                }
                let ink = |data: &[u8]| data.iter().map(|&a| u32::from(a)).sum::<u32>() as f32;
                let (a, b) = (ink(&ours.data), ink(&theirs.data));
                assert!((ours.placement.left - theirs.placement.left).abs() <= 2, "left {:?} {:?}", ours.placement, theirs.placement);
                assert!((ours.placement.top - theirs.placement.top).abs() <= 2, "top {:?} {:?}", ours.placement, theirs.placement);
                assert!((a / b - 1.).abs() < 0.35, "ink {a} against {b}");
                checked += 1;
            }
        }
        assert!(checked > 10);
    }
}
