//! Prints the image and overlay the app assigns to Warframe Systems / Chassis / Neuroptics
//! parts and blueprints, in the Inventory and on Foundry cards.
//!
//! Usage: cargo run --example systems_check -- <wfcd folder> <inventory.json>

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use wf_companion_lib::translate::{parse, wfcd, ItemIndex};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(wfcd_dir), Some(inv_path)) = (args.next(), args.next()) else {
        eprintln!("usage: systems_check <wfcd folder> <inventory.json>");
        std::process::exit(2);
    };
    let index = ItemIndex::build(&wfcd::load_cached(&wfcd_dir));
    let inv = parse(&std::fs::read(&inv_path).expect("read inventory")).expect("parse inventory");

    println!("== Inventory parts ==");
    let data = wf_companion_lib::inventory_view::build(&index, &inv.values);
    let mut combos: BTreeMap<(String, String, String), Vec<String>> = BTreeMap::new();
    for p in data.parts.iter().filter(|p| p.name.contains("Systems")) {
        let key = (
            p.kind.clone(),
            p.image.clone().unwrap_or_else(|| "-".into()),
            p.overlay.clone().unwrap_or_else(|| "-".into()),
        );
        combos.entry(key).or_default().push(p.name.clone());
    }
    for ((kind, image, overlay), names) in &combos {
        println!("{kind:9} image={image:32} overlay={overlay:36} ×{:3} e.g. {}", names.len(), names[0]);
    }

    println!("\n== Foundry Systems parts ==");
    let items = wf_companion_lib::foundry::build(&index, &inv.values, &HashSet::new());
    let mut seen: BTreeMap<(String, String), (usize, String)> = BTreeMap::new();
    for i in items.iter().filter(|i| format!("{:?}", i.tab) == "Warframe") {
        for p in i.parts.iter().filter(|p| p.name.contains("Systems")) {
            let key = (p.image.clone().unwrap_or_else(|| "-".into()), p.overlay.clone().unwrap_or_else(|| "-".into()));
            let e = seen.entry(key).or_insert((0, p.name.clone()));
            e.0 += 1;
        }
    }
    for ((image, overlay), (n, example)) in &seen {
        println!("image={image:32} overlay={overlay:36} ×{n:3} e.g. {example}");
    }
}
