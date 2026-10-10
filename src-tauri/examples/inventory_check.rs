//! Builds the Inventory tabs from cached WFCD data and an inventory file, and prints a summary.
//!
//! Usage: cargo run --example inventory_check -- <wfcd folder> <inventory.json>

use std::collections::BTreeMap;
use std::path::PathBuf;

use wf_companion_lib::inventory_view::build;
use wf_companion_lib::translate::{parse, wfcd, ItemIndex};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(wfcd_dir), Some(inv_path)) = (args.next(), args.next()) else {
        eprintln!("usage: inventory_check <wfcd folder> <inventory.json>");
        std::process::exit(2);
    };
    let index = ItemIndex::build(&wfcd::load_cached(&wfcd_dir));
    let inv = parse(&std::fs::read(&inv_path).expect("read inventory")).expect("parse inventory");
    let data = build(&index, &inv.values);

    println!("parts {} · relics {} · mods {} · arcanes {} · misc {}", data.parts.len(), data.relics.len(), data.mods.len(), data.arcanes.len(), data.misc.len());

    let unnamed = |path: &str| index.get(path).is_none();
    let fallback_parts: Vec<&str> = data.parts.iter().filter(|p| unnamed(&p.path)).map(|p| p.name.as_str()).collect();
    println!("\nparts not in WFCD: {} (codename-named: {})", fallback_parts.len(),
        data.parts.iter().filter(|p| unnamed(&p.path) && p.path.contains("/WarframeRecipes/")).count());
    for n in fallback_parts.iter().take(12) {
        println!("   {n}");
    }

    let blueprints: Vec<_> = data.parts.iter().filter(|p| p.kind == "Blueprint").collect();
    let with_overlay = blueprints.iter().filter(|p| p.overlay.is_some()).count();
    println!("blueprints with an overlay image: {with_overlay} / {}", blueprints.len());
    for p in blueprints.iter().filter(|p| p.overlay.is_none()).take(15) {
        println!("   no overlay: {}", p.name);
    }
    let bare: Vec<&str> = blueprints.iter().filter(|p| p.name == "Blueprint").map(|p| p.path.as_str()).collect();
    println!("named just \"Blueprint\": {}", bare.len());
    for p in bare.iter().take(10) {
        println!("   {p}");
    }

    let mut builds: BTreeMap<&str, usize> = BTreeMap::new();
    for p in &data.parts {
        *builds.entry(p.builds).or_default() += 1;
    }
    println!("parts by builds: {builds:?}");
    println!("blueprints: {}, parts: {}", data.parts.iter().filter(|p| p.kind == "Blueprint").count(), data.parts.iter().filter(|p| p.kind == "Part").count());

    if let Some(r) = data.relics.iter().find(|r| r.name == "Lith S8") {
        println!("\n{} [{}] vaulted={} counts={:?} total={}", r.name, r.era, r.vaulted, r.counts, r.total);
        for w in &r.rewards {
            println!("   {:9} {:30} owned {}", w.rarity, w.name, w.owned);
        }
    }
    let no_rewards = data.relics.iter().filter(|r| r.rewards.is_empty()).count();
    println!("relics without rewards data: {no_rewards}");

    for m in data.mods.iter().filter(|m| m.ranks.len() > 1).take(3) {
        println!("\nmod {} ×{} ranks {:?} max {:?} {:?}", m.name, m.count, m.ranks.iter().map(|r| (r.rank, r.count)).collect::<Vec<_>>(), m.max_rank, m.rarity);
    }
    let mods_unknown = data.mods.iter().filter(|m| unnamed(&m.path)).count();
    println!("mods not in WFCD: {mods_unknown}");

    let mut misc_kinds: BTreeMap<&str, usize> = BTreeMap::new();
    for m in &data.misc {
        *misc_kinds.entry(m.kind.as_str()).or_default() += 1;
    }
    println!("\nmisc kinds: {misc_kinds:?}");
}
