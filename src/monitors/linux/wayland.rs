use std::collections::BTreeMap;
use std::error::Error;
use std::ffi::os_str::Display;
use std::time::Duration;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use tokio::io::unix::AsyncFd;
use tokio::time::timeout;
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::WlShm;
use wayland_client::protocol::wl_shm_pool::WlShmPool;
use wayland_client::protocol::wl_surface::WlSurface;
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, delegate_noop};
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1;
use wayland_protocols::wp::fractional_scale::v1::client::wp_fractional_scale_v1::{
    self, WpFractionalScaleV1,
};
use wayland_protocols::wp::viewporter::client::wp_viewport::WpViewport;
use wayland_protocols::wp::viewporter::client::wp_viewporter::WpViewporter;
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_shell_v1::{self, ZwlrLayerShellV1};
use wayland_protocols_wlr::layer_shell::v1::client::zwlr_layer_surface_v1::{
    self, Anchor, ZwlrLayerSurfaceV1,
};

use crate::closing::{close, closed};
use crate::monitors::Monitor;

pub fn init() -> Option<Conn> {
    let con = Connection::connect_to_env().ok()?;
    let display = con.display();

    let queue = con.new_event_queue();
    let _registry = display.get_registry(&queue.handle(), ());

    println!("got wayland");
    Some(Conn {
        queue,
        _registry,
        state: AppData::default(),
    })
}

struct Output {
    name: u32,
    wl_output: WlOutput,
    fract_scale: Option<WpFractionalScaleV1>,
    surface: Option<WlSurface>,
    viewport: Option<WpViewport>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    // Resolution in logical pixels
    res: Option<(u32, u32)>,
    fractional_scale: Option<u32>,
    int_scale: i32,
}

impl Drop for Output {
    fn drop(&mut self) {
        if let Some(fract_scale) = self.fract_scale.take() {
            fract_scale.destroy();
        }
        if let Some(layer_surface) = self.layer_surface.take() {
            layer_surface.destroy();
        }
        if let Some(viewport) = self.viewport.take() {
            viewport.destroy();
        }
        if let Some(surface) = self.surface.take() {
            surface.destroy();
        }
        self.wl_output.release();
    }
}

impl Output {
    const fn ready(&self) -> bool {
        (self.fract_scale.is_none() || self.fractional_scale.is_some()) && self.res.is_some()
    }

    fn res(&self) -> Option<(i32, i32)> {
        let (w, h) = self.res?;
        if let Some(scale) = self.fractional_scale {
            // rounds upwards, not above u32 max
            let w = (w as u64 * scale as u64 + 60) / 120;
            let h = (h as u64 * scale as u64 + 60) / 120;
            return Some((w as i32, h as i32));
        } else if self.fract_scale.is_some() {
            return None;
        }

        Some((w as i32 * self.int_scale, h as i32 * self.int_scale))
    }
}

#[derive(Default)]
struct AppData {
    outputs: BTreeMap<u32, Output>,
    compositor: Option<WlCompositor>,
    fractional: Option<WpFractionalScaleManagerV1>,
    viewporter: Option<WpViewporter>,
    layer_shell: Option<ZwlrLayerShellV1>,
    shm: Option<WlShm>,
}

pub(super) struct Conn {
    queue: EventQueue<AppData>,
    _registry: WlRegistry,
    state: AppData,
}

impl Conn {
    // May briefly block
    pub async fn list_monitors(&mut self) -> Result<Vec<Monitor>> {
        // Be explicit about flushing and waiting, depending on program state we might have all
        // outputs ready but invalidated.
        self.queue.roundtrip(&mut self.state)?;

        loop {
            if self.state.outputs.values().all(Output::ready) {
                break;
            }
            timeout(Duration::from_secs(5), self.poll_once()).await??;
        }

        // timeout 5s -> treat as closed
        todo!()
    }

    // This should eventually return in case of a new monitor that needs a wallpaper in daemon
    // mode, maybe interactive too.
    pub async fn poll(&mut self) -> Result<()> {
        loop {
            self.poll_once().await?;
        }
    }

    async fn poll_once(&mut self) -> Result<()> {
        self.queue.flush()?;
        let Some(guard) = self.queue.prepare_read() else {
            self.queue.dispatch_pending(&mut self.state)?;
            return Ok(());
        };

        let mut fd = AsyncFd::new(guard.connection_fd())?;
        let mut readable = fd.readable_mut().await?;
        readable.clear_ready(); // Should be unnecssary since these fds are one-time use

        self.queue.dispatch_pending(&mut self.state)?;
        Ok(())
    }
}

