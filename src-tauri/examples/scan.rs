//! Scan a memory dump, any file, or the running game for the inventory JSON.
//!
//! Usage: cargo run --release --example scan -- <path to .dmp | --live | --live-all> [output.json]
//!
//! `--live` scans the running game's heap memory; `--live-all` scans every readable region.

use std::path::PathBuf;
use std::time::Instant;

use wf_companion_lib::capture::{
    find_inventory, find_pid, FileSource, MemorySource, ProcessSource, Scope, GAME_EXE,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(input) = args.next() else {
        eprintln!("usage: scan <dump file | --live | --live-all> [output.json]");
        std::process::exit(2);
    };
    let output = args.next().map(PathBuf::from);

    let live_scope = match input.as_str() {
        "--live" => Some(Scope::Heap),
        "--live-data" => Some(Scope::Data),
        "--live-all" => Some(Scope::AllReadable),
        _ => None,
    };
    let mut process = None;
    let source: Box<dyn MemorySource> = if let Some(scope) = live_scope {
        let pid = find_pid(GAME_EXE).unwrap_or_else(|| {
            eprintln!("{GAME_EXE} is not running");
            std::process::exit(1);
        });
        let open = || {
            ProcessSource::open(pid, scope).unwrap_or_else(|e| {
                eprintln!("can't open process {pid}: {e}");
                std::process::exit(1);
            })
        };
        process = Some(open());
        Box::new(open())
    } else {
        Box::new(FileSource::open(&PathBuf::from(&input)).unwrap_or_else(|e| {
            eprintln!("can't open {input}: {e}");
            std::process::exit(1);
        }))
    };

    let started = Instant::now();
    let (snapshot, stats) = find_inventory(source.as_ref());
    println!(
        "scanned {} regions, {} MB in {:.1} s, {} candidates",
        stats.regions,
        stats.bytes_scanned / (1024 * 1024),
        started.elapsed().as_secs_f64(),
        stats.candidates
    );
    match snapshot {
        Some(s) => {
            let kind = process.as_ref().map_or("file".to_string(), |p| p.describe(s.address));
            println!("inventory found at {:#x} ({kind}), {} bytes", s.address, s.bytes.len());
            if let Some(out) = output {
                std::fs::write(&out, &s.bytes).expect("write output");
                println!("saved to {}", out.display());
            }
        }
        None => {
            for r in &stats.rejections {
                println!("rejected {:#x}: {}", r.address, r.reason);
            }
            println!("no inventory found");
            std::process::exit(1);
        }
    }
}
