//! Read-only Windows sampler for the v1 diagnostic milestone.
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, ProcessRefreshKind, ProcessesToUpdate, System};

const MIB: f64 = 1_048_576.0;
const PDH_FMT_DOUBLE: u32 = 0x0000_0200;
const FOREGROUND_TIMEOUT_MS: u32 = 50;

type PdhHandle = isize;

#[repr(C)]
struct PdhDouble {
    status: u32,
    value: f64,
}

type OpenQuery = unsafe extern "system" fn(*const u16, usize, *mut PdhHandle) -> i32;
type AddCounter = unsafe extern "system" fn(PdhHandle, *const u16, usize, *mut PdhHandle) -> i32;
type Collect = unsafe extern "system" fn(PdhHandle) -> i32;
type ReadCounter = unsafe extern "system" fn(PdhHandle, u32, *mut u32, *mut PdhDouble) -> i32;
type CloseQuery = unsafe extern "system" fn(PdhHandle) -> i32;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(name: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const u8) -> *const c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: *mut c_void,
    ) -> isize;
    fn DeviceIoControl(
        device: isize,
        code: u32,
        input: *const c_void,
        input_size: u32,
        output: *mut c_void,
        output_size: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn CloseHandle(handle: isize) -> i32;
    fn GetSystemTime(time: *mut WinSystemTime);
}

#[repr(C)]
#[derive(Default)]
struct WinSystemTime {
    year: u16,
    month: u16,
    day_of_week: u16,
    day: u16,
    hour: u16,
    minute: u16,
    second: u16,
    milliseconds: u16,
}

fn utc_now() -> String {
    let mut time = WinSystemTime::default();
    // SAFETY: GetSystemTime fills the writable SYSTEMTIME structure.
    unsafe { GetSystemTime(&mut time) };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        time.year, time.month, time.day, time.hour, time.minute, time.second
    )
}

fn system_disk_bus() -> Option<&'static str> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    if drive.len() != 2
        || !drive.ends_with(':')
        || !drive.starts_with(|c: char| c.is_ascii_alphabetic())
    {
        return None;
    }
    let volume = format!(r"\\.\{drive}");
    let wide: Vec<u16> = volume.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the volume path is null-terminated and handle is checked below.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            0,
            0x3,
            std::ptr::null(),
            3,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == -1 {
        return None;
    }
    let query = [0_u32; 3]; // StorageDeviceProperty, PropertyStandardQuery.
    let mut output = [0_u8; 512];
    let mut returned = 0;
    // IOCTL_STORAGE_QUERY_PROPERTY is a read-only request.
    // SAFETY: buffers and sizes match the documented STORAGE_PROPERTY_QUERY and descriptor.
    let ok = unsafe {
        DeviceIoControl(
            handle,
            0x002D_1400,
            query.as_ptr().cast(),
            std::mem::size_of_val(&query) as u32,
            output.as_mut_ptr().cast(),
            output.len() as u32,
            &mut returned,
            std::ptr::null_mut(),
        )
    };
    // SAFETY: handle is valid and owned by this function.
    unsafe { CloseHandle(handle) };
    if ok == 0 || returned < 32 {
        return None;
    }
    let bus = u32::from_le_bytes(output[28..32].try_into().ok()?);
    Some(match bus {
        3 => "ATA",
        7 => "USB",
        11 => "SATA",
        14 => "Virtual",
        17 => "NVMe",
        _ => "other/unknown bus",
    })
}

struct PdhApi {
    module: *mut c_void,
    open_query: OpenQuery,
    add_counter: AddCounter,
    collect: Collect,
    read_counter: ReadCounter,
    close_query: CloseQuery,
}

impl PdhApi {
    fn load() -> Option<Self> {
        let wide: Vec<u16> = "pdh.dll".encode_utf16().chain(std::iter::once(0)).collect();
        // LOAD_LIBRARY_SEARCH_SYSTEM32 prevents loading a DLL from the working directory.
        // SAFETY: null-terminated DLL name and normal Windows loader contract.
        let module = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), 0x0000_0800) };
        if module.is_null() {
            return None;
        }
        macro_rules! symbol {
            ($name:literal, $ty:ty) => {{
                // SAFETY: module remains loaded until PdhApi is dropped.
                let pointer = unsafe { GetProcAddress(module, concat!($name, "\0").as_ptr()) };
                if pointer.is_null() {
                    // SAFETY: module came from LoadLibraryExW and no handles have been opened yet.
                    unsafe { FreeLibrary(module) };
                    return None;
                }
                // SAFETY: the symbol name and ABI exactly match the documented PDH function.
                unsafe { std::mem::transmute::<*const c_void, $ty>(pointer) }
            }};
        }
        Some(Self {
            module,
            open_query: symbol!("PdhOpenQueryW", OpenQuery),
            add_counter: symbol!("PdhAddEnglishCounterW", AddCounter),
            collect: symbol!("PdhCollectQueryData", Collect),
            read_counter: symbol!("PdhGetFormattedCounterValue", ReadCounter),
            close_query: symbol!("PdhCloseQuery", CloseQuery),
        })
    }
}

