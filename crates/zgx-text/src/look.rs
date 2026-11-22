//! How text looks where the system's rasterizer draws it (Windows): as the
//! system's own text does (`TextLook::System`, Direct2D's blending), or as
//! Chromium draws its pages' (`TextLook::Chromium`, Skia's). Chosen once,
//! before the first window shapes text: glyphs already in an atlas keep the
//! look they were drawn in.
use std::sync::atomic::{AtomicU8, Ordering};

/// How text looks where the system draws its glyphs (Windows).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextLook {
    /// As the system draws its own text: DirectWrite's recommended
    /// rendering mode, Direct2D's gamma and contrast (Windows Terminal's
    /// and GPUI's), which add contrast to dark text only.
    #[default]
    System,
    /// As Chromium draws a page's text: the rendering mode Skia picks from
    /// a font's `gasp` table, grid fitting on, ClearType's coverage averaged
    /// where text is grayscale, and Skia's gamma (sRGB, contrast 1), which
    /// draws light text on a dark ground heavier than the system's.
    Chromium,
}

/// Unset: `ZGX_TEXT_LOOK` (`system` or `chromium`) decides at first read.
const UNSET: u8 = u8::MAX;

static LOOK: AtomicU8 = AtomicU8::new(UNSET);

/// Choose how text looks, before the first window shapes text.
pub fn set_text_look(look: TextLook) {
    LOOK.store(look as u8, Ordering::Relaxed);
}

/// How text looks: what `set_text_look` chose, else `ZGX_TEXT_LOOK`
/// (`chromium` or `system`), else `TextLook::System`.
pub fn text_look() -> TextLook {
    match LOOK.load(Ordering::Relaxed) {
        UNSET => {
            let look = match zgx::env::var_os("ZGX_TEXT_LOOK") {
                Some(value) if value.eq_ignore_ascii_case("chromium") => TextLook::Chromium,
                _ => TextLook::System,
            };
            let _ = LOOK.compare_exchange(UNSET, look as u8, Ordering::Relaxed, Ordering::Relaxed);
            from_byte(LOOK.load(Ordering::Relaxed))
        }
        byte => from_byte(byte),
    }
}

fn from_byte(byte: u8) -> TextLook {
    if byte == TextLook::Chromium as u8 {
        TextLook::Chromium
    } else {
        TextLook::System
    }
}
