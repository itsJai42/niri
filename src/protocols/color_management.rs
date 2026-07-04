//! Niri-local implementation of the staging `color-management-v1` protocol.
//!
//! First HDR slice: parametric image descriptions built from named primaries and
//! named transfer functions, plus the predefined Windows-scRGB description.
//! Descriptions are backed by [`crate::render_helpers::color::ImageDescription`].
//!
//! Bound at interface version 1. The version-2 additions (64-bit identity
//! `ready2`/`preferred_changed2`, `get_image_description`/reference objects,
//! `absolute_no_adaptation` intent, `compound_power_2_4` transfer function) are
//! deferred; bump `VERSION` and add the events when a client needs them.
//!
//! Per-surface image description is double-buffered via smithay's `Cacheable`
//! surface cached state, so it only takes effect on `wl_surface.commit` with no
//! changes to niri's commit path. Read the committed value with
//! [`committed_image_description`].

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use smithay::output::{Output, WeakOutput};
use smithay::reexports::wayland_protocols::wp::color_management::v1::server::{
    wp_color_management_output_v1, wp_color_management_surface_feedback_v1,
    wp_color_management_surface_v1, wp_color_manager_v1, wp_image_description_creator_params_v1,
    wp_image_description_info_v1, wp_image_description_v1,
};
use smithay::reexports::wayland_server::backend::{ClientId, GlobalId};
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum, Weak,
};
use smithay::wayland::compositor::SurfaceData;
use wp_color_management_output_v1::WpColorManagementOutputV1;
use wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1;
use wp_color_management_surface_v1::WpColorManagementSurfaceV1;
use wp_color_manager_v1::WpColorManagerV1;
use wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1;
use wp_image_description_info_v1::WpImageDescriptionInfoV1;
use wp_image_description_v1::WpImageDescriptionV1;

use crate::render_helpers::color::{
    ImageDescription, Primaries, RenderingIntent, TransferFunction,
};

const VERSION: u32 = 1;

/// Rendering intents niri advertises. Only perceptual is implemented (and it is
/// the one the protocol makes mandatory).
const SUPPORTED_INTENTS: &[wp_color_manager_v1::RenderIntent] =
    &[wp_color_manager_v1::RenderIntent::Perceptual];

/// Features niri advertises. Parametric descriptions, per-volume luminances, and
/// the Windows-scRGB shortcut. ICC, arbitrary primaries, power curves, and
/// mastering-display target volumes are deferred.
const SUPPORTED_FEATURES: &[wp_color_manager_v1::Feature] = &[
    wp_color_manager_v1::Feature::Parametric,
    wp_color_manager_v1::Feature::SetLuminances,
    wp_color_manager_v1::Feature::WindowsScrgb,
];

/// Named transfer functions niri can build a description for.
const SUPPORTED_TF: &[wp_color_manager_v1::TransferFunction] = &[
    wp_color_manager_v1::TransferFunction::Srgb,
    wp_color_manager_v1::TransferFunction::St2084Pq,
    wp_color_manager_v1::TransferFunction::ExtLinear,
];

/// Named primaries niri can build a description for.
const SUPPORTED_PRIMARIES: &[wp_color_manager_v1::Primaries] = &[
    wp_color_manager_v1::Primaries::Srgb,
    wp_color_manager_v1::Primaries::Bt2020,
];

/// Monotonic, never-recycled image-description identity numbers (id 0 is
/// reserved as invalid by the protocol).
static NEXT_IDENTITY: AtomicU32 = AtomicU32::new(1);

fn next_identity() -> u32 {
    NEXT_IDENTITY.fetch_add(1, Ordering::Relaxed)
}

/// Map a protocol named transfer function to the internal color model.
fn map_tf(tf: wp_color_manager_v1::TransferFunction) -> Option<TransferFunction> {
    match tf {
        wp_color_manager_v1::TransferFunction::Srgb => Some(TransferFunction::Srgb),
        wp_color_manager_v1::TransferFunction::St2084Pq => Some(TransferFunction::St2084Pq),
        wp_color_manager_v1::TransferFunction::ExtLinear => Some(TransferFunction::Linear),
        _ => None,
    }
}

