//! Builds the Foundry list from cached WFCD data and an inventory file, and prints a summary.
//!
//! Usage: cargo run --example foundry_check -- <wfcd folder> <inventory.json>

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use wf_companion_lib::foundry::build;
use wf_companion_lib::translate::{parse, wfcd, ItemIndex, ACCOUNT};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(wfcd_dir), Some(inv_path)) = (args.next(), args.next()) else {
        eprintln!("usage: foundry_check <wfcd folder> <inventory.json>");
        std::process::exit(2);
    };
    let index = ItemIndex::build(&wfcd::load_cached(&wfcd_dir));
    let inv = parse(&std::fs::read(&inv_path).expect("read inventory")).expect("parse inventory");
    let items = build(&index, &inv.values, &HashSet::new());

    println!("mastery rank: {:?}", inv.values.get(&(ACCOUNT.to_string(), "PlayerLevel".to_string())));
    println!("{:<10} {:>6} {:>8} {:>6} {:>9} {:>6} {:>8}", "tab", "items", "mastered", "owned", "subsumed", "ready", "incarnon");
    let mut by_tab: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for i in &items {
        by_tab.entry(format!("{:?}", i.tab)).or_default().push(i);
    }
    for (tab, list) in &by_tab {
        let count = |f: &dyn Fn(&&&wf_companion_lib::foundry::FoundryItem) -> bool| list.iter().filter(f).count();
        println!(
            "{:<10} {:>6} {:>8} {:>6} {:>9} {:>6} {:>8}",
            tab,
            list.len(),
            count(&|i| i.mastered),
            count(&|i| i.owned > 0),
            count(&|i| i.subsumed),
            count(&|i| i.ready_to_build),
            count(&|i| i.incarnon),
        );
    }
    println!("total: {} items, {} mastered", items.len(), items.iter().filter(|i| i.mastered).count());

    let missing_images = items.iter().filter(|i| i.image.is_none()).count();
    println!("items without an image: {missing_images}");
    println!("\nsamples:");
    for name in ["Frost Prime", "Braton Prime", "Titania Prime", "Mote Prism", "Para Moa"] {
        if let Some(i) = items.iter().find(|i| i.name == name) {
            let parts: Vec<String> = i.parts.iter().map(|p| format!("{} {}/{}", p.name, p.owned, p.needed)).collect();
            println!(
                "  {} [{:?}] rank {}/{} mastered={} owned={} subsumed={} shards={} parts: {}",
                i.name, i.tab, i.rank, i.max_rank, i.mastered, i.owned, i.subsumed, i.archon_shards, parts.join(", ")
            );
        }
    }
}
