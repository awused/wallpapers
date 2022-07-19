use std::cmp::min;
use std::collections::HashSet;
use std::fs::{create_dir_all, File};
use std::num::NonZeroU8;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use aw_upscale::Upscaler;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::imageops::{self, overlay};
use image::{ColorType, GenericImage, ImageBuffer, ImageEncoder, RgbImage};
use lru::LruCache;
use once_cell::sync::{Lazy, OnceCell};
use tempfile::TempDir;

use crate::closing;
use crate::config::{ImageProperties, CONFIG};
use crate::directories::ids::WallpaperID;
use crate::monitors::Monitor;
use crate::processing::resample::resize_par_linear;
use crate::processing::resample::FilterType::Lanczos3;
use crate::processing::{UPSCALING, WORKER};

// This is a small cache because the files can get very large.
// For interactive or preview this is sufficient.
// For Sync mode it's enough that it'll dedupe reads to the same file almost every time.
static FILE_CACHE: Lazy<Mutex<LruCache<PathBuf, Arc<OnceCell<RgbImage>>>>> =
    Lazy::new(|| Mutex::new(LruCache::new(4)));

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct Res {
    w: u32,
    h: u32,
}


impl From<(u32, u32)> for Res {
    fn from(res: (u32, u32)) -> Self {
        Self { w: res.0, h: res.1 }
    }
}

impl Res {
    const fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    fn apply_crop_pad(self, props: &ImageProperties) -> Self {
        let w = u32::try_from(self.w as i32 - props.left.unwrap_or(0) - props.right.unwrap_or(0))
            .unwrap_or(0);
        let h = u32::try_from(self.h as i32 - props.top.unwrap_or(0) - props.bottom.unwrap_or(0))
            .unwrap_or(0);
        (w, h).into()
    }

    fn get_scale(self, props: &Option<ImageProperties>, m: &Monitor) -> NonZeroU8 {
        let r = if let Some(props) = props { self.apply_crop_pad(props) } else { self };

        if r.is_empty() {
            return NonZeroU8::new(1).unwrap();
        }

        let scale = f64::max(m.width as f64 / r.w as f64, m.height as f64 / r.h as f64);

        let scale = f64::max(scale.log2().ceil(), 0.0).exp2().round() as u64;
        let scale = scale.try_into().unwrap_or(32);

        NonZeroU8::new(scale).unwrap()
    }
}

#[derive(Debug)]
enum IntermediateFile {
    AlreadyExists(PathBuf),
    MustBeWritten(PathBuf),
}

impl IntermediateFile {
    fn path(&self) -> &Path {
        match self {
            IntermediateFile::AlreadyExists(p) | IntermediateFile::MustBeWritten(p) => p,
        }
    }
}

// We know at least the final file will be uncached, but the others may be present
#[derive(Debug)]
struct UncachedFiles<'a> {
    pub m: &'a Monitor,
    pub props: Option<ImageProperties>,
    pub cropped: Option<IntermediateFile>,
    pub scale: NonZeroU8,
    pub scaled: IntermediateFile,
    pub final_file: PathBuf,
}


pub struct Wallpaper<'a, T: WallpaperID> {
    pub id: &'a T,
    monitors: &'a [Monitor],
    parent_tdir: &'a TempDir,
    tdir: OnceCell<TempDir>,
    // This could save time and memory during interactive mode, but likely not worth too much.
    // original_image: OnceCell<Arc<DynamicImage>>,
    resolution: OnceCell<Res>,
    mtime: OnceCell<SystemTime>,
}

impl<'a, T: WallpaperID> Wallpaper<'a, T> {
    pub const fn new(id: &'a T, monitors: &'a [Monitor], parent_tdir: &'a TempDir) -> Self {
        Self {
            id,
            monitors,
            parent_tdir,
            tdir: OnceCell::new(),
            resolution: OnceCell::new(),
            mtime: OnceCell::new(),
        }
    }
}

