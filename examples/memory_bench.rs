//! Reproduce and quantify memory overhead when opening large ZIP files.
//!
//! Usage:
//!   cargo run --release --example memory_bench /tmp/bazel.jar

use std::fs;
use std::io::BufReader;

fn rss_bytes() -> usize {
    let pid = std::process::id();
    // Works on macOS and Linux
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .expect("failed to run ps");
    let rss_kb: usize = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .unwrap_or(0);
    rss_kb * 1024
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
    println!();

    // Baseline RSS
    let rss_before = rss_bytes();
    println!("RSS before opening archive: {}", fmt_bytes(rss_before));

    // Phase 1: Open archive (parses all central directory entries)
    let file = fs::File::open(path)?;
    let reader = BufReader::new(file);
    let mut archive = zip::ZipArchive::new(reader)?;

    let rss_after_open = rss_bytes();
    let num_entries = archive.len();
    println!("RSS after ZipArchive::new(): {}", fmt_bytes(rss_after_open));
    println!(
        "  Delta: {} for {num_entries} entries",
        fmt_bytes(rss_after_open.saturating_sub(rss_before))
    );
    if num_entries > 0 {
        println!(
            "  Per-entry overhead: {} bytes",
            rss_after_open.saturating_sub(rss_before) / num_entries
        );
    }
    println!();

    // Compute filename/metadata stats from public API
    let mut total_filename_bytes = 0usize;
    for name in archive.file_names() {
        total_filename_bytes += name.len();
    }
    println!("=== Allocation breakdown (estimated) ===");
    println!("Total filename bytes: {}", fmt_bytes(total_filename_bytes));
    println!(
        "  IndexMap key (Arc<str>) shares refcount with ZipFileName: no extra copy",
    );
    println!();

    // Phase 2: Iterate with by_index() (creates decompressor per entry)
    let rss_before_iter = rss_bytes();
    for i in 0..num_entries {
        let _file = archive.by_index(i)?;
        // drop immediately without reading
    }
    let rss_after_iter = rss_bytes();
    println!(
        "RSS after iterating by_index() (no read): {}",
        fmt_bytes(rss_after_iter)
    );
    println!(
        "  Delta from iteration: {}",
        fmt_bytes(rss_after_iter.saturating_sub(rss_before_iter))
    );
    if num_entries > 0 {
        println!(
            "  Per-entry overhead: {} bytes",
            rss_after_iter.saturating_sub(rss_before_iter) / num_entries
        );
    }
    println!();

    // Phase 3: Iterate with by_index_raw() (minimal allocation)
    let rss_before_raw = rss_bytes();
    for i in 0..num_entries {
        let _file = archive.by_index_raw(i)?;
    }
    let rss_after_raw = rss_bytes();
    println!(
        "RSS after iterating by_index_raw() (no read): {}",
        fmt_bytes(rss_after_raw)
    );
    println!(
        "  Delta from raw iteration: {}",
        fmt_bytes(rss_after_raw.saturating_sub(rss_before_raw))
    );
    println!();

    // Phase 4: Just iterate names (no file open at all)
    let rss_before_names = rss_bytes();
    let mut name_count = 0usize;
    for name in archive.file_names() {
        let _ = name;
        name_count += 1;
    }
    let rss_after_names = rss_bytes();
    println!(
        "RSS after iterating file_names(): {}",
        fmt_bytes(rss_after_names)
    );
    println!(
        "  Delta from name iteration: {}",
        fmt_bytes(rss_after_names.saturating_sub(rss_before_names))
    );
    println!("  Names iterated: {name_count}");

    Ok(())
}