/// Map protocol named primaries to the internal color model.
fn map_primaries(p: wp_color_manager_v1::Primaries) -> Option<Primaries> {
    match p {
        wp_color_manager_v1::Primaries::Srgb => Some(Primaries::BT709),
        wp_color_manager_v1::Primaries::Bt2020 => Some(Primaries::BT2020),
        _ => None,
    }
}

fn map_intent(intent: wp_color_manager_v1::RenderIntent) -> Option<RenderingIntent> {
    match intent {
        wp_color_manager_v1::RenderIntent::Perceptual => Some(RenderingIntent::Perceptual),
        wp_color_manager_v1::RenderIntent::Relative => Some(RenderingIntent::RelativeColorimetric),
        wp_color_manager_v1::RenderIntent::Saturation => Some(RenderingIntent::Saturation),
        wp_color_manager_v1::RenderIntent::Absolute => Some(RenderingIntent::AbsoluteColorimetric),
        _ => None,
    }
}

/// Accumulated parametric parameters. Each required property is set exactly once.
#[derive(Debug, Default)]
struct ParamsBuilder {
    tf: Option<TransferFunction>,
    primaries: Option<Primaries>,
    luminances: Option<(f64, f64, f64)>, // (min, max, reference) cd/m²
    max_cll: Option<f64>,
    max_fall: Option<f64>,
}

/// Why a parametric description could not be produced.
#[derive(Debug, PartialEq, Eq)]
enum BuildError {
    /// A protocol error must be raised (client killed).
    Protocol(wp_image_description_creator_params_v1::Error),
    /// Graceful failure: deliver `failed(unsupported)` on the description.
    Unsupported,
}

impl ParamsBuilder {
    /// Build an immutable image description from the accumulated parameters.
    fn build(&self) -> Result<ImageDescription, BuildError> {
        let (Some(transfer), Some(primaries)) = (self.tf, self.primaries) else {
            return Err(BuildError::Protocol(
                wp_image_description_creator_params_v1::Error::IncompleteSet,
            ));
        };

        if let (Some(cll), Some(fall)) = (self.max_cll, self.max_fall) {
            if fall > cll {
                return Err(BuildError::Protocol(
                    wp_image_description_creator_params_v1::Error::InvalidLuminance,
                ));
            }
        }

        let mut desc = ImageDescription {
            primaries,
            transfer,
            min_luminance: 0.0,
            max_luminance: 0.0,
            reference_white: 0.0,
            luminance_scale: 0.0,
            mastering: None,
            max_cll: self.max_cll,
            max_fall: self.max_fall,
            rendering_intent: RenderingIntent::Perceptual,
        };

        // Protocol defaults. Only PQ implies different values; parametric
        // ext_linear is not the predefined Windows-scRGB description.
        (desc.min_luminance, desc.max_luminance, desc.reference_white) = match transfer {
            TransferFunction::St2084Pq => (0.005, 10000.0, 203.0),
            TransferFunction::Linear | TransferFunction::Srgb => (0.2, 80.0, 80.0),
        };
        desc.luminance_scale = match transfer {
            TransferFunction::St2084Pq => 10000.0,
            TransferFunction::Linear | TransferFunction::Srgb => desc.reference_white,
        };

        if let Some((min, max, reference)) = self.luminances {
            desc.min_luminance = min;
            // ST 2084 fixes the swing at 10000 cd/m² regardless of max_lum.
            desc.max_luminance = if transfer == TransferFunction::St2084Pq {
                min + 10000.0
            } else {
                max
            };
            desc.reference_white = reference;
            if transfer != TransferFunction::St2084Pq {
                desc.luminance_scale = reference;
            }
        }

        if !desc.min_luminance.is_finite()
            || !desc.max_luminance.is_finite()
            || !desc.reference_white.is_finite()
        {
            return Err(BuildError::Unsupported);
        }

        // Version 1 requires content-light levels inside the mastering range.
        for level in [self.max_cll, self.max_fall].into_iter().flatten() {
            if level <= desc.min_luminance || level > desc.max_luminance {
                return Err(BuildError::Protocol(
                    wp_image_description_creator_params_v1::Error::InvalidLuminance,
                ));
            }
        }

        Ok(desc)
    }
}

/// A description object that reached the `ready` state.
#[derive(Debug, Clone, Copy)]
pub struct ReadyImageDescription {
    pub description: ImageDescription,
    pub identity: u32,
    /// Whether `get_information` is permitted on this object.
    allow_info: bool,
}

