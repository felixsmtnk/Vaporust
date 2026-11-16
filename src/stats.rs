//! Load diagnostics (only with the VAPORUST_STATS=1 environment variable): every 10 s writes to
//! %TEMP%\vaporust_stats.log how many frames there were, how long they took and how many times
//! the process list was polled. For measurements on an old PC; does nothing without the variable.

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
        let line = trf!(
            "{} кадров, {:.1} мс на кадры; опросов процессов {}, {:.1} мс; кэш картинок {:.1} МБ\n", "{} frames, {:.1} ms on frames; process polls {}, {:.1} ms; image cache {:.1} MB\n",
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

/// Own process memory: (what Task Manager shows - the private working set,
/// committed private memory), bytes. On old Windows 10 builds without the needed counter the
/// first number is the whole working set.
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
