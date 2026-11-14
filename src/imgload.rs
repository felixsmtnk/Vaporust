//! Загрузка картинок для egui: скачивание, раскодирование с уменьшением в фоне, экономия памяти.
//!
//! Обычные загрузчики egui_extras раскодируют картинку целиком в главном потоке и держат в
//! памяти сразу три копии: скачанный файл, раскодированную картинку и текстуру. Здесь:
//! - HttpLoader скачивает не больше 6 файлов разом (сначала то, что запрошено последним, то есть
//!   видимое сейчас) и не сохраняет файлы, докачавшиеся уже после ухода из раздела;
//! - SmallLoader раскодирует в двух фоновых потоках (а не в потоке на каждую картинку: десяток
//!   GIF по 4 МБ одновременно раздувал память на сотню мегабайт) и уменьшает так, чтобы большая
//!   сторона была не больше max_side(); после того как egui сделал из картинки текстуру, своя
//!   копия выбрасывается.
//! Анимированные GIF (адрес «...gif#кадр») и SVG отдаем загрузчикам egui_extras.
//!
//! Кэш картинок чистится при переходе между разделами и при превышении предела (budget_tick).

use eframe::egui;
use egui::load::{Bytes, BytesLoadResult, BytesLoader, BytesPoll, ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::mutex::Mutex;
use egui::ColorImage;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// Режим экономии памяти (настройка «Производительность»): картинки мельче, кэш меньше.
pub static ECONOMY: AtomicBool = AtomicBool::new(false);
/// Анимации (стикеры, GIF, эффекты чата, значок загрузки). Выключаются в настройках.
pub static ANIMATIONS: AtomicBool = AtomicBool::new(true);
/// Картинки не больше этого (аватары, значки игр, смайлики) не выгружаются при смене раздела.
const SMALL: usize = 128;
/// Кэш раскодированных картинок (для выборочной выгрузки).
static SMALL_CACHE: std::sync::OnceLock<Arc<Mutex<HashMap<String, Entry>>>> = std::sync::OnceLock::new();
/// Состояние скачивания (для выборочной выгрузки).
static HTTP_STATE: std::sync::OnceLock<Arc<Mutex<HttpState>>> = std::sync::OnceLock::new();

/// Размер кэша картинок при последней проверке (для журнала замеров stats.rs).
pub static LAST_BYTES: AtomicU64 = AtomicU64::new(0);

/// Большая сторона картинки в памяти, пикселей (в программе нигде не показываем крупнее).
pub fn max_side() -> u32 {
    if ECONOMY.load(Ordering::Relaxed) { 512 } else { 800 }
}

/// Предел памяти кэша картинок egui, после которого кэш чистится.
fn budget() -> usize {
    if ECONOMY.load(Ordering::Relaxed) { 20 << 20 } else { 48 << 20 }
}

pub fn animations() -> bool {
    ANIMATIONS.load(Ordering::Relaxed)
}

/// Пометка в конце адреса: показать GIF неподвижным (egui анимирует адреса, оканчивающиеся на
/// «.gif»). HttpLoader убирает ее перед скачиванием.
const STILL: &str = "?vaporust-still";
/// Метка «уменьшить до N точек по большей стороне» (миниатюры крупных картинок: фоны профиля).
const MAX: &str = "?vaporust-max=";

/// Адрес картинки, которую достаточно показать не больше `px` точек по большей стороне.
pub fn sized(url: &str, px: u32) -> String {
    format!("{url}{MAX}{px}")
}

/// Настоящий адрес для скачивания: без меток Vaporust.
fn strip_markers(uri: &str) -> &str {
    uri.find("?vaporust-").map_or(uri, |i| &uri[..i])
}

/// Предел стороны из метки адреса (если есть).
fn max_from(uri: &str) -> Option<u32> {
    let i = uri.find(MAX)?;
    uri[i + MAX.len()..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// Адрес GIF, который всегда показывается неподвижным (сетки предметов: десятки анимаций сразу
/// заняли бы сотни мегабайт).
pub fn still(url: &str) -> String {
    if url.to_ascii_lowercase().ends_with(".gif") {
        format!("{url}{STILL}")
    } else {
        url.to_string()
    }
}

/// Адрес картинки с учетом настройки анимаций: при выключенных анимациях GIF будет неподвижным.
pub fn still_if_needed(url: &str) -> String {
    if !animations() && url.to_ascii_lowercase().ends_with(".gif") {
        format!("{url}{STILL}")
    } else {
        url.to_string()
    }
}

// ---------------------------------------------------------------- скачивание

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

/// Запустить скачивания из очереди (последние запрошенные - первыми).
fn pump(st: &Arc<Mutex<HttpState>>, ctx: &egui::Context) {
    let mut s = st.lock();
    while s.running < MAX_DOWNLOADS {
        let Some(uri) = s.queue.pop_back() else { break };
        if !matches!(s.cache.get(&uri), Some(Http::Queued)) {
            continue; // забыли, пока ждала в очереди
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
                // Раздел закрыли, пока качали: файл больше не нужен, не храним.
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
                    // Снова нужна: поднять в начало очереди.
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

// ---------------------------------------------------------------- раскодирование

#[derive(Clone)]
enum Entry {
    Pending,
    Ready(Arc<ColorImage>),
    /// Отдана egui (он сделал текстуру) - своя копия выброшена. small - мелкая (аватар, значок).
    Delivered { small: bool },
    Failed(String),
}

struct Job {
    uri: String,
    bytes: Bytes,
    big: bool,
    /// Предел большей стороны.
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
                    // Последние запрошенные - первыми (видимое сейчас).
                    let job = jobs.lock().pop_back();
                    let Some(job) = job else {
                        if rx.lock().unwrap().recv().is_err() {
                            return;
                        }
                        continue;
                    };
                    if !matches!(cache.lock().get(&job.uri), Some(Entry::Pending)) {
                        continue; // уже не нужна
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
                    // Крупный исходный файл больше не нужен (картинка уже раскодирована и уменьшена).
                    if job.big {
                        for l in ctx.loaders().bytes.lock().iter() {
                            l.forget(&job.uri);
                        }
                    }
                    ctx.request_repaint();
                })
                .expect("поток картинок");
        }
        let _ = SMALL_CACHE.set(cache.clone());
        SmallLoader { cache, jobs, wake }
    }
}

/// Кадр GIF egui запрашивает как «адрес#номер» - это не наше.
fn is_frame_uri(uri: &str) -> bool {
    uri.rsplit_once('#').is_some_and(|(_, n)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Раскодировать и уменьшить (большая сторона не больше max_side()).
#[cfg(test)]
pub fn decode_small(bytes: &[u8]) -> Result<ColorImage, String> {
    decode_max(bytes, max_side())
}

/// Раскодировать и уменьшить до `side` по большей стороне.
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
                    // egui сделает из нее текстуру и будет держать ее сам: свою копию не храним.
                    let small = image.size[0].max(image.size[1]) <= SMALL;
                    c.insert(uri.to_string(), Entry::Delivered { small });
                    return Ok(ImagePoll::Ready { image });
                }
                Some(Entry::Failed(err)) => return Err(LoadError::Loading(err)),
                // Текстуру забыли (или нужна с другими настройками) - раскодируем заново.
                Some(Entry::Delivered { .. }) | None => {}
            }
        }
        match ctx.try_load_bytes(uri) {
            Ok(BytesPoll::Ready { bytes, mime, .. }) => {
                if mime.as_deref().is_some_and(|m| m.contains("svg")) {
                    return Err(LoadError::NotSupported);
                }
                // GIF без «#кадр» в адресе показывается неподвижным: берем первый кадр (превью
                // Мастерской бывают GIF по 4 МБ). Анимацию (адрес «...gif#N») ведет egui_extras.
                let Ok(format) = image::guess_format(&bytes) else { return Err(LoadError::NotSupported) };
                // Чату (анимация стикеров, anim.rs) байты понадобятся снова - он скачает их сам.
                let big = bytes.len() > 200_000;
                let _ = format;
                self.cache.lock().insert(uri.to_string(), Entry::Pending);
                let max = max_from(uri).map_or(max_side(), |m| m.min(max_side()));
                // Крупный исходник ради миниатюры тоже не храним.
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

/// Подключить загрузчики (после egui_extras::install_image_loaders: последний добавленный
/// спрашивается первым).
pub fn install(ctx: &egui::Context) {
    let http = HttpLoader::default();
    let _ = HTTP_STATE.set(http.st.clone());
    ctx.add_bytes_loader(Arc::new(http));
    ctx.add_image_loader(Arc::new(SmallLoader::new(ctx)));
}

/// Сколько памяти занимают кэши картинок egui: скачанные файлы, картинки, текстуры.
pub fn cache_bytes(ctx: &egui::Context) -> usize {
    let l = ctx.loaders();
    let b: usize = l.bytes.lock().iter().map(|x| x.byte_size()).sum();
    let i: usize = l.image.lock().iter().map(|x| x.byte_size()).sum();
    let t: usize = l.texture.lock().iter().map(|x| x.byte_size()).sum();
    b + i + t
}

/// Раз в несколько секунд: если кэш картинок вырос больше предела - очистить его
/// (видимое тут же загрузится заново, уже по одной картинке).
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

/// Отдать системе память, которую Vaporust сейчас не трогает (окно в трее, идет игра):
/// страницы уходят из рабочего набора и вернутся сами, когда понадобятся.
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

/// При смене раздела: выгрузить крупные картинки, а мелкие (аватары друзей, значки игр,
/// смайлики) оставить - они нужны почти везде и занимают мало.
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