impl<T: WallpaperID> Wallpaper<'_, T> {
    pub fn process(&self, compress: bool) {
        let r = self.get_resolution();
        if r.is_empty() {
            println!("Image {:?} is empty", self.id.original_abs_path());
            return;
        }

        let uncached_monitors = self.get_uncached_files();

        if uncached_monitors.is_empty() {
            return;
        }

        WORKER.in_place_scope_fifo(|s| {
            uncached_monitors
                .iter()
                .filter(|uf| matches!(uf.cropped, Some(IntermediateFile::MustBeWritten(_))))
                .for_each(|uf| {
                    s.spawn_fifo(move |_s| {
                        self.crop(uf);
                    })
                })
        });

        if closing::closed() {
            return;
        }

        UPSCALING.in_place_scope_fifo(|s| {
            uncached_monitors
                .iter()
                .filter(|uf| matches!(uf.scaled, IntermediateFile::MustBeWritten(_)))
                .for_each(|uf| s.spawn_fifo(move |_s| self.upscale(uf)))
        });

        if closing::closed() {
            return;
        }

        // Guaranteed to be work here, so we can go straight to the pool
        WORKER.scope_fifo(|s| {
            uncached_monitors
                .iter()
                .for_each(|uf| s.spawn_fifo(move |_s| Self::finish(uf, compress)))
        });
    }

    fn crop(&self, uf: &UncachedFiles) {
        if closing::closed() {
            return;
        }

        let mut input = image::open(self.id.original_abs_path()).unwrap_or_else(|e| {
            panic!("Unable to read image {:?}: {}", self.id.original_abs_path(), e)
        });
        let props = uf.props.as_ref().expect("Impossible");
        let output_file = uf.cropped.as_ref().expect("Impossible").path();

        let r = self.get_resolution();
        let new_r = r.apply_crop_pad(props);

        assert!(!new_r.is_empty(), "Empty output image after cropping.");

        let mut output = ImageBuffer::from_pixel(
            new_r.w,
            new_r.h,
            props.background.unwrap_or_else(|| [0, 0, 0, 0xff].into()),
        );

        let (inset_left, margin_left) = match props.left {
            Some(left) if left > 0 => (left as u32, 0),
            Some(left) if left < 0 => (0, left.unsigned_abs()),
            _ => (0, 0),
        };

        let (inset_top, margin_top) = match props.top {
            Some(top) if top > 0 => (top as u32, 0),
            Some(top) if top < 0 => (0, top.unsigned_abs()),
            _ => (0, 0),
        };

        let inset_right = match props.right {
            Some(right) if right > 0 => right as u32,
            _ => 0,
        };

        let inset_bottom = match props.bottom {
            Some(bottom) if bottom > 0 => bottom as u32,
            _ => 0,
        };

        let sub_input = input.sub_image(
            inset_left,
            inset_top,
            r.w - inset_left - inset_right,
            r.h - inset_top - inset_bottom,
        );


        // NOTE -- cannot be easily replaced by memcpy in the general case, needs to handle alpha
        // blending.
        // TODO -- simplify the above code now that it's possible.
        overlay(&mut output, &*sub_input, margin_left as i64, margin_top as i64);
        // TODO -- throw output in the LruCache here, only if there's no denoising/upscaling
        // scheduled.

        let f = File::create(output_file).expect("Couldn't create output file");
        let enc = PngEncoder::new_with_quality(f, CompressionType::Fast, FilterType::Sub);

        enc.write_image(output.as_raw(), output.width(), output.height(), ColorType::Rgba8)
            .unwrap_or_else(|e| panic!("Failed to save file {:?}: {}", output_file, e));
    }

    fn upscale(&self, uf: &UncachedFiles) {
        if closing::closed() {
            return;
        }

        let mut upscaler = Upscaler::new(CONFIG.alternate_upscaler.clone());
        upscaler.set_scale(uf.scale.get());
        if let Some(ImageProperties { denoise: Some(denoise), .. }) = uf.props {
            upscaler.set_denoise(Some(denoise));
        } else {
            upscaler.set_denoise(Some(1));
        }

        upscaler
            .run(
                uf.cropped
                    .as_ref()
                    .map_or_else(|| self.id.original_abs_path(), |p| p.path().to_path_buf()),
                uf.scaled.path(),
            )
            .map_or_else(
                |e| {
                    assert!(
                        closing::closed(),
                        "Failed to upscale image {:?}: {}",
                        self.id.original_abs_path(),
                        e
                    );
                },
                |_| (),
            );
    }

    fn finish(uf: &UncachedFiles, compress: bool) {
        if closing::closed() {
            return;
        }

        create_dir_all(
            uf.final_file.parent().expect("Impossible for cached file to have no directory"),
        )
        .expect("Unable to create cache directories");


        let cell = {
            let mut cache = FILE_CACHE.lock().unwrap();
            cache
                .get_or_insert(uf.scaled.path().to_path_buf(), Arc::default)
                .unwrap()
                .clone()
        };

        // Just double box, this is only dereferenced a few times anyway.
        let mut img: Box<dyn Deref<Target = RgbImage>> = Box::new(cell.get_or_init(|| {
            image::open(uf.scaled.path())
                .unwrap_or_else(|e| panic!("Unable to read image {:?}: {}", uf.scaled.path(), e))
                .into_rgb8()
        }));

        if let Some(ImageProperties { vertical, horizontal, .. }) = uf.props.as_ref() {
            if vertical.is_some() || horizontal.is_some() {
                img = Box::new(Box::new(translate_image(&img, vertical, horizontal)));
            }
        }


        let (m_w, m_h) = (uf.m.width, uf.m.height);

        if img.width() != m_w && img.height() != m_h {
            let ratio = f64::max(m_w as f64 / img.width() as f64, m_h as f64 / img.height() as f64);

            let int_w = (img.width() as f64 * ratio).round() as u32;
            let int_h = (img.height() as f64 * ratio).round() as u32;

            img = Box::new(Box::new(resize_par_linear(
                &img,
                img.dimensions(),
                (int_w, int_h),
                Lanczos3,
            )));
        }

        if img.width() != m_w || img.width() != m_h {
            let (w, h) = img.dimensions();
            img = Box::new(Box::new(
                imageops::crop_imm(
                    &**img,
                    w.saturating_sub(m_w) / 2,
                    h.saturating_sub(m_h) / 2,
                    m_w,
                    m_h,
                )
                .to_image(),
            ));
        }

        let f = File::create(&uf.final_file).expect("Couldn't create output file");
        let enc = PngEncoder::new_with_quality(
            f,
            if compress { CompressionType::Best } else { CompressionType::Fast },
            FilterType::NoFilter,
        );

        enc.write_image(&*img, img.width(), img.height(), ColorType::Rgb8)
            .unwrap_or_else(|e| panic!("Failed to save file {:?}: {}", uf.final_file, e));
    }

    fn get_uncached_files(&self) -> Vec<UncachedFiles> {
        let mtime = *self.mtime.get_or_init(|| get_mtime(self.id.original_abs_path()));

        let mut dedupe = HashSet::new();

        let uncached_monitors: Vec<_> = self
            .monitors
            .iter()
            .filter_map(|m| {
                let props = self.id.get_props(m);

                let path = self.id.cached_abs_path(m, &props);
                if path.is_file() && get_mtime(&path) >= mtime {
                    None
                } else if !dedupe.contains(&path) {
                    dedupe.insert(path.clone());
                    Some((m, path, props))
                } else {
                    None
                }
            })
            .collect();

        uncached_monitors
            .into_iter()
            .map(|(m, final_file, props)| {
                let cropped = self.id.cropped_rel_path(&props).map(|p| self.get_tdir().join(p));
                let cropped = if let Some(cropped) = cropped {
                    if !dedupe.contains(&cropped) {
                        dedupe.insert(cropped.clone());
                        if cropped.is_file() {
                            Some(IntermediateFile::AlreadyExists(cropped))
                        } else {
                            Some(IntermediateFile::MustBeWritten(cropped))
                        }
                    } else {
                        Some(IntermediateFile::AlreadyExists(cropped))
                    }
                } else {
                    None
                };

                let scale = self.get_resolution().get_scale(&props, m);

                let scaled = self.get_tdir().join(self.id.upscaled_rel_path(scale, &props));
                let scaled = if !dedupe.contains(&scaled) {
                    dedupe.insert(scaled.clone());
                    if scaled.is_file() {
                        IntermediateFile::AlreadyExists(scaled)
                    } else {
                        IntermediateFile::MustBeWritten(scaled)
                    }
                } else {
                    IntermediateFile::AlreadyExists(scaled)
                };

                UncachedFiles {
                    m,
                    props,
                    cropped,
                    scale,
                    scaled,
                    final_file,
                }
            })
            .collect()
    }

    fn get_resolution(&self) -> Res {
        *self.resolution.get_or_init(|| {
            image::image_dimensions(self.id.original_abs_path())
                .unwrap_or_else(|_| {
                    panic!("Unable to read resolution of image {:?}", self.id.original_abs_path())
                })
                .into()
        })
    }

    fn get_tdir(&self) -> &Path {
        self.tdir
            .get_or_init(|| {
                let mut builder = tempfile::Builder::new();
                builder.prefix("wallpaper");
                builder.tempdir_in(self.parent_tdir.path()).unwrap()
            })
            .path()
    }
}