impl Dispatch<WlRegistry, ()> for AppData {
    fn event(
        state: &mut Self,
        reg: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _con: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        // When receiving events from the wl_registry, we are only interested in the
        // `global` event, which signals a new available global.
        // When receiving this event, we just print its characteristics in this example.
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                println!("[{}] {} (v{})", name, interface, version);
                if interface == WlOutput::interface().name {
                    let wl_output = reg.bind::<WlOutput, _, _>(name, 2, qh, name);
                    let output = Output {
                        name,
                        wl_output,
                        fract_scale: None,
                        surface: None,
                        viewport: None,
                        layer_surface: None,
                        res: None,
                        fractional_scale: None,
                        int_scale: 1,
                    };
                    state.outputs.insert(name, output);
                } else if interface == WpFractionalScaleManagerV1::interface().name {
                    let fractional_manager =
                        reg.bind::<WpFractionalScaleManagerV1, _, _>(name, 1, qh, ());
                    state.fractional = Some(fractional_manager);
                } else if interface == WlCompositor::interface().name {
                    let compositor = reg.bind::<WlCompositor, _, _>(name, 1, qh, ());
                    state.compositor = Some(compositor);
                } else if interface == WpViewporter::interface().name {
                    let viewporter = reg.bind::<WpViewporter, _, _>(name, 1, qh, ());
                    state.viewporter = Some(viewporter);
                } else if interface == ZwlrLayerShellV1::interface().name {
                    let layer_shell = reg.bind::<ZwlrLayerShellV1, _, _>(name, 1, qh, ());
                    state.layer_shell = Some(layer_shell);
                } else if interface == WlShm::interface().name {
                    let shm = reg.bind::<WlShm, _, _>(name, 1, qh, ());
                    state.shm = Some(shm);
                }
            }
            wl_registry::Event::GlobalRemove { name } => {
                println!(
                    "Removing {name}, was known output: {}",
                    state.outputs.remove(&name).is_some()
                );
            }
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for AppData {
    fn event(
        state: &mut Self,
        _reg: &WlOutput,
        event: wl_output::Event,
        name: &u32,
        _con: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Scale { factor } = event {
            let output = state.outputs.get_mut(name).unwrap();
            output.int_scale = factor;
        }

        if matches!(event, wl_output::Event::Done) {
            let output = state.outputs.get_mut(name).unwrap();
            let compositor = state.compositor.as_mut().unwrap_or_else(|| {
                panic!("Required interface not implemented: {}", WlCompositor::interface().name)
            });

            let surface = compositor.create_surface(qh, ());
            let region = compositor.create_region(qh, ());
            surface.set_input_region(Some(&region));
            region.destroy();

            if let Some(manager) = &state.fractional {
                let scale = manager.get_fractional_scale(&surface, qh, *name);
                output.fract_scale = Some(scale);

                if let Some(viewporter) = &state.viewporter {
                    let viewport = viewporter.get_viewport(&surface, qh, ());
                    output.viewport = Some(viewport);
                }
            }

            let layer_shell = state.layer_shell.as_mut().unwrap_or_else(|| {
                panic!("Required interface not implemented: {}", ZwlrLayerShellV1::interface().name)
            });

            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                Some(&output.wl_output),
                zwlr_layer_shell_v1::Layer::Background,
                "wall".to_string(),
                qh,
                *name,
            );
            layer_surface.set_size(0, 0);
            layer_surface.set_exclusive_zone(-1);
            layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Right | Anchor::Left);

            output.layer_surface = Some(layer_surface);
            surface.commit();
            output.surface = Some(surface);
        }
    }
}

impl Dispatch<WpFractionalScaleV1, u32> for AppData {
    fn event(
        state: &mut Self,
        _proxy: &WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        println!("frac {event:?}");
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let output = state.outputs.get_mut(name).unwrap();
            output.fractional_scale = Some(scale);
        }
    }
}

impl Dispatch<ZwlrLayerSurfaceV1, u32> for AppData {
    fn event(
        state: &mut Self,
        proxy: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        name: &u32,
        _conn: &Connection,
        _qhandle: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, width, height } = event {
            let output = state.outputs.get_mut(name).unwrap();
            output.res = Some((width, height));

            proxy.ack_configure(serial);
        }
    }
}

delegate_noop!(AppData: WlBuffer);
delegate_noop!(AppData: WlCompositor);
delegate_noop!(AppData: WlRegion);
// We don't care about format since Xrgb8888 _must_ be supported
delegate_noop!(AppData: WlShm);
delegate_noop!(AppData: WlShmPool);
delegate_noop!(AppData: WlSurface);
delegate_noop!(AppData: WpFractionalScaleManagerV1);
delegate_noop!(AppData: WpViewport);
delegate_noop!(AppData: WpViewporter);
delegate_noop!(AppData: ZwlrLayerShellV1);
