use std::collections::{BTreeMap, HashMap};
use std::ffi::CString;
use std::os::fd::BorrowedFd;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use std::{mem, process, ptr};

use color_eyre::Result;
use color_eyre::eyre::bail;
use futures::StreamExt;
use futures::stream::FuturesUnordered;
use image::RgbaImage;
use libc::{
    MAP_SHARED, O_CREAT, O_EXCL, O_RDWR, PROT_READ, PROT_WRITE, close, ftruncate, shm_open,
    shm_unlink,
};
use nix::errno::Errno;
use tokio::io::unix::AsyncFd;
use tokio::select;
use tokio::sync::oneshot;
use tokio::time::{Instant, sleep_until, timeout};
use wayland_client::protocol::wl_buffer::WlBuffer;
use wayland_client::protocol::wl_compositor::WlCompositor;
use wayland_client::protocol::wl_output::{self, WlOutput};
use wayland_client::protocol::wl_region::WlRegion;
use wayland_client::protocol::wl_registry::{self, WlRegistry};
use wayland_client::protocol::wl_shm::{Format, WlShm};
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

use crate::closing::closed;
use crate::monitors::Monitor;
use crate::processing::WORKER;
use crate::wallpaper::OPTIMISTIC_CACHE;

pub fn init() -> Option<Conn> {
    let con = Connection::connect_to_env().ok()?;
    let display = con.display();

    let queue = con.new_event_queue();
    let _registry = display.get_registry(&queue.handle(), ());

    Some(Conn {
        queue,
        _registry,
        state: AppData::default(),
    })
}

#[derive(Debug)]
struct Output {
    wl_output: WlOutput,
    fract_scale: Option<WpFractionalScaleV1>,
    surface: Option<WlSurface>,
    viewport: Option<WpViewport>,
    layer_surface: Option<ZwlrLayerSurfaceV1>,
    // Resolution in logical pixels
    res: Option<(u32, u32)>,
    fractional_scale: Option<u32>,
    int_scale: i32,
    clean: bool,
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
            if closed() {
                return Ok(Vec::new());
            }