fn get_mtime<P: AsRef<Path>>(p: P) -> SystemTime {
    p.as_ref()
        .metadata()
        .unwrap_or_else(|_| panic!("Could not stat file {:?}", p.as_ref()))
        .modified()
        .unwrap_or_else(|_| panic!("Could not read modification time of file {:?}", p.as_ref()))
}

fn translate_image(img: &RgbImage, vertical: &Option<f64>, horizontal: &Option<f64>) -> RgbImage {
    static CHANNELS: usize = 3;

    let (v, h) = (vertical.unwrap_or(0.0), horizontal.unwrap_or(0.0));

    let (width, height) = (img.width() as usize, img.height() as usize);

    let (inset_left, margin_left) = if h < 0.0 {
        (0, (h / -100.0 * (width as f64)).round() as usize)
    } else {
        ((h / 100.0 * (width as f64)).round() as usize, 0)
    };

    let (inset_top, margin_top) = if v > 0.0 {
        (0, (v / 100.0 * (height as f64)).round() as usize)
    } else {
        ((v / -100.0 * (height as f64)).round() as usize, 0)
    };

    if inset_left == 0 && margin_left == 0 && inset_top == 0 && margin_top == 0 {
        // This should rarely happen if we actually enter this function.
        return img.clone();
    }

    let src_top = min(inset_top, height);
    let src_bottom = height.saturating_sub(margin_top);
    let src_left = min(inset_left, width);
    let src_right = width.saturating_sub(margin_left);

    // With enough effort this can be done in-place, but it's annoying and not really worth it.
    let input = img.as_raw();
    let mut output = vec![0xffu8; width * height * CHANNELS];

    if src_bottom > src_top && src_right > src_left {
        let row_bytes = width * CHANNELS;
        let dst_top = min(margin_top, height);
        let dst_left_byte = min(margin_left, width) * CHANNELS;
        let dst_right_byte = dst_left_byte + (src_right - src_left) * CHANNELS;

        if src_left == 0 && src_right == width {
            // Not always appreciably faster, but make an effort.
            output[dst_top * row_bytes..(dst_top + src_bottom - src_top) * row_bytes]
                .copy_from_slice(&input[src_top * row_bytes..src_bottom * row_bytes]);
        } else {
            for (y, row) in output
                .chunks_exact_mut(row_bytes)
                .enumerate()
                .skip(dst_top)
                .take(src_bottom - src_top)
            {
                let src_row_start = (src_top + y - dst_top) * row_bytes;
                let src_start = src_row_start + src_left * CHANNELS;
                let src_end = src_row_start + src_right * CHANNELS;

                row[dst_left_byte..dst_right_byte].copy_from_slice(&input[src_start..src_end]);
            }
        }
    }


    RgbImage::from_vec(width as u32, height as u32, output).unwrap()
}