/// User data of a `wp_image_description_v1`: `None` means the object failed and
/// can only be destroyed.
#[derive(Debug)]
pub struct ImageDescriptionData {
    result: Option<ReadyImageDescription>,
}

/// User data of a `wp_image_description_creator_params_v1`.
#[derive(Debug, Default)]
pub struct CreatorParamsData {
    builder: Mutex<ParamsBuilder>,
}

/// User data of a `wp_color_management_output_v1`.
#[derive(Debug)]
pub struct ColorOutputData {
    output: WeakOutput,
}

/// User data of a `wp_color_management_surface_v1` / `_feedback_v1`.
#[derive(Debug)]
pub struct ColorSurfaceData {
    surface: Weak<WlSurface>,
}

impl ColorSurfaceData {
    fn surface(&self) -> Option<WlSurface> {
        self.surface.upgrade().ok()
    }
}

/// Double-buffered per-surface color state. `None` = untagged (treated as sRGB).
#[derive(Debug, Clone, Copy, Default)]
pub struct ColorSurfaceCachedState {
    config: Option<SurfaceColorConfig>,
}

/// Committed color configuration of a surface.
#[derive(Debug, Clone, Copy)]
pub struct SurfaceColorConfig {
    pub description: ImageDescription,
    pub render_intent: RenderingIntent,
}

impl ColorSurfaceCachedState {
    /// The committed color configuration, if the surface is tagged.
    pub fn config(&self) -> Option<SurfaceColorConfig> {
        self.config
    }
}

impl smithay::wayland::compositor::Cacheable for ColorSurfaceCachedState {
    fn commit(&mut self, _dh: &DisplayHandle) -> Self {
        *self
    }

    fn merge_into(self, into: &mut Self, _dh: &DisplayHandle) {
        *into = self;
    }
}

/// Read the committed color configuration from already-locked surface state.
/// Returns `None` for untagged surfaces (which must be treated as sRGB).
pub fn committed_image_description(states: &SurfaceData) -> Option<SurfaceColorConfig> {
    states
        .cached_state
        .get::<ColorSurfaceCachedState>()
        .current()
        .config()
}

/// Marks that a `wp_color_management_surface_v1` currently exists for a surface,
/// so a second `get_surface` raises `surface_exists`.
#[derive(Debug, Default)]
struct SurfaceExistsMarker(std::sync::atomic::AtomicBool);

/// Handler the compositor implements to supply output/preferred descriptions.
pub trait ColorManagementHandler {
    fn color_management_state(&mut self) -> &mut ColorManagementManagerState;

    /// Image description a client should use for content shown on `output`.
    fn image_description_for_output(&mut self, _output: &Output) -> ImageDescription {
        ImageDescription::srgb()
    }

    /// Preferred image description hint for `surface`.
    fn preferred_image_description(&mut self, _surface: &WlSurface) -> ImageDescription {
        ImageDescription::srgb()
    }

    /// Send `done` on `info` after the current dispatch completes.
    ///
    /// `done` is a destructor event. Sending it inside the dispatch that created the
    /// object destroys the resource while wayland-backend still holds a pointer to its
    /// user data, which it writes to after the handler returns (use-after-free, segfault).
    /// Implementations must defer it, e.g. via `LoopHandle::insert_idle`.
    fn defer_info_done(&mut self, info: WpImageDescriptionInfoV1);
}

/// Delegate state for the `wp_color_manager_v1` global.
#[derive(Debug)]
pub struct ColorManagementManagerState {
    global: GlobalId,
}

impl ColorManagementManagerState {
    pub fn new<D>(display: &DisplayHandle) -> Self
    where
        D: GlobalDispatch<WpColorManagerV1, ()>,
        D: Dispatch<WpColorManagerV1, ()>,
        D: Dispatch<WpColorManagementOutputV1, ColorOutputData>,
        D: Dispatch<WpColorManagementSurfaceV1, ColorSurfaceData>,
        D: Dispatch<WpColorManagementSurfaceFeedbackV1, ColorSurfaceData>,
        D: Dispatch<WpImageDescriptionCreatorParamsV1, CreatorParamsData>,
        D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
        D: Dispatch<WpImageDescriptionInfoV1, ()>,
        D: ColorManagementHandler,
        D: 'static,
    {
        let global = display.create_global::<D, WpColorManagerV1, _>(VERSION, ());
        Self { global }
    }

