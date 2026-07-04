use smithay::backend::renderer::element::{Element, Id, Kind, RenderElement, UnderlyingStorage};
use smithay::backend::renderer::gles::{
    GlesError, GlesFrame, GlesRenderer, GlesTexProgram, GlesTexture, Uniform,
};
use smithay::backend::renderer::utils::{CommitCounter, DamageSet, OpaqueRegions};
use smithay::utils::user_data::UserDataMap;
use smithay::utils::{Buffer, Physical, Rectangle, Scale, Size, Transform};

use super::color::{primary_conversion, sanitize_mat3, Primaries};
use super::renderer::AsGlesFrame as _;
use super::shaders::{mat3_uniform, Shaders};
use super::texture::{TextureBuffer, TextureRenderElement};
use crate::backend::tty::{TtyFrame, TtyRenderer, TtyRendererError};

#[derive(Debug)]
pub struct HdrOutputRenderElement {
    inner: TextureRenderElement<GlesTexture>,
    id: Id,
    commit: CommitCounter,
    damage: Vec<Rectangle<i32, Physical>>,
    program: GlesTexProgram,
    uniforms: [Uniform<'static>; 3],
}

impl HdrOutputRenderElement {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        renderer: &mut GlesRenderer,
        texture: GlesTexture,
        size: Size<i32, Physical>,
        output_scale: f64,
        sdr_reference_luminance: f64,
        target_peak_luminance: f64,
        id: Id,
        commit: CommitCounter,
        damage: Vec<Rectangle<i32, Physical>>,
    ) -> Option<Self> {
        let program = Shaders::get(renderer).hdr_output.clone()?;
        let buffer = TextureBuffer::from_texture(
            renderer,
            texture,
            output_scale,
            Transform::Normal,
            vec![Rectangle::from_size(Size::<i32, Buffer>::from((
                size.w, size.h,
            )))],
        );
        let inner = TextureRenderElement::from_texture_buffer(
            buffer,
            (0., 0.),
            1.,
            None,
            Some(size.to_f64().to_logical(output_scale)),
            Kind::Unspecified,
        );
        let gamut = sanitize_mat3(primary_conversion(Primaries::BT709, Primaries::BT2020));
        Some(Self {
            inner,
            id,
            commit,
            damage,
            program,
            uniforms: [
                Uniform::new("sdr_reference_luminance", sdr_reference_luminance as f32),
                Uniform::new("target_peak_luminance", target_peak_luminance as f32),
                mat3_uniform("gamut_matrix", gamut).into_owned(),
            ],
        })
    }
}

impl Element for HdrOutputRenderElement {
    fn id(&self) -> &Id {
        &self.id
    }
    fn current_commit(&self) -> CommitCounter {
        self.commit
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
        _scale: Scale<f64>,
        commit: Option<CommitCounter>,
    ) -> DamageSet<i32, Physical> {
        if commit == Some(self.commit) {
            DamageSet::default()
        } else {
            DamageSet::from_slice(&self.damage)
        }
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

impl RenderElement<GlesRenderer> for HdrOutputRenderElement {
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

impl<'render> RenderElement<TtyRenderer<'render>> for HdrOutputRenderElement {
    fn draw(
        &self,
        frame: &mut TtyFrame<'render, '_, '_>,
        src: Rectangle<f64, Buffer>,
        dst: Rectangle<i32, Physical>,
        damage: &[Rectangle<i32, Physical>],
        opaque: &[Rectangle<i32, Physical>],
        cache: Option<&UserDataMap>,
    ) -> Result<(), TtyRendererError<'render>> {
        let frame = frame.as_gles_frame();
        RenderElement::<GlesRenderer>::draw(self, frame, src, dst, damage, opaque, cache)?;
        Ok(())
    }
    fn underlying_storage(
        &self,
        _renderer: &mut TtyRenderer<'render>,
    ) -> Option<UnderlyingStorage<'_>> {
        None
    }
}
