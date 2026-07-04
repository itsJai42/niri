use smithay::backend::renderer::element::surface::WaylandSurfaceRenderElement;
use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, Uniform,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Physical, Rectangle, Scale, Transform};

use super::color::{primary_conversion, sanitize_mat3, ImageDescription, TransferFunction};
use super::renderer::{AsGlesFrame as _, NiriRenderer};
use super::shaders::{mat3_uniform, Shaders};
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};

#[derive(Debug)]
pub struct ColorSurfaceRenderElement<R: NiriRenderer> {
    inner: WaylandSurfaceRenderElement<R>,
    program: GlesTexProgram,
    uniforms: [Uniform<'static>; 5],
}

impl<R: NiriRenderer> ColorSurfaceRenderElement<R> {
    pub fn new(
        renderer: &mut R,
        inner: WaylandSurfaceRenderElement<R>,
        source: ImageDescription,
        sdr_reference_luminance: f64,
        tone_map_to_sdr: bool,
    ) -> Option<Self> {
        let program = Shaders::get(renderer).color_transform.clone()?;
        let source_tf = match source.transfer {
            TransferFunction::Linear => 0.0,
            TransferFunction::Srgb => 1.0,
            TransferFunction::St2084Pq => 2.0,
        };
        let gamut = sanitize_mat3(primary_conversion(
            source.primaries,
            super::color::Primaries::BT709,
        ));
        let uniforms = [
            Uniform::new("source_tf", source_tf),
            Uniform::new(
                "luminance_scale",
                (source.luminance_scale / sdr_reference_luminance) as f32,
            ),
            mat3_uniform("gamut_matrix", gamut).into_owned(),
            Uniform::new("output_sdr", f32::from(tone_map_to_sdr)),
            Uniform::new(
                "source_peak_ratio",
                (source.max_luminance / sdr_reference_luminance).max(1.0) as f32,
            ),
        ];
        Some(Self {
            inner,
            program,
            uniforms,
        })
    }

    pub fn inner(&self) -> &WaylandSurfaceRenderElement<R> {
        &self.inner
    }

    pub fn into_parts(self) -> (WaylandSurfaceRenderElement<R>, Vec<Uniform<'static>>) {
        (self.inner, self.uniforms.to_vec())
    }
}

impl<R: NiriRenderer> Element for ColorSurfaceRenderElement<R> {
    fn id(&self) -> &Id {
        self.inner.id()
    }
    fn current_commit(&self) -> CommitCounter {
        self.inner.current_commit()
    }
    fn geometry(&self, scale: Scale<f64>) -> Rectangle<i32, Physical> {
        self.inner.geometry(scale)
    }
    fn transform(&self) -> Transform {
        self.inner.transform()
    }
    fn src(&self) -> Rectangle<f64, Buffer> {
        self.inner.src()
    }
    fn damage_since(
        &self,
        scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        self.inner.damage_since(scale, commit)
    }
    fn opaque_regions(&self, scale: Scale<f64>) -> OpaqueRegions<i32, Physical> {
        self.inner.opaque_regions(scale)
    }
    fn alpha(&self) -> f32 {
        self.inner.alpha()
    }
    fn kind(&self) -> Kind {
        self.inner.kind()
    }
}

impl RenderElement<GlesRenderer> for ColorSurfaceRenderElement<GlesRenderer> {
    fn draw(
        &self,
        frame: &mut GlesFrame<'_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), GlesError> {
        frame.override_default_tex_program(self.program.clone(), self.uniforms.to_vec());
        let result = RenderElement::<GlesRenderer>::draw(
            &self.inner,
            frame,
            src,
            dst,
            damage,
            opaque,
            cache,
        );
        frame.clear_tex_program_override();
        result
    }

    fn underlying_storage(&self, _renderer: &mut GlesRenderer) -> Option<UnderlyingStorage<'_>> {
        None
    }
}

impl<'render> RenderElement<TtyRenderer<'render>>
    for ColorSurfaceRenderElement<TtyRenderer<'render>>
{
    fn draw(
        &self,
        frame: &mut TtyFrame<'render, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        frame
            .as_gles_frame()
            .override_default_tex_program(self.program.clone(), self.uniforms.to_vec());
        let result = RenderElement::draw(&self.inner, frame, src, dst, damage, opaque, cache);
        frame.as_gles_frame().clear_tex_program_override();
        result
    }

    fn underlying_storage(
        &self,
        _renderer: &mut TtyRenderer<'render>,
    ) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