    pub fn global(&self) -> GlobalId {
        self.global.clone()
    }
}

/// Create a `wp_image_description_v1` that immediately delivers `ready` or
/// `failed`, and return the object.
fn make_ready_description<D>(
    id: New<WpImageDescriptionV1>,
    data_init: &mut DataInit<'_, D>,
    description: ImageDescription,
    allow_info: bool,
) -> WpImageDescriptionV1
where
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData> + 'static,
{
    let identity = next_identity();
    let obj = data_init.init(
        id,
        ImageDescriptionData {
            result: Some(ReadyImageDescription {
                description,
                identity,
                allow_info,
            }),
        },
    );
    obj.ready(identity);
    obj
}

fn make_failed_description<D>(
    id: New<WpImageDescriptionV1>,
    data_init: &mut DataInit<'_, D>,
    cause: wp_image_description_v1::Cause,
    msg: &str,
) -> WpImageDescriptionV1
where
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData> + 'static,
{
    let obj = data_init.init(id, ImageDescriptionData { result: None });
    obj.failed(cause, msg.to_owned());
    obj
}

impl<D> GlobalDispatch<WpColorManagerV1, (), D> for ColorManagementManagerState
where
    D: GlobalDispatch<WpColorManagerV1, ()>,
    D: Dispatch<WpColorManagerV1, ()>,
    D: Dispatch<WpColorManagementOutputV1, ColorOutputData>,
    D: Dispatch<WpColorManagementSurfaceV1, ColorSurfaceData>,
    D: Dispatch<WpColorManagementSurfaceFeedbackV1, ColorSurfaceData>,
    D: Dispatch<WpImageDescriptionCreatorParamsV1, CreatorParamsData>,
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: Dispatch<WpImageDescriptionInfoV1, ()>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn bind(
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<WpColorManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, D>,
    ) {
        let manager = data_init.init(manager, ());
        for &intent in SUPPORTED_INTENTS {
            manager.supported_intent(intent);
        }
        for &feature in SUPPORTED_FEATURES {
            manager.supported_feature(feature);
        }
        for &tf in SUPPORTED_TF {
            manager.supported_tf_named(tf);
        }
        for &primaries in SUPPORTED_PRIMARIES {
            manager.supported_primaries_named(primaries);
        }
        manager.done();
    }
}

impl<D> Dispatch<WpColorManagerV1, (), D> for ColorManagementManagerState
where
    D: Dispatch<WpColorManagerV1, ()>,
    D: Dispatch<WpColorManagementOutputV1, ColorOutputData>,
    D: Dispatch<WpColorManagementSurfaceV1, ColorSurfaceData>,
    D: Dispatch<WpColorManagementSurfaceFeedbackV1, ColorSurfaceData>,
    D: Dispatch<WpImageDescriptionCreatorParamsV1, CreatorParamsData>,
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: Dispatch<WpImageDescriptionInfoV1, ()>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        manager: &WpColorManagerV1,
        request: <WpColorManagerV1 as Resource>::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use wp_color_manager_v1::Request;
        match request {
            Request::GetOutput { id, output } => {
                let output = Output::from_resource(&output).map(|output| output.downgrade());
                data_init.init(
                    id,
                    ColorOutputData {
                        output: output.unwrap_or_default(),
                    },
                );
            }
            Request::GetSurface { id, surface } => {
                let already = smithay::wayland::compositor::with_states(&surface, |states| {
                    states
                        .data_map
                        .insert_if_missing_threadsafe(SurfaceExistsMarker::default);
                    let marker = states.data_map.get::<SurfaceExistsMarker>().unwrap();
                    marker.0.swap(true, Ordering::AcqRel)
                });
                if already {
                    manager.post_error(
                        wp_color_manager_v1::Error::SurfaceExists,
                        "a color management surface already exists for this wl_surface",
                    );
                    return;
                }
                data_init.init(
                    id,
                    ColorSurfaceData {
                        surface: surface.downgrade(),
                    },
                );
            }
            Request::GetSurfaceFeedback { id, surface } => {
                data_init.init(
                    id,
                    ColorSurfaceData {
                        surface: surface.downgrade(),
                    },
                );
            }
            Request::CreateParametricCreator { obj } => {
                data_init.init(obj, CreatorParamsData::default());
            }
            Request::CreateWindowsScrgb { image_description } => {
                // get_information is not allowed on Windows-scRGB descriptions.
                make_ready_description(
                    image_description,
                    data_init,
                    ImageDescription::windows_scrgb(),
                    false,
                );
            }
            Request::CreateIccCreator { obj } => {
                // ICC feature is not advertised.
                manager.post_error(
                    wp_color_manager_v1::Error::UnsupportedFeature,
                    "ICC image descriptions are not supported",
                );
                // Still init so the id is consumed if the client survives; it
                // won't, post_error terminates the client.
                let _ = obj;
            }
            Request::Destroy => (),
            _ => (),
        }
    }
}

