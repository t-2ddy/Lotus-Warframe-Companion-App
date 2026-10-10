//! Runs the Mastery Helper calculation on real data and prints a summary.
//!
//! Usage: cargo run --example mastery_check -- <wfcd folder> <inventory.json>

use std::collections::HashSet;
use std::path::PathBuf;

use wf_companion_lib::mastery::build;
use wf_companion_lib::translate::nodes::NodeData;
use wf_companion_lib::translate::{parse, wfcd, ItemIndex};

fn main() {
    let mut args = std::env::args().skip(1).map(PathBuf::from);
    let (Some(wfcd_dir), Some(inv_path)) = (args.next(), args.next()) else {
        eprintln!("usage: mastery_check <wfcd folder> <inventory.json>");
        std::process::exit(2);
    };
    let index = ItemIndex::build(&wfcd::load_cached(&wfcd_dir));
    let nodes = NodeData::bundled();
    let inv = parse(&std::fs::read(&inv_path).expect("read inventory")).expect("parse inventory");
    let d = build(&index, &nodes, &inv.values, &HashSet::new());

    let b = &d.breakdown;
    println!("rank {}  total {:>12}", d.rank, fmt(d.total_xp));
    println!("  items {}  nodes {}  steel path {}  junctions {}  intrinsics {}",
        fmt(b.items), fmt(b.nodes), fmt(b.steel_path_nodes), fmt(b.junctions), fmt(b.intrinsics));
    println!("  progress {} / {} ({:.0}%)  ready for test: {}",
        fmt(d.total_xp - d.rank_start), fmt(d.next_rank - d.rank_start),
        100.0 * (d.total_xp - d.rank_start) as f64 / (d.next_rank - d.rank_start) as f64, d.ready_for_test);

    for (title, list) in [("content", &d.content), ("star chart", &d.star_chart), ("intrinsics", &d.intrinsics)] {
        let parts: Vec<String> = list.iter().map(|p| format!("{} {}/{}", p.label, p.done, p.total)).collect();
        println!("{title}: {}", parts.join(" · "));
    }

    println!("\neasy: {}", d.easy.len());
    for s in d.easy.iter().take(5) {
        println!("   +{:>5}  {} (rank {}/{})", s.xp_gain, s.name, s.rank, s.max_rank);
    }
    println!("from relics: {}", d.relics.len());
    for s in d.relics.iter().take(8) {
        let state = if s.ready { "ready to build".to_string() } else { format!("relics: {}", s.relic_sources.join(", ")) };
        println!("   {}/{}  {:22} {}", s.relic_parts_owned, s.relic_parts_total, s.name, state);
    }
    println!("with platinum candidates: {} (parts to price: {})",
        d.platinum.len(), d.platinum.iter().flat_map(|s| s.buy.iter().map(|b| b.path.as_str())).collect::<HashSet<_>>().len());
}

fn fmt(n: i64) -> String {
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}
