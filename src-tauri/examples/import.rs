//! Imports saved inventory JSON files into the app's database, oldest first, as if they had
//! been captured at each file's modified time. Useful for testing and for seeding history.
//!
//! Usage: cargo run --example import -- <wf-companion.db> <inventory.json>...

use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use wf_companion_lib::translate::parse;
use wf_companion_lib::update::{Outcome, Store};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let Some(db) = args.next() else {
        eprintln!("usage: import <wf-companion.db> <inventory.json>...");
        std::process::exit(2);
    };
    let mut files: Vec<(i64, PathBuf)> = args
        .map(|p| {
            let modified = std::fs::metadata(&p)
                .and_then(|m| m.modified())
                .map(|t| t.duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as i64)
                .unwrap_or_else(|e| panic!("can't read {}: {e}", p.display()));
            (modified, p)
        })
        .collect();
    files.sort();

    let mut store = Store::open(&db).expect("open database");
    for (at, path) in files {
        let raw = std::fs::read(&path).expect("read file");
        let inv = parse(&raw).expect("parse inventory");
        match store.apply(&inv, &raw, at).expect("store capture") {
            Outcome::Baseline { .. } => println!("{}: stored as starting point", path.display()),
            Outcome::Changed { changes, .. } => println!("{}: {} changes", path.display(), changes.len()),
            Outcome::Unchanged => println!("{}: no changes", path.display()),
        }
    }
}