impl Drop for PdhApi {
    fn drop(&mut self) {
        // SAFETY: module was loaded once by LoadLibraryExW.
        unsafe { FreeLibrary(self.module) };
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> *mut c_void;
    fn SendMessageTimeoutW(
        window: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        flags: u32,
        timeout: u32,
        result: *mut usize,
    ) -> isize;
}

struct PdhSampler {
    api: Option<PdhApi>,
    query: PdhHandle,
    counters: BTreeMap<&'static str, PdhHandle>,
    missing: Vec<&'static str>,
}

impl PdhSampler {
    fn new() -> Self {
        let Some(api) = PdhApi::load() else {
            return Self {
                api: None,
                query: 0,
                counters: BTreeMap::new(),
                missing: vec!["all PDH counters (pdh.dll unavailable)"],
            };
        };
        let mut query = 0;
        // SAFETY: null source selects the local system, and query is a writable output handle.
        let status = unsafe { (api.open_query)(std::ptr::null(), 0, &mut query) };
        if status != 0 {
            return Self {
                api: Some(api),
                query: 0,
                counters: BTreeMap::new(),
                missing: vec!["all PDH counters (query unavailable)"],
            };
        }
        let mut sampler = Self {
            api: Some(api),
            query,
            counters: BTreeMap::new(),
            missing: Vec::new(),
        };
        for (name, path) in [
            ("commit", r"\Memory\Committed Bytes"),
            ("commit_limit", r"\Memory\Commit Limit"),
            ("page_reads", r"\Memory\Page Reads/sec"),
            ("pages_input", r"\Memory\Pages Input/sec"),
            (
                "disk_queue",
                r"\PhysicalDisk(_Total)\Avg. Disk Queue Length",
            ),
            ("disk_busy", r"\PhysicalDisk(_Total)\% Disk Time"),
        ] {
            let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
            let mut counter = 0;
            // SAFETY: query is valid until Drop; wide is null-terminated and counter is writable.
            if unsafe {
                (sampler.api.as_ref().unwrap().add_counter)(query, wide.as_ptr(), 0, &mut counter)
            } == 0
            {
                sampler.counters.insert(name, counter);
            } else {
                sampler.missing.push(name);
            }
        }
        sampler
    }

    fn collect(&self) -> bool {
        // SAFETY: query remains open for the lifetime of this sampler.
        self.query != 0 && unsafe { (self.api.as_ref().unwrap().collect)(self.query) } == 0
    }