impl<D> Dispatch<WpImageDescriptionCreatorParamsV1, CreatorParamsData, D>
    for ColorManagementManagerState
where
    D: Dispatch<WpImageDescriptionCreatorParamsV1, CreatorParamsData>,
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        creator: &WpImageDescriptionCreatorParamsV1,
        request: <WpImageDescriptionCreatorParamsV1 as Resource>::Request,
        data: &CreatorParamsData,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use wp_image_description_creator_params_v1::{Error, Request};
        match request {
            Request::SetTfNamed { tf } => {
                let mut b = data.builder.lock().unwrap();
                if b.tf.is_some() {
                    creator.post_error(Error::AlreadySet, "transfer function already set");
                    return;
                }
                match to_enum(tf).and_then(map_tf) {
                    Some(tf) => b.tf = Some(tf),
                    None => creator.post_error(Error::InvalidTf, "unsupported transfer function"),
                }
            }
            Request::SetPrimariesNamed { primaries } => {
                let mut b = data.builder.lock().unwrap();
                if b.primaries.is_some() {
                    creator.post_error(Error::AlreadySet, "primaries already set");
                    return;
                }
                match to_enum(primaries).and_then(map_primaries) {
                    Some(p) => b.primaries = Some(p),
                    None => {
                        creator.post_error(Error::InvalidPrimariesNamed, "unsupported primaries")
                    }
                }
            }
            Request::SetLuminances {
                min_lum,
                max_lum,
                reference_lum,
            } => {
                let mut b = data.builder.lock().unwrap();
                if b.luminances.is_some() {
                    creator.post_error(Error::AlreadySet, "luminances already set");
                    return;
                }
                let min = min_lum as f64 / 10000.0;
                let max = max_lum as f64;
                let reference = reference_lum as f64;
                if max <= min || reference <= min {
                    creator.post_error(Error::InvalidLuminance, "invalid luminance range");
                    return;
                }
                b.luminances = Some((min, max, reference));
            }
            Request::SetMaxCll { max_cll } => {
                data.builder.lock().unwrap().max_cll = Some(max_cll as f64);
            }
            Request::SetMaxFall { max_fall } => {
                data.builder.lock().unwrap().max_fall = Some(max_fall as f64);
            }
            Request::SetTfPower { .. } => {
                creator.post_error(
                    Error::UnsupportedFeature,
                    "power transfer functions unsupported",
                );
            }
            Request::SetPrimaries { .. } => {
                creator.post_error(Error::UnsupportedFeature, "custom primaries unsupported");
            }
            Request::SetMasteringDisplayPrimaries { .. }
            | Request::SetMasteringLuminance { .. } => {
                creator.post_error(
                    Error::UnsupportedFeature,
                    "mastering display target volume unsupported",
                );
            }
            Request::Create { image_description } => {
                let result = data.builder.lock().unwrap().build();
                match result {
                    Ok(desc) => {
                        make_ready_description(image_description, data_init, desc, false);
                    }
                    Err(BuildError::Protocol(err)) => {
                        creator.post_error(err, "invalid parametric image description");
                    }
                    Err(BuildError::Unsupported) => {
                        make_failed_description(
                            image_description,
                            data_init,
                            wp_image_description_v1::Cause::Unsupported,
                            "unsupported image description parameters",
                        );
                    }
                }
            }
            _ => (),
        }
    }
}