            if self.state.outputs.values().all(Output::ready) {
                // If there are any pending updates we should catch them in set_wallpapers
                break;
            }
            timeout(Duration::from_secs(5), self.poll_once()).await??;
        }

        Ok(self
            .state
            .outputs
            .iter_mut()
            .map(|(name, out)| {
                out.clean = true;
                // ready -> all of them have resolutions
                let (w, h) = out.res().unwrap();
                Monitor {
                    width: w as u32,
                    height: h as u32,
                    #[cfg(feature = "x11")]
                    top: 0,
                    #[cfg(feature = "x11")]
                    left: 0,
                    name: *name,
                }
            })
            .collect::<Vec<_>>())
    }

    // This should eventually return in case of a new monitor that needs a wallpaper in daemon
    // mode, maybe interactive too.
    pub async fn poll(&mut self) -> Result<Vec<Monitor>> {
        while !self.state.outputs.values().any(|out| out.ready() && !out.clean) {
            self.poll_once().await?;
        }

        self.queue.roundtrip(&mut self.state)?;

        Ok(self
            .state
            .outputs
            .iter_mut()
            .filter(|(_name, out)| out.ready() && !out.clean)
            .map(|(name, out)| {
                out.clean = true;
                // ready -> all of them have resolutions
                let (w, h) = out.res().unwrap();
                Monitor {
                    width: w as u32,
                    height: h as u32,
                    #[cfg(feature = "x11")]
                    top: 0,
                    #[cfg(feature = "x11")]
                    left: 0,
                    name: *name,
                }
            })
            .collect())
    }

    async fn poll_once(&mut self) -> Result<()> {
        self.queue.flush()?;

        let Some(guard) = self.queue.prepare_read() else {
            self.queue.dispatch_pending(&mut self.state)?;
            return Ok(());
        };

        let mut fd = AsyncFd::new(guard.connection_fd())?;
        let _readable = fd.readable_mut().await?;
        drop(fd);
        guard.read()?;

        self.queue.dispatch_pending(&mut self.state)?;
        Ok(())
    }

    pub async fn set_wallpapers(
        &mut self,
        wallpapers: HashMap<PathBuf, Vec<&Monitor>>,
    ) -> Result<()> {
        self.queue.roundtrip(&mut self.state)?;

        let mut image_futures: FuturesUnordered<_> = wallpapers
            .into_iter()
            .map(|(p, monitors)| {
                let (send, recv) = oneshot::channel::<(Result<ShmImage>, Vec<u32>)>();
                let monitors = monitors.into_iter().map(|m| m.name).collect();

                WORKER.spawn(move || {
                    let mi = if let Some(cache) = OPTIMISTIC_CACHE.get()
                        && let Some(cached) = cache.read().unwrap().peek(&p)
                    {
                        copy_to_shm(cached)
                    } else {
                        let mut img = image::open(&p).unwrap().into_rgba8();
                        // rgba8 -> BGRX, remove transparency
                        img.chunks_exact_mut(4).for_each(|c| c.swap(0, 2));
                        copy_to_shm(&img)
                    };
                    send.send((mi, monitors)).unwrap();
                });

                recv
            })
            .collect();

        let mut pending_wallpapers: Vec<(ShmImage, Vec<u32>)> = Vec::new();

        let deadline = Instant::now() + Duration::from_secs(60);
        // Only do flushing/polling if it seems to be going slow, otherwise try to do it
        // atomically;
        let mut fast_deadline = None;
        let mut slow = false;

        loop {
            // A bit clunky to insert them into a vec and just dump them but it's fine
            pending_wallpapers.retain_mut(|(image, monitors)| {
                monitors.retain(|m| {
                    // Could be true if it was a permanent failure, but that's fine here.
                    let committed = self.try_upload(image, *m);
                    if committed && fast_deadline.is_none() {
                        fast_deadline = Some(Instant::now() + Duration::from_millis(500));
                    }

                    !committed
                });
                !monitors.is_empty()
            });

            if pending_wallpapers.is_empty() && image_futures.is_empty() {
                break;
            }

            // Pause polling for a brief duration after the first commit if we're not waiting for
            // some updates.
            let allow_polling = slow || fast_deadline.is_none() || !pending_wallpapers.is_empty();
            select! {
                Some(res) = image_futures.next() => {
                    let (image, monitors) = res?;
                    let image = image?;
                    pending_wallpapers.push((image, monitors));
                },
                res = self.poll_once(), if allow_polling => {
                    res?;
                },
                _ = sleep_until(fast_deadline.unwrap()), if !slow && fast_deadline.is_some() => {
                    slow = true;
                },
                _ = sleep_until(deadline) => {
                    bail!("Failed to set all wallpapers within a reasonable timeframe");
                }
            }
        }
        self.queue.flush()?;

        Ok(())
    }

    // Returns true if this was the final try. False means to retain this monitor to try again.
    fn try_upload(&mut self, image: &ShmImage, m: u32) -> bool {
        let Some(output) = self.state.outputs.get_mut(&m) else {
            println!("Missing monitor after load {m}");
            return true;
        };

        if !output.ready() || !output.clean {
            println!("Output isn't ready: {m}");
            return false;
        }

        if !output
            .res()
            .is_some_and(|r| r.0 as u32 == image.res.0 && r.1 as u32 == image.res.1)
        {
            println!("Output resolution has changed: {m}");
            return true;
        }

        let w = image.res.0 as i32;
        let h = image.res.1 as i32;
        let qh = &self.queue.handle();

        let pool = self.state.shm.as_ref().unwrap().create_pool(
            unsafe { BorrowedFd::borrow_raw(image.fd) },
            image.size as i32,
            qh,
            (),
        );

        let buf = pool.create_buffer(0, w, h, w * 4, Format::Xrgb8888, qh, ());
        pool.destroy();


        let surface = output.surface.as_ref().unwrap();
        surface.attach(Some(&buf), 0, 0);

        surface.damage(0, 0, w, h);

        if let Some(view) = &output.viewport {
            let unscaled = output.res.unwrap();
            view.set_destination(unscaled.0 as i32, unscaled.1 as i32);
        } else {
            surface.set_buffer_scale(output.int_scale);
        }

        surface.commit();

        buf.destroy();
        true
    }
}