    fn value(&self, name: &'static str) -> Option<f64> {
        let counter = *self.counters.get(name)?;
        let mut result = PdhDouble {
            status: u32::MAX,
            value: 0.0,
        };
        // SAFETY: counter belongs to the open query and result has the PDH double layout.
        let status = unsafe {
            (self.api.as_ref()?.read_counter)(
                counter,
                PDH_FMT_DOUBLE,
                std::ptr::null_mut(),
                &mut result,
            )
        };
        (status == 0 && result.status <= 1 && result.value.is_finite() && result.value >= 0.0)
            .then_some(result.value)
    }
}

impl Drop for PdhSampler {
    fn drop(&mut self) {
        if self.query != 0 {
            // SAFETY: query was opened by PdhOpenQueryW and has not been closed yet.
            unsafe { (self.api.as_ref().unwrap().close_query)(self.query) };
        }
    }
}

#[derive(Default)]
struct Probe {
    latency_ms: Option<f64>,
    stalled: bool,
}

fn probe_foreground() -> Probe {
    // SAFETY: GetForegroundWindow returns a borrowed HWND, checked for null below.
    let window = unsafe { GetForegroundWindow() };
    if window.is_null() {
        return Probe::default();
    }
    let start = Instant::now();
    let mut result = 0;
    // WM_NULL does not modify the target. SMTO_ABORTIFHUNG | SMTO_ERRORONEXIT.
    // SAFETY: window is the current foreground HWND; result is a writable output value.
    let ok = unsafe {
        SendMessageTimeoutW(
            window,
            0,
            0,
            0,
            0x0002 | 0x0020,
            FOREGROUND_TIMEOUT_MS,
            &mut result,
        )
    };
    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
    if ok != 0 {
        Probe {
            latency_ms: Some(elapsed),
            stalled: false,
        }
    } else {
        Probe {
            latency_ms: None,
            stalled: elapsed >= f64::from(FOREGROUND_TIMEOUT_MS),
        }
    }
}

#[derive(Default)]
struct Summary {
    cpu: Vec<f64>,
    commit_percent: Vec<f64>,
    available_mib: Vec<f64>,
    page_reads: Vec<f64>,
    pages_input: Vec<f64>,
    disk_queue: Vec<f64>,
    disk_busy: Vec<f64>,
    latency: Vec<f64>,
    stalls: usize,
    probe_unavailable: usize,
    own_cpu: Vec<f64>,
    own_peak_mib: f64,
    own_io_bytes: u64,
    process_cpu: BTreeMap<String, f64>,
}

impl Summary {
    fn add(&mut self, sample: &Sample) {
        push_opt(&mut self.cpu, sample.cpu);
        push_opt(&mut self.commit_percent, sample.commit_percent);
        self.available_mib.push(sample.available_mib);
        push_opt(&mut self.page_reads, sample.page_reads);
        push_opt(&mut self.pages_input, sample.pages_input);
        push_opt(&mut self.disk_queue, sample.disk_queue);
        push_opt(&mut self.disk_busy, sample.disk_busy);
        if sample.probe.stalled {
            self.stalls += 1;
        } else if let Some(latency) = sample.probe.latency_ms {
            self.latency.push(latency);
        } else {
            self.probe_unavailable += 1;
        }
        push_opt(&mut self.own_cpu, sample.own_cpu);
        self.own_peak_mib = self.own_peak_mib.max(sample.own_mib);
        self.own_io_bytes += sample.own_io_bytes;
        for (name, cpu) in &sample.process_cpu {
            *self.process_cpu.entry(name.clone()).or_default() += cpu;
        }
    }
}

fn push_opt(values: &mut Vec<f64>, value: Option<f64>) {
    if let Some(value) = value {
        values.push(value);
    }
}

struct Sample {
    cpu: Option<f64>,
    commit_percent: Option<f64>,
    available_mib: f64,
    page_reads: Option<f64>,
    pages_input: Option<f64>,
    disk_queue: Option<f64>,
    disk_busy: Option<f64>,
    probe: Probe,
    own_cpu: Option<f64>,
    own_mib: f64,
    own_io_bytes: u64,
    process_cpu: Vec<(String, f64)>,
}

fn read_sample(system: &mut System, pdh: &PdhSampler) -> Sample {
    system.refresh_cpu_usage();
    system.refresh_memory();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cpu()
            .with_memory()
            .with_disk_usage(),
    );
    let pdh_ok = pdh.collect();
    let metric = |name| pdh_ok.then(|| pdh.value(name)).flatten();
    let commit = metric("commit");
    let limit = metric("commit_limit");
    let logical = system.cpus().len().max(1) as f64;
    let mut process_cpu = Vec::new();
    for process in system.processes().values() {
        let cpu = f64::from(process.cpu_usage()) / logical;
        if cpu >= 0.2 {
            process_cpu.push((process.name().to_string_lossy().into_owned(), cpu));
        }
    }
    let own = system.process(sysinfo::Pid::from_u32(std::process::id()));
    Sample {
        cpu: Some(f64::from(system.global_cpu_usage())),
        commit_percent: commit
            .zip(limit)
            .and_then(|(used, total)| (total > 0.0).then_some(used * 100.0 / total)),
        available_mib: system.available_memory() as f64 / MIB,
        page_reads: metric("page_reads"),
        pages_input: metric("pages_input"),
        disk_queue: metric("disk_queue"),
        disk_busy: metric("disk_busy"),
        probe: probe_foreground(),
        own_cpu: own.map(|p| f64::from(p.cpu_usage()) / logical),
        own_mib: own.map_or(0.0, |p| p.memory() as f64 / MIB),
        own_io_bytes: own.map_or(0, |p| {
            let io = p.disk_usage();
            io.read_bytes + io.written_bytes
        }),
        process_cpu,
    }
}