impl<D> Dispatch<WpImageDescriptionV1, ImageDescriptionData, D> for ColorManagementManagerState
where
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: Dispatch<WpImageDescriptionInfoV1, ()>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        obj: &WpImageDescriptionV1,
        request: <WpImageDescriptionV1 as Resource>::Request,
        data: &ImageDescriptionData,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use wp_image_description_v1::{Error, Request};
        match request {
            Request::GetInformation { information } => {
                let Some(ready) = &data.result else {
                    obj.post_error(Error::NotReady, "image description is not ready");
                    return;
                };
                if !ready.allow_info {
                    obj.post_error(
                        Error::NoInformation,
                        "get_information is not allowed on this image description",
                    );
                    return;
                }
                let info = data_init.init(information, ());
                send_information(&info, &ready.description);
                state.defer_info_done(info);
            }
            Request::Destroy => (),
            _ => (),
        }
    }
}

impl<D> Dispatch<WpImageDescriptionInfoV1, (), D> for ColorManagementManagerState
where
    D: Dispatch<WpImageDescriptionInfoV1, ()>,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        _obj: &WpImageDescriptionInfoV1,
        _request: <WpImageDescriptionInfoV1 as Resource>::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        // wp_image_description_info_v1 has no requests.
    }
}

fn xy_to_arg(v: f64) -> i32 {
    (v * 1_000_000.0).round() as i32
}

fn send_information(info: &WpImageDescriptionInfoV1, desc: &ImageDescription) {
    let p = desc.primaries;
    info.primaries(
        xy_to_arg(p.red[0]),
        xy_to_arg(p.red[1]),
        xy_to_arg(p.green[0]),
        xy_to_arg(p.green[1]),
        xy_to_arg(p.blue[0]),
        xy_to_arg(p.blue[1]),
        xy_to_arg(p.white[0]),
        xy_to_arg(p.white[1]),
    );
    if p == Primaries::BT709 {
        info.primaries_named(wp_color_manager_v1::Primaries::Srgb);
    } else if p == Primaries::BT2020 {
        info.primaries_named(wp_color_manager_v1::Primaries::Bt2020);
    }
    let tf = match desc.transfer {
        TransferFunction::Srgb => wp_color_manager_v1::TransferFunction::Srgb,
        TransferFunction::St2084Pq => wp_color_manager_v1::TransferFunction::St2084Pq,
        TransferFunction::Linear => wp_color_manager_v1::TransferFunction::ExtLinear,
    };
    info.tf_named(tf);
    info.luminances(
        (desc.min_luminance * 10000.0).round() as u32,
        desc.max_luminance.round() as u32,
        desc.reference_white.round() as u32,
    );
    info.target_luminance(
        (desc.min_luminance * 10000.0).round() as u32,
        desc.max_luminance.round() as u32,
    );
    // `done` is deliberately NOT sent here: it is a destructor event and must be
    // deferred out of the creating dispatch (see ColorManagementHandler::defer_info_done).
}

impl<D> Dispatch<WpColorManagementOutputV1, ColorOutputData, D> for ColorManagementManagerState
where
    D: Dispatch<WpColorManagementOutputV1, ColorOutputData>,
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        _obj: &WpColorManagementOutputV1,
        request: <WpColorManagementOutputV1 as Resource>::Request,
        data: &ColorOutputData,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use wp_color_management_output_v1::Request;
        match request {
            Request::GetImageDescription { image_description } => {
                match data.output.upgrade() {
                    Some(output) => {
                        let desc = state.image_description_for_output(&output);
                        make_ready_description(image_description, data_init, desc, true);
                    }
                    None => {
                        // Inert object: the output global is gone.
                        make_failed_description(
                            image_description,
                            data_init,
                            wp_image_description_v1::Cause::NoOutput,
                            "the output no longer exists",
                        );
                    }
                }
            }
            Request::Destroy => (),
            _ => (),
        }
    }
}

