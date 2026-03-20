//! Count heap allocations when opening large ZIP files.
//!
//! Usage:
//!   cargo run --release --example memory_bench_alloc /tmp/bazel.jar

use std::alloc::{GlobalAlloc, Layout, System};
use std::fs;
use std::io::BufReader;
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static ALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_COUNT: AtomicUsize = AtomicUsize::new(0);
static DEALLOC_BYTES: AtomicUsize = AtomicUsize::new(0);
static TRACKING: AtomicUsize = AtomicUsize::new(0); // 0 = off, 1 = on

struct CountingAlloc;

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if TRACKING.load(Ordering::Relaxed) == 1 {
            ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            ALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if TRACKING.load(Ordering::Relaxed) == 1 {
            DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
            DEALLOC_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

fn reset_counters() {
    ALLOC_COUNT.store(0, Ordering::Relaxed);
    ALLOC_BYTES.store(0, Ordering::Relaxed);
    DEALLOC_COUNT.store(0, Ordering::Relaxed);
    DEALLOC_BYTES.store(0, Ordering::Relaxed);
}

fn snapshot() -> (usize, usize, usize, usize) {
    (
        ALLOC_COUNT.load(Ordering::Relaxed),
        ALLOC_BYTES.load(Ordering::Relaxed),
        DEALLOC_COUNT.load(Ordering::Relaxed),
        DEALLOC_BYTES.load(Ordering::Relaxed),
    )
}

fn fmt_bytes(b: usize) -> String {
    if b >= 1024 * 1024 {
        format!("{:.1} MiB", b as f64 / (1024.0 * 1024.0))
    } else if b >= 1024 {
        format!("{:.1} KiB", b as f64 / 1024.0)
    } else {
        format!("{b} B")
    }
}

fn print_stats(label: &str, num_entries: usize) {
    let (ac, ab, dc, db) = snapshot();
    let net_count = ac as isize - dc as isize;
    let net_bytes = ab as isize - db as isize;
    println!("  {label}:");
    println!("    allocs: {ac} ({})  deallocs: {dc} ({})", fmt_bytes(ab), fmt_bytes(db));
    println!(
        "    net: {net_count} allocs, {} bytes still held",
        fmt_bytes(net_bytes.unsigned_abs())
    );
    if num_entries > 0 {
        println!(
            "    per entry: {:.1} allocs, {:.0} bytes alloc'd, {:.0} net bytes",
            ac as f64 / num_entries as f64,
            ab as f64 / num_entries as f64,
            net_bytes as f64 / num_entries as f64
        );
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <zipfile>", args[0]);
        std::process::exit(1);
    }
    let path = &args[1];

    let file_size = fs::metadata(path)?.len();
    println!("File: {path}");
    println!("File size: {}", fmt_bytes(file_size as usize));

    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);

    // Phase 1: ZipArchive::new()
    TRACKING.store(1, Ordering::Relaxed);
    reset_counters();
    let mut archive = zip::ZipArchive::new(reader)?;
    TRACKING.store(0, Ordering::Relaxed);
    let num_entries = archive.len();
    println!("\nPhase 1: ZipArchive::new() [{num_entries} entries]");
    print_stats("Central directory parsing", num_entries);

    // Phase 2: by_index() iteration
    TRACKING.store(1, Ordering::Relaxed);
    reset_counters();
    for i in 0..num_entries {
        let _file = archive.by_index(i)?;
    }
    TRACKING.store(0, Ordering::Relaxed);
    println!("\nPhase 2: by_index() iteration (drop without reading)");
    print_stats("Per-entry reader creation", num_entries);

    // Phase 3: by_index_raw() iteration
    TRACKING.store(1, Ordering::Relaxed);
    reset_counters();
    for i in 0..num_entries {
        let _file = archive.by_index_raw(i)?;
    }
    TRACKING.store(0, Ordering::Relaxed);
    println!("\nPhase 3: by_index_raw() iteration");
    print_stats("Per-entry raw reader", num_entries);

    Ok(())
}
