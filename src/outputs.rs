//! Logical sizes of the Wayland outputs, read through xdg-output.
//!
//! The picker overlay has to ask the compositor for an explicit size: GPUI
//! cannot open a layer surface with the zero size that would let the
//! compositor stretch it, because it sends that size as the viewport
//! destination when the output scale arrives before the first configure,
//! which is a protocol error.

use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output, wl_registry};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle};
use wayland_protocols::xdg::xdg_output::zv1::client::{zxdg_output_manager_v1, zxdg_output_v1};

/// Logical size, in the compositor's logical pixels, of each output.
pub fn logical_sizes() -> Vec<(u32, u32)> {
    read_logical_sizes().unwrap_or_default()
}

/// The overlay size that covers whichever output the compositor places it on.
///
/// The compositor chooses the output (usually the focused one), so with
/// outputs of different sizes this takes the largest of each dimension. The
/// overlay is centred on its output, and so is the panel inside it.
pub fn overlay_size(sizes: &[(u32, u32)]) -> Option<(u32, u32)> {
    let width = sizes.iter().map(|size| size.0).max()?;
    let height = sizes.iter().map(|size| size.1).max()?;
    (width > 0 && height > 0).then_some((width, height))
}

#[derive(Default)]
struct State {
    sizes: Vec<Option<(u32, u32)>>,
}

fn read_logical_sizes() -> Option<Vec<(u32, u32)>> {
    let connection = Connection::connect_to_env().ok()?;
    let (globals, mut queue) = registry_queue_init::<State>(&connection).ok()?;
    let handle = queue.handle();
    let manager: zxdg_output_manager_v1::ZxdgOutputManagerV1 =
        globals.bind(&handle, 2..=3, ()).ok()?;
    let mut state = State::default();
    let mut xdg_outputs = Vec::new();
    for global in globals.contents().clone_list() {
        if global.interface != wl_output::WlOutput::interface().name {
            continue;
        }
        let output: wl_output::WlOutput =
            globals
                .registry()
                .bind(global.name, global.version.min(4), &handle, ());
        xdg_outputs.push(manager.get_xdg_output(&output, &handle, state.sizes.len()));
        state.sizes.push(None);
    }
    queue.roundtrip(&mut state).ok()?;
    for xdg_output in xdg_outputs {
        xdg_output.destroy();
    }
    manager.destroy();
    let _ = connection.flush();
    Some(state.sizes.into_iter().flatten().collect())
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wl_output::WlOutput, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_output::WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_manager_v1::ZxdgOutputManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &zxdg_output_manager_v1::ZxdgOutputManagerV1,
        _: zxdg_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<zxdg_output_v1::ZxdgOutputV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &zxdg_output_v1::ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        index: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::LogicalSize { width, height } = event
            && let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height))
        {
            state.sizes[*index] = Some((width, height));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::overlay_size;

    #[test]
    fn overlay_matches_a_single_output() {
        assert_eq!(overlay_size(&[(1600, 1000)]), Some((1600, 1000)));
    }

    #[test]
    fn overlay_covers_the_largest_output() {
        assert_eq!(
            overlay_size(&[(1920, 1080), (1280, 1440)]),
            Some((1920, 1440))
        );
    }

    #[test]
    fn no_outputs_means_no_overlay_size() {
        assert_eq!(overlay_size(&[]), None);
        assert_eq!(overlay_size(&[(0, 0)]), None);
    }
}
