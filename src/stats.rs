//! Диагностика нагрузки (только при переменной окружения VAPORUST_STATS=1): раз в 10 с пишет в
//! %TEMP%\vaporust_stats.log, сколько было кадров, сколько времени заняли кадры и сколько раз
//! опрашивался список процессов. Для замеров на старом ПК; без переменной ничего не делает.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

static FRAMES: AtomicU64 = AtomicU64::new(0);
static FRAME_US: AtomicU64 = AtomicU64::new(0);
static PROC_SCANS: AtomicU64 = AtomicU64::new(0);
static PROC_US: AtomicU64 = AtomicU64::new(0);
static ON: OnceLock<bool> = OnceLock::new();
static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

pub fn on() -> bool {
    *ON.get_or_init(|| std::env::var_os("VAPORUST_STATS").is_some())
}

pub fn frame(started: Instant) {
    if !on() {
        return;
    }
    FRAMES.fetch_add(1, Ordering::Relaxed);
    FRAME_US.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    let mut last = LAST.lock().unwrap();
    let t = *last.get_or_insert_with(Instant::now);
    if t.elapsed().as_secs() >= 10 {
        *last = Some(Instant::now());
        let line = format!(
            "{} кадров, {:.1} мс на кадры; опросов процессов {}, {:.1} мс; кэш картинок {:.1} МБ\n",
            FRAMES.swap(0, Ordering::Relaxed),
            FRAME_US.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            PROC_SCANS.swap(0, Ordering::Relaxed),
            PROC_US.swap(0, Ordering::Relaxed) as f64 / 1000.0,
            crate::imgload::LAST_BYTES.load(Ordering::Relaxed) as f64 / 1048576.0,
        );
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(std::env::temp_dir().join("vaporust_stats.log"))
        {
            let _ = f.write_all(line.as_bytes());
        }
    }
}

pub fn proc_scan(started: Instant) {
    if on() {
        PROC_SCANS.fetch_add(1, Ordering::Relaxed);
        PROC_US.fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
}

/// Память своего процесса: (то, что показывает Диспетчер задач - частный рабочий набор,
/// выделенная частная память), байты. На старых Windows 10 без нужного счетчика вместо
/// первого числа - весь рабочий набор.
pub fn memory() -> (u64, u64) {
    use windows_sys::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX, PROCESS_MEMORY_COUNTERS_EX2};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut c: PROCESS_MEMORY_COUNTERS_EX2 = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX2>() as u32;
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb) };
    if ok != 0 {
        return (c.PrivateWorkingSetSize as u64, c.PrivateUsage as u64);
    }
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut PROCESS_MEMORY_COUNTERS, c.cb) };
    if ok == 0 {
        return (0, 0);
    }
    (c.WorkingSetSize as u64, c.PrivateUsage as u64)
}
