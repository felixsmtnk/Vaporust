//! Image loading for egui: download, decoding with downscaling in the background, memory saving.
//!
//! The usual egui_extras loaders decode the whole image on the main thread and keep
//! three copies in memory at once: the downloaded file, the decoded image and the texture. Here:
//! - HttpLoader downloads at most 6 files at a time (most recently requested first, i.e.
//!   what is visible now) and does not keep files that finished after leaving the section;
//! - SmallLoader decodes on two background threads (not a thread per image: a dozen
//!   4 MB GIFs at once blew memory up by a hundred megabytes) and downscales so that the larger
//!   side is at most max_side(); once egui has made a texture from the image, our own
//!   copy is dropped.
//! Animated GIFs (URL "...gif#frame") and SVG go to the egui_extras loaders.
//!
//! The image cache is cleared when switching sections and when over the limit (budget_tick).

use eframe::egui;
use egui::load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::mutex::Mutex;
use egui::ColorImage;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Memory saving mode ("Performance" setting): smaller images, smaller cache.
pub static ECONOMY: AtomicBool = AtomicBool::new(false);
/// Animations (stickers, GIFs, chat effects, loading icon). Disabled in settings.
pub static ANIMATIONS: AtomicBool = AtomicBool::new(true);
/// Images no larger than this (avatars, game icons, emoticons) are not unloaded on section change.
const SMALL: usize = 128;
/// Cache of decoded images (for selective unloading).
static SMALL_CACHE: std::sync::OnceLock<Arc<Mutex<HashMap<String, Entry>>>> = std::sync::OnceLock::new();
/// Download state (for selective unloading).
static HTTP_STATE: std::sync::OnceLock<Arc<Mutex<HttpState>>> = std::sync::OnceLock::new();

/// Image cache size at the last check (for the stats.rs measurement log).
pub static LAST_BYTES: AtomicU64 = AtomicU64::new(0);

/// Larger side of an image in memory, pixels (nothing in the program is shown bigger).
pub fn max_side() -> u32 {
    if ECONOMY.load(Ordering::Relaxed) { 512 } else { 800 }
}

/// Memory limit of the egui image cache, above which the cache is cleared.
fn budget() -> usize {
    if ECONOMY.load(Ordering::Relaxed) { 20 << 20 } else { 48 << 20 }
}

pub fn animations() -> bool {
    ANIMATIONS.load(Ordering::Relaxed)
}

/// Marker at the end of a URL: show a GIF still (egui animates URLs ending with
/// ".gif"). HttpLoader removes it before downloading.
const STILL: &str = "?vaporust-still";
/// Marker "shrink to N points on the larger side" (thumbnails of large images: profile backgrounds).
const MAX: &str = "?vaporust-max=";

/// URL of an image that is enough to show at most `px` points on the larger side.
pub fn sized(url: &str, px: u32) -> String {
    format!("{url}{MAX}{px}")
}

/// Real URL for downloading: without Vaporust markers.
fn strip_markers(uri: &str) -> &str {
    uri.find("?vaporust-").map_or(uri, |i| &uri[..i])
}

