//! Real GLES import checks for HDR client buffer formats.

use smithay::backend::allocator::Fourcc;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::{ImportMem, Texture};
use smithay::utils::{Buffer, Physical, Scale, Size, Transform};

use super::fixture::Fixture;
use crate::hdr::HDR_CLIENT_DMABUF_FORMATS;
use crate::render_helpers::render_to_texture;
use crate::render_helpers::shaders::Shaders;
use crate::render_helpers::solid_color::{SolidColorBuffer, SolidColorRenderElement};

/// Verifies the GLES renderer can upload each HDR client format via
/// `import_memory` (the shm/mem path). Dmabuf import is validated on real HDR
/// hardware (Phase 6): the headless backend does not implement dmabuf import.
#[test]
fn hdr_client_formats_import_as_memory() {
    let mut f = Fixture::new();
    f.niri_state().backend.headless().add_renderer().unwrap();

    let size = Size::<i32, Buffer>::from((2, 2));
    // 8 bytes/px covers the widest format (ABGR16161616F); the 10-bit formats
    // need only 4 bytes/px, so this buffer is large enough for all four.
    let data = [0u8; 2 * 2 * 8];

    f.niri_state()
        .backend
        .headless()
        .with_primary_renderer(|renderer| {
            for fourcc in HDR_CLIENT_DMABUF_FORMATS {
                renderer
                    .import_memory(&data, fourcc, size, false)
                    .unwrap_or_else(|e| panic!("import_memory({fourcc:?}) failed: {e}"));
            }
        })
        .unwrap();
}

#[test]
fn fp16_composition_target_renders() {
    let mut f = Fixture::new();
    f.niri_state().backend.headless().add_renderer().unwrap();

    f.niri_state()
        .backend
        .headless()
        .with_primary_renderer(|renderer| {
            let buffer = SolidColorBuffer::new((2.0, 2.0), [2.0, 0.5, 0.25, 1.0]);
            let element =
                SolidColorRenderElement::from_buffer(&buffer, (0.0, 0.0), 1.0, Kind::Unspecified);
            let size = Size::<i32, Physical>::from((2, 2));
            let (texture, sync) = render_to_texture(
                renderer,
                size,
                Scale::from(1.0),
                Transform::Normal,
                Fourcc::Abgr16161616f,
                std::iter::once(element),
            )
            .unwrap();

            sync.wait().unwrap();
            assert_eq!(texture.size(), Size::from((2, 2)));
            assert_eq!(texture.format(), Some(Fourcc::Abgr16161616f));
        })
        .unwrap();
}

#[test]
fn hdr_shaders_compile() {
    let mut f = Fixture::new();
    f.niri_state().backend.headless().add_renderer().unwrap();
    f.niri_state()
        .backend
        .headless()
        .with_primary_renderer(|renderer| {
            let shaders = Shaders::get(renderer);
            assert!(shaders.color_transform.is_some());
            assert!(shaders.color_clipped_surface.is_some());
            assert!(shaders.hdr_output.is_some());
        })
        .unwrap();
}