impl<D> Dispatch<WpColorManagementSurfaceV1, ColorSurfaceData, D> for ColorManagementManagerState
where
    D: Dispatch<WpColorManagementSurfaceV1, ColorSurfaceData>,
    D: 'static,
{
    fn request(
        _state: &mut D,
        _client: &Client,
        obj: &WpColorManagementSurfaceV1,
        request: <WpColorManagementSurfaceV1 as Resource>::Request,
        data: &ColorSurfaceData,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        use wp_color_management_surface_v1::{Error, Request};
        match request {
            Request::SetImageDescription {
                image_description,
                render_intent,
            } => {
                let Some(surface) = data.surface() else {
                    obj.post_error(Error::Inert, "the wl_surface no longer exists");
                    return;
                };
                let Some(intent) = to_enum(render_intent).and_then(map_intent) else {
                    obj.post_error(Error::RenderIntent, "unsupported rendering intent");
                    return;
                };
                if !SUPPORTED_INTENTS
                    .iter()
                    .any(|i| map_intent(*i) == Some(intent))
                {
                    obj.post_error(Error::RenderIntent, "unsupported rendering intent");
                    return;
                }
                let desc_data = image_description.data::<ImageDescriptionData>();
                let Some(ready) = desc_data.and_then(|d| d.result) else {
                    obj.post_error(Error::ImageDescription, "image description is not ready");
                    return;
                };
                let config = SurfaceColorConfig {
                    description: ready.description,
                    render_intent: intent,
                };
                set_pending_config(&surface, Some(config));
            }
            Request::UnsetImageDescription => {
                let Some(surface) = data.surface() else {
                    obj.post_error(Error::Inert, "the wl_surface no longer exists");
                    return;
                };
                set_pending_config(&surface, None);
            }
            Request::Destroy => {
                // Destroying acts like unset, and releases the surface_exists marker.
                if let Some(surface) = data.surface() {
                    set_pending_config(&surface, None);
                    clear_surface_marker(&surface);
                }
            }
            _ => (),
        }
    }

    fn destroyed(
        _state: &mut D,
        _client: ClientId,
        _obj: &WpColorManagementSurfaceV1,
        data: &ColorSurfaceData,
    ) {
        if let Some(surface) = data.surface() {
            clear_surface_marker(&surface);
        }
    }
}

fn set_pending_config(surface: &WlSurface, config: Option<SurfaceColorConfig>) {
    smithay::wayland::compositor::with_states(surface, |states| {
        states
            .cached_state
            .get::<ColorSurfaceCachedState>()
            .pending()
            .config = config;
    });
}

fn clear_surface_marker(surface: &WlSurface) {
    smithay::wayland::compositor::with_states(surface, |states| {
        if let Some(marker) = states.data_map.get::<SurfaceExistsMarker>() {
            marker.0.store(false, Ordering::Release);
        }
    });
}

impl<D> Dispatch<WpColorManagementSurfaceFeedbackV1, ColorSurfaceData, D>
    for ColorManagementManagerState
where
    D: Dispatch<WpColorManagementSurfaceFeedbackV1, ColorSurfaceData>,
    D: Dispatch<WpImageDescriptionV1, ImageDescriptionData>,
    D: ColorManagementHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        obj: &WpColorManagementSurfaceFeedbackV1,
        request: <WpColorManagementSurfaceFeedbackV1 as Resource>::Request,
        data: &ColorSurfaceData,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        use wp_color_management_surface_feedback_v1::{Error, Request};
        match request {
            Request::GetPreferred { image_description }
            | Request::GetPreferredParametric { image_description } => {
                let Some(surface) = data.surface() else {
                    obj.post_error(Error::Inert, "the wl_surface no longer exists");
                    return;
                };
                let desc = state.preferred_image_description(&surface);
                make_ready_description(image_description, data_init, desc, true);
            }
            Request::Destroy => (),
            _ => (),
        }
    }
}

/// Convert a `WEnum` request argument to its enum, discarding unknowns.
fn to_enum<T>(value: WEnum<T>) -> Option<T> {
    value.into_result().ok()
}

