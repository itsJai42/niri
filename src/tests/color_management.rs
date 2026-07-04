//! Live round-trip test for the color-management-v1 protocol (Phase 2 gate).
//!
//! Binds the global from a real client, creates sRGB, PQ/BT.2020 and
//! Windows-scRGB descriptions and asserts they reach `ready`, and asserts the
//! `incomplete_set` protocol error for a parametric creator missing primaries.

use std::sync::{Arc, Mutex};

use smithay::reexports::wayland_protocols::wp::color_management::v1::client::{
    wp_color_management_output_v1::WpColorManagementOutputV1,
    wp_color_management_surface_feedback_v1::WpColorManagementSurfaceFeedbackV1,
    wp_color_management_surface_v1::WpColorManagementSurfaceV1,
    wp_color_manager_v1::{
        self, Feature, Primaries, RenderIntent, TransferFunction, WpColorManagerV1,
    },
    wp_image_description_creator_params_v1::WpImageDescriptionCreatorParamsV1,
    wp_image_description_info_v1::{self, WpImageDescriptionInfoV1},
    wp_image_description_v1::{self, WpImageDescriptionV1},
};
use wayland_client::{Dispatch, QueueHandle, WEnum};

use super::client::State;
use super::fixture::Fixture;

/// Compositor capabilities collected from the manager's initial events.
#[derive(Debug, Default)]
pub struct ColorCaps {
    pub intents: Vec<RenderIntent>,
    pub features: Vec<Feature>,
    pub tf: Vec<TransferFunction>,
    pub primaries: Vec<Primaries>,
    pub done: bool,
}

/// Readiness of a created image description, tracked via the object's user data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DescStatus {
    Pending,
    Ready,
    Failed,
}

impl Dispatch<WpColorManagerV1, ()> for State {
    fn event(
        state: &mut Self,
        _proxy: &WpColorManagerV1,
        event: <WpColorManagerV1 as wayland_client::Proxy>::Event,
        _data: &(),
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wp_color_manager_v1::Event;
        match event {
            Event::SupportedIntent { render_intent } => {
                if let WEnum::Value(v) = render_intent {
                    state.color_caps.intents.push(v);
                }
            }
            Event::SupportedFeature { feature } => {
                if let WEnum::Value(v) = feature {
                    state.color_caps.features.push(v);
                }
            }
            Event::SupportedTfNamed { tf } => {
                if let WEnum::Value(v) = tf {
                    state.color_caps.tf.push(v);
                }
            }
            Event::SupportedPrimariesNamed { primaries } => {
                if let WEnum::Value(v) = primaries {
                    state.color_caps.primaries.push(v);
                }
            }
            Event::Done => state.color_caps.done = true,
            _ => {}
        }
    }
}

impl Dispatch<WpImageDescriptionV1, Arc<Mutex<DescStatus>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpImageDescriptionV1,
        event: <WpImageDescriptionV1 as wayland_client::Proxy>::Event,
        data: &Arc<Mutex<DescStatus>>,
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        use wp_image_description_v1::Event;
        match event {
            Event::Ready { .. } | Event::Ready2 { .. } => *data.lock().unwrap() = DescStatus::Ready,
            Event::Failed { .. } => *data.lock().unwrap() = DescStatus::Failed,
            _ => {}
        }
    }
}

wayland_client::delegate_noop!(State: ignore WpImageDescriptionCreatorParamsV1);
wayland_client::delegate_noop!(State: ignore WpColorManagementSurfaceV1);
wayland_client::delegate_noop!(State: ignore WpColorManagementSurfaceFeedbackV1);
wayland_client::delegate_noop!(State: ignore WpColorManagementOutputV1);
impl Dispatch<WpImageDescriptionInfoV1, Arc<Mutex<bool>>> for State {
    fn event(
        _state: &mut Self,
        _proxy: &WpImageDescriptionInfoV1,
        event: <WpImageDescriptionInfoV1 as wayland_client::Proxy>::Event,
        data: &Arc<Mutex<bool>>,
        _conn: &wayland_client::Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wp_image_description_info_v1::Event::Done = event {
            *data.lock().unwrap() = true;
        }
    }
}