/// Side limit from the URL marker (if any).
fn max_from(uri: &str) -> Option<u32> {
    let i = uri.find(MAX)?;
    uri[i + MAX.len()..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// URL of a GIF that is always shown still (item grids: dozens of animations at once
/// would take hundreds of megabytes).
pub fn still(url: &str) -> String {
    if url.to_ascii_lowercase().ends_with(".gif") {
        format!("{url}{STILL}")
    } else {
        url.to_string()
    }
}

/// Image URL with the animation setting applied: with animations off a GIF is shown still.
pub fn still_if_needed(url: &str) -> String {
    if !animations() && url.to_ascii_lowercase().ends_with(".gif") {
        format!("{url}{STILL}")
    } else {
        url.to_string()
    }
}

// ---------------------------------------------------------------- downloading

#[derive(Clone)]
enum Http {
    Queued,
    Running,
    Ready(Arc<[u8]>, Option<String>),
    Failed(String),
}

#[derive(Default)]
struct HttpState {
    cache: HashMap<String, Http>,
    queue: VecDeque<String>,
    running: usize,
}

#[derive(Default)]
pub struct HttpLoader {
    st: Arc<Mutex<HttpState>>,
}

impl HttpLoader {
    pub const ID: &'static str = egui::generate_loader_id!(HttpLoader);
}

const MAX_DOWNLOADS: usize = 6;

/// Start downloads from the queue (most recently requested first).
fn pump(st: &Arc<Mutex<HttpState>>, ctx: &egui::Context) {
    let mut s = st.lock();
    while s.running < MAX_DOWNLOADS {
        let Some(uri) = s.queue.pop_back() else { break };
        if !matches!(s.cache.get(&uri), Some(Http::Queued)) {
            continue; // forgotten while it waited in the queue
        }
        s.cache.insert(uri.clone(), Http::Running);
        s.running += 1;
        let fetch = strip_markers(&uri).to_string();
        let (st2, ctx2) = (st.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(fetch), move |r| {
            let entry = match r {
                Ok(r) if r.ok => Http::Ready(r.bytes.clone().into(), r.content_type().map(str::to_owned)),
                Ok(r) => Http::Failed(format!("{} {}", r.status, r.status_text)),
                Err(e) => Http::Failed(e),
            };
            {
                let mut s = st2.lock();
                s.running -= 1;
                // The section was closed while downloading: the file is no longer needed, do not keep it.
                if matches!(s.cache.get(&uri), Some(Http::Running)) {
                    s.cache.insert(uri, entry);
                }
            }
            pump(&st2, &ctx2);
            ctx2.request_repaint();
        });
    }
}

impl BytesLoader for HttpLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str) -> BytesLoadResult {
        if !(uri.starts_with("http://") || uri.starts_with("https://")) {
            return Err(LoadError::NotSupported);
        }
        {
            let mut s = self.st.lock();
            match s.cache.get(uri).cloned() {
                Some(Http::Ready(bytes, mime)) => return Ok(BytesPoll::Ready { size: None, bytes: Bytes::Shared(bytes), mime }),
                Some(Http::Failed(e)) => return Err(LoadError::Loading(e)),
                Some(Http::Queued) => {
                    // Needed again: move to the front of the queue.
                    if let Some(i) = s.queue.iter().position(|u| u == uri) {
                        if i + 1 != s.queue.len() {
                            let u = s.queue.remove(i).unwrap();
                            s.queue.push_back(u);
                        }
                    }
                    return Ok(BytesPoll::Pending { size: None });
                }
                Some(Http::Running) => return Ok(BytesPoll::Pending { size: None }),
                None => {
                    s.cache.insert(uri.to_string(), Http::Queued);
                    s.queue.push_back(uri.to_string());
                }
            }
        }
        pump(&self.st, ctx);
        Ok(BytesPoll::Pending { size: None })
    }

    fn forget(&self, uri: &str) {
        let mut s = self.st.lock();
        s.cache.remove(uri);
        s.queue.retain(|u| u != uri);
    }

    fn forget_all(&self) {
        let mut s = self.st.lock();
        s.cache.clear();
        s.queue.clear();
    }

    fn byte_size(&self) -> usize {
        self.st
            .lock()
            .cache
            .values()
            .map(|e| match e {
                Http::Ready(b, _) => b.len(),
                _ => 0,
            })
            .sum()
    }
}

// ---------------------------------------------------------------- decoding

#[derive(Clone)]
enum Entry {
    Pending,
    Ready(Arc<ColorImage>),
    /// Handed to egui (it made a texture) - own copy dropped. small - a small one (avatar, icon).
    Delivered { small: bool },
    Failed(String),
}

struct Job {
    uri: String,
    bytes: Bytes,
    big: bool,
    /// Limit of the larger side.
    max: u32,
}

pub struct SmallLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
    jobs: Arc<Mutex<VecDeque<Job>>>,
    wake: std::sync::mpsc::Sender<()>,
}

impl SmallLoader {
    pub const ID: &'static str = egui::generate_loader_id!(SmallLoader);