/// Delegate the color-management protocol dispatch to [`ColorManagementManagerState`].
#[macro_export]
macro_rules! delegate_color_management {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_color_manager_v1::WpColorManagerV1: ()
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_color_manager_v1::WpColorManagerV1: ()
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_color_management_output_v1::WpColorManagementOutputV1: $crate::protocols::color_management::ColorOutputData
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_color_management_surface_v1::WpColorManagementSurfaceV1: $crate::protocols::color_management::ColorSurfaceData
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1: $crate::protocols::color_management::ColorSurfaceData
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1: $crate::protocols::color_management::CreatorParamsData
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_image_description_v1::WpImageDescriptionV1: $crate::protocols::color_management::ImageDescriptionData
        ] => $crate::protocols::color_management::ColorManagementManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            smithay::reexports::wayland_protocols::wp::color_management::v1::server::wp_image_description_info_v1::WpImageDescriptionInfoV1: ()
        ] => $crate::protocols::color_management::ColorManagementManagerState);
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_are_backed_by_the_color_model() {
        // Every advertised named tf/primaries must map into the color model.
        for &tf in SUPPORTED_TF {
            assert!(map_tf(tf).is_some(), "advertised tf {tf:?} has no mapping");
        }
        for &p in SUPPORTED_PRIMARIES {
            assert!(
                map_primaries(p).is_some(),
                "advertised primaries {p:?} unmapped"
            );
        }
        for &i in SUPPORTED_INTENTS {
            assert!(map_intent(i).is_some(), "advertised intent {i:?} unmapped");
        }
        // Perceptual is mandatory.
        assert!(SUPPORTED_INTENTS.contains(&wp_color_manager_v1::RenderIntent::Perceptual));
    }

    #[test]
    fn identity_is_monotonic_and_never_zero() {
        let a = next_identity();
        let b = next_identity();
        assert_ne!(a, 0);
        assert!(b > a);
    }

    #[test]
    fn build_srgb_description() {
        let b = ParamsBuilder {
            tf: Some(TransferFunction::Srgb),
            primaries: Some(Primaries::BT709),
            ..Default::default()
        };
        let desc = b.build().unwrap();
        assert_eq!(desc.transfer, TransferFunction::Srgb);
        assert_eq!(desc.primaries, Primaries::BT709);
        assert_eq!(desc.min_luminance, 0.2);
        assert_eq!(desc.max_luminance, 80.0);
        assert_eq!(desc.reference_white, 80.0);
    }

    #[test]
    fn build_pq_bt2020_description() {
        let b = ParamsBuilder {
            tf: Some(TransferFunction::St2084Pq),
            primaries: Some(Primaries::BT2020),
            ..Default::default()
        };
        let desc = b.build().unwrap();
        assert_eq!(desc.transfer, TransferFunction::St2084Pq);
        assert_eq!(desc.primaries, Primaries::BT2020);
        assert_eq!(desc.min_luminance, 0.005);
        assert_eq!(desc.max_luminance, 10000.0);
        assert_eq!(desc.reference_white, 203.0);
    }

    #[test]
    fn build_requires_tf_and_primaries() {
        let only_tf = ParamsBuilder {
            tf: Some(TransferFunction::Srgb),
            ..Default::default()
        };
        assert_eq!(
            only_tf.build().unwrap_err(),
            BuildError::Protocol(wp_image_description_creator_params_v1::Error::IncompleteSet)
        );
    }

    #[test]
    fn build_rejects_fall_above_cll() {
        let b = ParamsBuilder {
            tf: Some(TransferFunction::St2084Pq),
            primaries: Some(Primaries::BT2020),
            max_cll: Some(1000.0),
            max_fall: Some(2000.0),
            ..Default::default()
        };
        assert_eq!(
            b.build().unwrap_err(),
            BuildError::Protocol(wp_image_description_creator_params_v1::Error::InvalidLuminance)
        );
    }

    #[test]
    fn version_one_rejects_content_light_outside_mastering_range() {
        for b in [
            ParamsBuilder {
                tf: Some(TransferFunction::Srgb),
                primaries: Some(Primaries::BT709),
                max_cll: Some(0.0),
                ..Default::default()
            },
            ParamsBuilder {
                tf: Some(TransferFunction::Srgb),
                primaries: Some(Primaries::BT709),
                max_fall: Some(81.0),
                ..Default::default()
            },
        ] {
            assert_eq!(
                b.build().unwrap_err(),
                BuildError::Protocol(
                    wp_image_description_creator_params_v1::Error::InvalidLuminance
                )
            );
        }
    }

    #[test]
    fn pq_luminance_swing_is_fixed_at_10000() {
        let b = ParamsBuilder {
            tf: Some(TransferFunction::St2084Pq),
            primaries: Some(Primaries::BT2020),
            luminances: Some((0.01, 500.0, 203.0)), // max_lum ignored for PQ
            ..Default::default()
        };
        let desc = b.build().unwrap();
        assert!((desc.max_luminance - (0.01 + 10000.0)).abs() < 1e-6);
        assert_eq!(desc.reference_white, 203.0);
    }

    #[test]
    fn windows_scrgb_is_linear() {
        let desc = ImageDescription::windows_scrgb();
        assert_eq!(desc.transfer, TransferFunction::Linear);
        assert_eq!(desc.reference_white, 203.0);
    }
}
