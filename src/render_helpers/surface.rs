use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::Kind;
use smithay::backend::renderer::gles::{GlesRenderer, GlesTexture};
use smithay::backend::renderer::utils::{import_surface, RendererSurfaceStateUserData};
use smithay::backend::renderer::Renderer;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::utils::{Logical, Physical, Point, Scale};
use smithay::wayland::compositor::{with_surface_tree_downward, TraversalAction};

use super::color::ImageDescription;
use super::color_surface::ColorSurfaceRenderElement;
use super::renderer::NiriRenderer;
use super::texture::TextureBuffer;
use crate::protocols::color_management::committed_image_description;

crate::niri_render_elements! {
    SurfaceRenderElement<R> => {
        Wayland = WaylandSurfaceRenderElement<R>,
        Color = ColorSurfaceRenderElement<R>,
    }
}

impl<R: NiriRenderer> SurfaceRenderElement<R> {
    pub fn wayland(&self) -> &WaylandSurfaceRenderElement<R> {
        match self {
            Self::Wayland(elem) => elem,
            Self::Color(elem) => elem.inner(),
        }
    }

    pub fn is_color_managed(&self) -> bool {
        matches!(self, Self::Color(_))
    }

    pub fn into_wayland_and_color_uniforms(
        self,
    ) -> (
        WaylandSurfaceRenderElement<R>,
        Option<Vec<smithay::backend::renderer::gles::Uniform<'static>>>,
    ) {
        match self {
            Self::Wayland(elem) => (elem, None),
            Self::Color(elem) => {
                let (elem, uniforms) = elem.into_parts();
                (elem, Some(uniforms))
            }
        }
    }
}
use super::BakedBuffer;

/// Renders elements from a surface tree as textures into `storage`.
pub fn render_snapshot_from_surface_tree(
    renderer: &mut GlesRenderer,
    surface: &WlSurface,
    location: Point<f64, Logical>,
    storage: &mut Vec<BakedBuffer<TextureBuffer<GlesTexture>>>,
) {
    let _span = tracy_client::span!("render_snapshot_from_surface_tree");

    with_surface_tree_downward(
        surface,
        location,
        |_, states, location| {
            let mut location = *location;
            let data = states.data_map.get::<RendererSurfaceStateUserData>();

            if let Some(data) = data {
                let data = &*data.lock().unwrap();

                if let Some(view) = data.view() {
                    location += view.offset.to_f64();
                    TraversalAction::DoChildren(location)
                } else {
                    TraversalAction::SkipChildren
                }
            } else {
                TraversalAction::SkipChildren
            }
        },
        |_, states, location| {
            let mut location = *location;
            let data = states.data_map.get::<RendererSurfaceStateUserData>();

            if let Some(data) = data {
                let Some(view) = data.lock().unwrap().view() else {
                    return;
                };
                location += view.offset.to_f64();

                if let Err(err) = import_surface(renderer, states) {
                    warn!("failed to import surface: {err:?}");
                    return;
                }

                let data = data.lock().unwrap();
                let Some(texture) = data.texture(renderer.context_id()) else {
                    return;
                };

                let buffer = TextureBuffer::from_texture(
                    renderer,
                    texture.clone(),
                    f64::from(data.buffer_scale()),
                    data.buffer_transform(),
                    Vec::new(),
                );

                let baked = BakedBuffer {
                    buffer,
                    location,
                    src: Some(view.src),
                    dst: Some(view.dst),
                };

                storage.push(baked);
            }
        },
        |_, _, _| true,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn push_elements_from_surface_tree<R>(
    renderer: &mut R,
    surface: &WlSurface,
    // Fractional scale expects surface buffers to be aligned to physical pixels.
    location: Point<i32, Physical>,
    scale: Scale<f64>,
    alpha: f32,
    kind: Kind,
    color_managed: bool,
    tone_map_to_sdr: bool,
    sdr_reference_luminance: f64,
    push: &mut dyn FnMut(SurfaceRenderElement<R>),
) where
    R: NiriRenderer,
    R::TextureId: Clone + 'static,
{
    let _span = tracy_client::span!("push_elements_from_surface_tree");

    let location = location.to_f64();

    with_surface_tree_downward(
        surface,
        location,
        |_, states, location| {
            let mut location = *location;
            let data = states.data_map.get::<RendererSurfaceStateUserData>();

            if let Some(data) = data {
                if let Some(view) = data.lock().unwrap().view() {
                    location += view.offset.to_f64().to_physical(scale);
                    TraversalAction::DoChildren(location)
                } else {
                    TraversalAction::SkipChildren
                }
            } else {
                TraversalAction::SkipChildren
            }
        },
        |surface, states, location| {
            let mut location = *location;
            let data = states.data_map.get::<RendererSurfaceStateUserData>();

            if let Some(data) = data {
                let has_view = if let Some(view) = data.lock().unwrap().view() {
                    location += view.offset.to_f64().to_physical(scale);
                    true
                } else {
                    false
                };

                if has_view {
                    match WaylandSurfaceRenderElement::from_surface(
                        renderer, surface, states, location, alpha, kind,
                    ) {
                        Ok(Some(elem)) => {
                            let committed = committed_image_description(states);
                            if color_managed && (!tone_map_to_sdr || committed.is_some()) {
                                let source = committed
                                    .map(|config| config.description)
                                    .unwrap_or_else(|| {
                                        let mut source = ImageDescription::srgb();
                                        source.luminance_scale = sdr_reference_luminance;
                                        source.reference_white = sdr_reference_luminance;
                                        source
                                    });
                                if let Some(elem) = ColorSurfaceRenderElement::new(
                                    renderer,
                                    elem,
                                    source,
                                    sdr_reference_luminance,
                                    tone_map_to_sdr,
                                ) {
                                    push(elem.into());
                                }
                            } else {
                                push(elem.into());
                            }
                        }
                        Ok(None) => {} // surface is not mapped
                        Err(err) => {
                            warn!("failed to import surface: {}", err);
                        }
                    };
                }
            }
        },
        |_, _, _| true,
    );
}