    fn new(ctx: &egui::Context) -> Self {
        let cache: Arc<Mutex<HashMap<String, Entry>>> = Default::default();
        let jobs: Arc<Mutex<VecDeque<Job>>> = Default::default();
        let (wake, rx) = std::sync::mpsc::channel::<()>();
        let rx = Arc::new(std::sync::Mutex::new(rx));
        for _ in 0..2 {
            let (cache, jobs, ctx, rx) = (cache.clone(), jobs.clone(), ctx.clone(), rx.clone());
            std::thread::Builder::new()
                .name("vaporust-images".into())
                .spawn(move || loop {
                    // Most recently requested first (visible now).
                    let job = jobs.lock().pop_back();
                    let Some(job) = job else {
                        if rx.lock().unwrap().recv().is_err() {
                            return;
                        }
                        continue;
                    };
                    if !matches!(cache.lock().get(&job.uri), Some(Entry::Pending)) {
                        continue; // no longer needed
                    }
                    let e = match decode_max(&job.bytes, job.max) {
                        Ok(img) => Entry::Ready(Arc::new(img)),
                        Err(err) => Entry::Failed(err),
                    };
                    drop(job.bytes);
                    {
                        let mut c = cache.lock();
                        if matches!(c.get(&job.uri), Some(Entry::Pending)) {
                            c.insert(job.uri.clone(), e);
                        }
                    }
                    // The large source file is no longer needed (the image is already decoded and downscaled).
                    if job.big {
                        for l in ctx.loaders().bytes.lock().iter() {
                            l.forget(&job.uri);
                        }
                    }
                    ctx.request_repaint();
                })
                .expect(tr!("поток картинок", "image stream"));
        }
        let _ = SMALL_CACHE.set(cache.clone());
        SmallLoader { cache, jobs, wake }
    }
}