#[derive(Debug)]
struct ShmImage {
    buf: *mut i8,
    size: usize,
    res: (u32, u32),
    fd: i32,
}
unsafe impl Send for ShmImage {}

impl Drop for ShmImage {
    fn drop(&mut self) {
        unsafe {
            close(self.fd);
            libc::munmap(self.buf.cast(), self.size);
        }
    }
}

// Should be good enough
static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

fn copy_to_shm(img: &RgbaImage) -> Result<ShmImage> {
    // If this runs into problems, we'll need rng
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let name = CString::new(format!("aw-wallpapers{}-{}", process::id(), id)).unwrap();

    let res = img.dimensions();
    let raw_img = img.as_raw();
    let size = mem::size_of_val(&raw_img[0]) * raw_img.len();

    let fd = unsafe { shm_open(name.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600) };
    if fd < 0 {
        bail!("Unable to open shared memory: {fd}");
    }

    let buf = unsafe {
        shm_unlink(name.as_ptr());
        let mut ret = 1;
        for _ in 0..100 {
            ret = ftruncate(fd, size as i64);
            if ret == 0 || Errno::last() != Errno::EINTR {
                break;
            }
        }
        if ret < 0 {
            close(fd);
            bail!("Failed to extend file descriptor to {}: {}", size, ret);
        }

        let buf = libc::mmap(ptr::null_mut(), size as _, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        assert_eq!(size, img.len());
        buf.copy_from_nonoverlapping(img.as_ptr().cast(), size as _);
        buf
    }
    .cast();

    Ok(ShmImage { buf, res, size, fd })
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
            wl_registry::Event::Global { name, interface, .. } => {
                if interface == WlOutput::interface().name {
                    let wl_output = reg.bind::<WlOutput, _, _>(name, 2, qh, name);
                    let output = Output {
                        wl_output,
                        fract_scale: None,
                        surface: None,
                        viewport: None,
                        layer_surface: None,
                        res: None,
                        fractional_scale: None,
                        int_scale: 1,
                        clean: false,
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
            if output.int_scale != factor {
                output.int_scale = factor;
                output.clean = false;
                println!("Output {name} dirtied by new int scale {factor}");
            }
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
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            let output = state.outputs.get_mut(name).unwrap();
            if output.fractional_scale != Some(scale) {
                output.fractional_scale = Some(scale);
                output.clean = false;
                println!("Output {name} dirtied by PreferredScale {scale}");
            }
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
            if output.res != Some((width, height)) {
                output.res = Some((width, height));
                output.clean = false;
                println!("Output {name} dirtied by Configure {width}x{height}");
            }

            proxy.ack_configure(serial);
        }
    }
}

delegate_noop!(AppData: ignore WlBuffer);
delegate_noop!(AppData: ignore WlCompositor);
delegate_noop!(AppData: ignore WlRegion);
// We don't care about format since Xrgb8888 _must_ be supported
delegate_noop!(AppData: ignore WlShm);
delegate_noop!(AppData: ignore WlShmPool);
delegate_noop!(AppData: ignore WlSurface);
delegate_noop!(AppData: ignore WpFractionalScaleManagerV1);
delegate_noop!(AppData: ignore WpViewport);
delegate_noop!(AppData: ignore WpViewporter);
delegate_noop!(AppData: ignore ZwlrLayerShellV1);