fn average(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn percentile(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let mut ordered = values.to_vec();
    ordered.sort_by(f64::total_cmp);
    let index = ((ordered.len() - 1) as f64 * percentile).ceil() as usize;
    ordered.get(index).copied()
}

fn display(value: Option<f64>) -> String {
    value.map_or_else(|| "n/a".into(), |number| format!("{number:.1}"))
}

pub fn run(seconds: u64) -> Result<(), String> {
    if !(3..=60).contains(&seconds) {
        return Err("report duration must be 3-60 seconds".into());
    }
    let mut system = System::new().map_err(|e| e.to_string())?;
    system.refresh_cpu_list(CpuRefreshKind::everything());
    system.refresh_memory();
    let system_mount = format!(
        "{}\\",
        std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into())
    );
    let disk_kind = Disks::new_with_refreshed_list()
        .ok()
        .and_then(|disks| {
            disks
                .iter()
                .find(|disk| {
                    disk.mount_point()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&system_mount)
                })
                .map(|disk| format!("{:?}", disk.kind()))
        })
        .unwrap_or_else(|| "unknown".into());
    let disk_bus = system_disk_bus().unwrap_or("unknown bus");
    println!("Memory Pilot diagnostic report — read-only");
    println!("Time: {}", utc_now());
    println!(
        "Hardware: {:.1} GiB RAM | {} | {} physical / {} logical cores | disk {} ({})",
        system.total_memory() as f64 / (MIB * 1024.0),
        system
            .cpus()
            .first()
            .map_or("unknown CPU", |cpu| cpu.brand()),
        System::physical_core_count().unwrap_or(0),
        system.cpus().len(),
        disk_kind,
        disk_bus
    );
    println!(
        "Windows: {}",
        System::long_os_version().unwrap_or_else(|_| "unknown".into())
    );
    let pdh = PdhSampler::new();
    if !pdh.missing.is_empty() {
        println!("Unavailable PDH counters: {}", pdh.missing.join(", "));
    }
    // Prime rate counters: their first PDH reading has no meaningful interval.
    pdh.collect();
    system.refresh_cpu_usage();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cpu(),
    );
    println!("s  CPU% Commit% AvailMiB Reads/s Input/s DiskQ Disk% UI-ms");
    let mut summary = Summary::default();
    let mut next = Instant::now();
    for second in 1..=seconds {
        next += Duration::from_secs(1);
        std::thread::sleep(next.saturating_duration_since(Instant::now()));
        let sample = read_sample(&mut system, &pdh);
        println!(
            "{second:>2} {:>5} {:>7} {:>8.0} {:>7} {:>7} {:>5} {:>5} {:>6}",
            display(sample.cpu),
            display(sample.commit_percent),
            sample.available_mib,
            display(sample.page_reads),
            display(sample.pages_input),
            display(sample.disk_queue),
            display(sample.disk_busy),
            if sample.probe.stalled {
                "STALL".into()
            } else {
                display(sample.probe.latency_ms)
            }
        );
        summary.add(&sample);
    }
    println!(
        "Summary: CPU avg {}% | commit avg {}% | available min {} MiB",
        display(average(&summary.cpu)),
        display(average(&summary.commit_percent)),
        display(summary.available_mib.iter().copied().reduce(f64::min))
    );
    println!(
        "Page reads avg {}/s | pages input avg {}/s | disk queue avg {} | disk busy avg {}%",
        display(average(&summary.page_reads)),
        display(average(&summary.pages_input)),
        display(average(&summary.disk_queue)),
        display(average(&summary.disk_busy))
    );
    println!(
        "Foreground WM_NULL: p95 {} ms | p99 {} ms | stalls {} | unavailable {}",
        display(percentile(&summary.latency, 0.95)),
        display(percentile(&summary.latency, 0.99)),
        summary.stalls,
        summary.probe_unavailable
    );
    println!(
        "Sampler: CPU avg {}% of whole PC | RAM peak {:.1} MiB | I/O {:.1} KiB in {} s",
        display(average(&summary.own_cpu)),
        summary.own_peak_mib,
        summary.own_io_bytes as f64 / 1024.0,
        seconds
    );
    let mut top: Vec<_> = summary.process_cpu.into_iter().collect();
    top.sort_by(|a, b| b.1.total_cmp(&a.1));
    println!("Top CPU groups (average % of whole PC):");
    for (name, total_cpu) in top.into_iter().take(8) {
        println!("  {name:<24} {:.1}%", total_cpu / seconds as f64);
    }
    println!("No process settings were changed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentile_handles_empty_and_tail() {
        assert_eq!(percentile(&[], 0.95), None);
        assert_eq!(percentile(&[1.0, 2.0, 3.0], 0.95), Some(3.0));
    }
}
