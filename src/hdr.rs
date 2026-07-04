//! HDR client buffer format support.
//!
//! Single source of truth for the client buffer formats niri accepts for HDR
//! content, plus the runtime intersection with what the renderer can import.
//!
//! Advertisement: dmabuf formats are advertised to clients automatically from
//! `renderer.dmabuf_formats()` via `DmabufFeedbackBuilder` (see `backend::tty`
//! and `backend::winit`). niri does not filter that set, so every format below
//! is offered to clients whenever the EGL driver reports it. No separate
//! advertisement path is required for dmabuf HDR formats.
//!
//! wl_shm: HDR shm formats are intentionally not advertised in the first HDR
//! slice. Smithay's GLES `import_memory` does support these fourccs on GL ES 3,
//! but per the HDR plan the first slice is dmabuf-only; shm FP16/10-bit upload
//! is deferred. `ShmState` therefore stays 8-bit (see `Niri::new` in `niri.rs`).
//!
//! Live per-format dmabuf import depends on GPU and driver and is validated on
//! real HDR hardware (Phase 0 forced-HDR path and Phase 6), not in the
//! GPU-free unit-test suite. Use [`importable_hdr_dmabuf_formats`] at runtime
//! to gate against the renderer's actual capabilities.

use smithay::backend::allocator::format::FormatSet;
use smithay::backend::allocator::Fourcc;

/// Client dmabuf buffer formats niri accepts for HDR content.
///
/// - `Abgr16161616f` / `Xbgr16161616f`: FP16, for Windows-scRGB clients.
/// - `Abgr2101010` / `Xbgr2101010`: 10-bit, for PQ/BT.2020 clients.
///
/// YUV formats (e.g. P010) are out of scope until `color-representation-v1`.
///
/// Note: per the color-management protocol an image description stays valid on
/// any buffer format (a PQ description on an 8-bit buffer is legal, just
/// banded). This list gates advertised formats and import paths, not which
/// image descriptions are accepted.
pub const HDR_CLIENT_DMABUF_FORMATS: [Fourcc; 4] = [
    Fourcc::Abgr16161616f,
    Fourcc::Xbgr16161616f,
    Fourcc::Abgr2101010,
    Fourcc::Xbgr2101010,
];

/// Returns the formats from [`HDR_CLIENT_DMABUF_FORMATS`] that the renderer can
/// actually import, given its reported dmabuf format set (any modifier).
///
/// Use this to gate accepted/advertised HDR formats to what the current GPU and
/// driver support instead of assuming every desired format is importable.
pub fn importable_hdr_dmabuf_formats(available: &FormatSet) -> Vec<Fourcc> {
    HDR_CLIENT_DMABUF_FORMATS
        .into_iter()
        .filter(|fourcc| available.iter().any(|f| f.code == *fourcc))
        .collect()
}

#[cfg(test)]
mod tests {
    use smithay::backend::allocator::Format;
    use smithay::reexports::gbm::Modifier;

    use super::*;

    #[test]
    fn intersects_with_renderer_formats() {
        // Renderer reports one supported HDR format plus one unrelated format;
        // only the supported HDR format survives the intersection.
        let available = FormatSet::from_iter([
            Format {
                code: Fourcc::Abgr16161616f,
                modifier: Modifier::Linear,
            },
            Format {
                code: Fourcc::Abgr8888,
                modifier: Modifier::Linear,
            },
        ]);
        assert_eq!(
            importable_hdr_dmabuf_formats(&available),
            vec![Fourcc::Abgr16161616f]
        );

        // No HDR formats reported -> empty.
        let sdr_only = FormatSet::from_iter([Format {
            code: Fourcc::Abgr8888,
            modifier: Modifier::Linear,
        }]);
        assert!(importable_hdr_dmabuf_formats(&sdr_only).is_empty());
    }
}