/// egui requests a GIF frame as "url#number" - that is not ours.
fn is_frame_uri(uri: &str) -> bool {
    uri.rsplit_once('#').is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Decode and downscale (larger side at most max_side()).
#[cfg(test)]
pub fn decode_small(bytes: &[u8]) -> Result<ColorImage, String> {
    decode_max(bytes, max_side())
}

/// Decode and downscale to `side` on the larger side.
fn decode_max(bytes: &[u8], side: u32) -> Result<ColorImage, String> {
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let img = if img.width().max(img.height()) > side {
        img.resize(side, side, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Ok(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

impl ImageLoader for SmallLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str, _: SizeHint) -> ImageLoadResult {
        if !(uri.starts_with("http://") || uri.starts_with("https://") || uri.starts_with("file://")) || is_frame_uri(uri) {
            return Err(LoadError::NotSupported);
        }
        if uri.to_ascii_lowercase().split('?').next().is_some_and(|p| p.ends_with(".svg")) {
            return Err(LoadError::NotSupported);
        }
        {
            let mut c = self.cache.lock();
            match c.get(uri).cloned() {
                Some(Entry::Pending) => return Ok(ImagePoll::Pending { size: None }),
                Some(Entry::Ready(image)) => {
                    // egui will make a texture from it and keep it itself: we do not keep our copy.
                    let small = image.size[0].max(image.size[1]) <= SMALL;
                    c.insert(uri.to_string(), Entry::Delivered { small });
                    return Ok(ImagePoll::Ready { image });
                }
                Some(Entry::Failed(err)) => return Err(LoadError::Loading(err)),
                // The texture was forgotten (or is needed with other settings) - decode again.
                Some(Entry::Delivered { .. }) | None => {}
            }
        }
        match ctx.try_load_bytes(uri) {
            Ok(BytesPoll::Ready { bytes, mime, .. }) => {
                if mime.as_deref().is_some_and(|m| m.contains("svg")) {
                    return Err(LoadError::NotSupported);
                }
                // A GIF without "#frame" in the URL is shown still: take the first frame (Workshop
                // previews can be 4 MB GIFs). Animation (URL "...gif#N") is handled by egui_extras.
                let Ok(format) = image::guess_format(&bytes) else { return Err(LoadError::NotSupported) };
                // The chat (sticker animation, anim.rs) will need the bytes again - it downloads them itself.
                let big = bytes.len() > 200_000;
                let _ = format;
                self.cache.lock().insert(uri.to_string(), Entry::Pending);
                let max = max_from(uri).map_or(max_side(), |m| m.min(max_side()));
                // Do not keep a large source just for a thumbnail either.
                let big = big || max_from(uri).is_some();
                self.jobs.lock().push_back(Job { uri: uri.to_string(), bytes, big, max });
                let _ = self.wake.send(());
                Ok(ImagePoll::Pending { size: None })
            }
            Ok(BytesPoll::Pending { size }) => Ok(ImagePoll::Pending { size }),
            Err(err) => Err(err),
        }
    }

    fn forget(&self, uri: &str) {
        self.cache.lock().remove(uri);
        self.jobs.lock().retain(|j| j.uri != uri);
    }

    fn forget_all(&self) {
        self.cache.lock().clear();
        self.jobs.lock().clear();
    }

    fn byte_size(&self) -> usize {
        let c: usize = self
            .cache
            .lock()
            .values()
            .map(|e| match e {
                Entry::Ready(i) => i.pixels.len() * 4,
                _ => 0,
            })
            .sum();
        c + self.jobs.lock().iter().map(|j| j.bytes.len()).sum::<usize>()
    }
}

/// Install the loaders (after egui_extras::install_image_loaders: the last added one
/// is asked first).
pub fn install(ctx: &egui::Context) {
    let http = HttpLoader::default();
    let _ = HTTP_STATE.set(http.st.clone());
    ctx.add_bytes_loader(Arc::new(http));
    ctx.add_image_loader(Arc::new(SmallLoader::new(ctx)));
}

/// How much memory the egui image caches take: downloaded files, images, textures.
pub fn cache_bytes(ctx: &egui::Context) -> usize {
    let l = ctx.loaders();
    let b: usize = l.bytes.lock().iter().map(|x| x.byte_size()).sum();
    let i: usize = l.image.lock().iter().map(|x| x.byte_size()).sum();
    let t: usize = l.texture.lock().iter().map(|x| x.byte_size()).sum();
    b + i + t
}

/// Every few seconds: if the image cache grew over the limit - clear it
/// (visible ones reload right away, one image at a time).
pub fn budget_tick(ctx: &egui::Context) {
    thread_local! {
        static LAST: std::cell::Cell<f64> = const { std::cell::Cell::new(0.0) };
    }
    let now = ctx.input(|i| i.time);
    if LAST.with(|l| now - l.get() < 5.0) {
        return;
    }
    LAST.with(|l| l.set(now));
    let bytes = cache_bytes(ctx);
    LAST_BYTES.store(bytes as u64, Ordering::Relaxed);
    if bytes > budget() {
        forget_large(ctx);
        if cache_bytes(ctx) > budget() {
            ctx.forget_all_images();
        }
    }
}

/// Give back to the system the memory Vaporust is not using now (window in the tray, a game is running):
/// pages leave the working set and come back by themselves when needed.
pub fn trim_working_set() {
    unsafe {
        windows_sys::Win32::System::ProcessStatus::K32EmptyWorkingSet(
            windows_sys::Win32::System::Threading::GetCurrentProcess(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_uri() {
        assert!(is_frame_uri("https://x/a.gif#0"));
        assert!(is_frame_uri("https://x/a.gif#12"));
        assert!(!is_frame_uri("https://x/a.png"));
        assert!(!is_frame_uri("https://x/a#top"));
    }

    #[test]
    fn shrinks_big_image() {
        let big = image::RgbaImage::from_pixel(1920, 1080, image::Rgba([10, 20, 30, 255]));
        let mut buf = std::io::Cursor::new(Vec::new());
        big.write_to(&mut buf, image::ImageFormat::Png).unwrap();
        let img = decode_small(&buf.into_inner()).unwrap();
        assert_eq!(img.size, [800, 450]);
    }

    #[test]
    fn still_marker() {
        ANIMATIONS.store(false, Ordering::Relaxed);
        assert_eq!(still_if_needed("https://x/a.gif"), "https://x/a.gif?vaporust-still");
        assert_eq!(still_if_needed("https://x/a.png"), "https://x/a.png");
        ANIMATIONS.store(true, Ordering::Relaxed);
        assert_eq!(still_if_needed("https://x/a.gif"), "https://x/a.gif");
    }
}

/// On section change: unload large images, but keep small ones (friend avatars, game icons,
/// emoticons) - they are needed almost everywhere and take little space.
pub fn forget_large(ctx: &egui::Context) {
    let mut keep = std::collections::HashSet::new();
    let mut drop: Vec<String> = Vec::new();
    if let Some(c) = SMALL_CACHE.get() {
        for (uri, e) in c.lock().iter() {
            match e {
                Entry::Delivered { small: true } => {
                    keep.insert(uri.clone());
                }
                Entry::Ready(i) if i.size[0].max(i.size[1]) <= SMALL => {
                    keep.insert(uri.clone());
                }
                _ => drop.push(uri.clone()),
            }
        }
    }
    if let Some(h) = HTTP_STATE.get() {
        drop.extend(h.lock().cache.keys().filter(|u| !keep.contains(*u)).cloned());
    }
    drop.sort();
    drop.dedup();
    for uri in drop {
        ctx.forget_image(&uri);
    }
}