fn make_parametric(
    mgr: &WpColorManagerV1,
    qh: &QueueHandle<State>,
    tf: TransferFunction,
    primaries: Primaries,
) -> Arc<Mutex<DescStatus>> {
    let creator = mgr.create_parametric_creator(qh, ());
    creator.set_tf_named(tf);
    creator.set_primaries_named(primaries);
    let status = Arc::new(Mutex::new(DescStatus::Pending));
    creator.create(qh, status.clone());
    status
}

#[test]
fn advertises_caps_and_creates_named_descriptions() {
    let mut f = Fixture::new();
    let id = f.add_client();
    f.roundtrip(id);

    // Capabilities advertised on bind.
    {
        let caps = &f.client(id).state.color_caps;
        assert!(caps.done, "manager did not send done");
        assert!(caps.intents.contains(&RenderIntent::Perceptual));
        assert!(caps.features.contains(&Feature::Parametric));
        assert!(caps.features.contains(&Feature::WindowsScrgb));
        assert!(caps.tf.contains(&TransferFunction::Srgb));
        assert!(caps.tf.contains(&TransferFunction::St2084Pq));
        assert!(caps.tf.contains(&TransferFunction::ExtLinear));
        assert!(caps.primaries.contains(&Primaries::Srgb));
        assert!(caps.primaries.contains(&Primaries::Bt2020));
    }

    let (mgr, qh) = {
        let c = f.client(id);
        (c.state.color_manager.clone().unwrap(), c.qh.clone())
    };

    let srgb = make_parametric(&mgr, &qh, TransferFunction::Srgb, Primaries::Srgb);
    let pq = make_parametric(&mgr, &qh, TransferFunction::St2084Pq, Primaries::Bt2020);
    let scrgb = Arc::new(Mutex::new(DescStatus::Pending));
    mgr.create_windows_scrgb(&qh, scrgb.clone());

    f.roundtrip(id);

    assert_eq!(*srgb.lock().unwrap(), DescStatus::Ready, "sRGB not ready");
    assert_eq!(
        *pq.lock().unwrap(),
        DescStatus::Ready,
        "PQ/BT.2020 not ready"
    );
    assert_eq!(
        *scrgb.lock().unwrap(),
        DescStatus::Ready,
        "Windows-scRGB not ready"
    );
}

#[test]
fn incomplete_parametric_set_is_a_protocol_error() {
    let mut f = Fixture::new();
    let id = f.add_client();
    f.roundtrip(id);

    let (mgr, qh) = {
        let c = f.client(id);
        (c.state.color_manager.clone().unwrap(), c.qh.clone())
    };

    // Create with a transfer function but no primaries -> incomplete_set.
    let creator = mgr.create_parametric_creator(&qh, ());
    creator.set_tf_named(TransferFunction::Srgb);
    let status = Arc::new(Mutex::new(DescStatus::Pending));
    creator.create(&qh, status);

    // The harness client dispatch panics when it observes a protocol error, so
    // the roundtrip unwinds; that unwind IS the assertion that the compositor
    // raised the error and killed the client.
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f.roundtrip(id)));
    assert!(res.is_err(), "expected an incomplete_set protocol error");
}

/// Regression: `wp_image_description_info_v1.done` is a destructor event. Sending it
/// inside the `get_information` dispatch that created the info object destroyed the
/// resource while wayland-backend still held a pointer to its user data, which it
/// writes back after the handler returns — use-after-free, compositor segfault.
/// (Any client binding the manager and querying an output description crashed the
/// whole session; wayland-info alone reproduced it.)
#[test]
fn output_get_information_delivers_done() {
    let mut f = Fixture::new();
    f.add_output(1, (1920, 1080));
    let id = f.add_client();
    f.roundtrip(id);

    let (mgr, qh, output) = {
        let c = f.client(id);
        let output = c.output("headless-1");
        (c.state.color_manager.clone().unwrap(), c.qh.clone(), output)
    };

    let cm_output = mgr.get_output(&output, &qh, ());
    let status = Arc::new(Mutex::new(DescStatus::Pending));
    let desc = cm_output.get_image_description(&qh, status.clone());
    f.roundtrip(id);
    assert_eq!(
        *status.lock().unwrap(),
        DescStatus::Ready,
        "output image description not ready"
    );

    let done = Arc::new(Mutex::new(false));
    desc.get_information(&qh, done.clone());
    f.double_roundtrip(id);
    assert!(
        *done.lock().unwrap(),
        "wp_image_description_info_v1.done not received"
    );
}
