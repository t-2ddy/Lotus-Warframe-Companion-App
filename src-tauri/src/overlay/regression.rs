//! The riven card regression set: real cards with their known stats (`fixtures/riven-cards/`,
//! built by `scripts/riven_cards.py`, local only). Every case is read and worked out as the
//! overlay does and compared with the truth; the score is printed.
//!
//! - `text_cases` (runs with `cargo test`): OCR text through `parse_card`, `consensus` and
//!   `work_out`. Lotus's own reads and AlecaFrame's.
//! - `image_cases` (`--ignored`, ~a minute): game screenshots through the overlay's crop, cleanup
//!   copies and Windows OCR first, as captured and dimmed to 55% (a card fading in).
//!
//! Cases listed in `known-failures.txt` may fail; any other failure fails the test, and a listed
//! case that now passes is reported so it can be taken off. `RIVEN_CARDS_BLESS=1` rewrites the
//! list from this run.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;

use super::card::{self, consensus, parse_card, parse_stat, Card};
use super::{read_variants, screen, INSPECT_CARD, REROLL_CENTRE, REROLL_LEFT, REROLL_RIGHT};
use crate::riven_grades::GradeSheet;
use crate::rivens::{self, RivenData};

#[derive(Deserialize)]
struct Case {
    id: String,
    #[serde(default)]
    reads: Vec<Vec<String>>,
    #[serde(default)]
    image: Option<String>,
    #[serde(rename = "type")]
    kind: String,
    weapon: Option<String>,
    expect: Expect,
}

#[derive(Deserialize)]
struct Expect {
    title: String,
    /// (tag, number as shown) in card order. A `?label` tag is a stat Lotus's data doesn't have
    /// (a splice); a `null` number is one nobody could read (any number, or "not read", passes).
    stats: Vec<(String, Option<f64>)>,
    rank: Option<u32>,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/riven-cards")
}

fn cases() -> Option<Vec<Case>> {
    let text = std::fs::read_to_string(dir().join("cards.json")).ok()?;
    Some(serde_json::from_str(&text).expect("cards.json is valid"))
}

/// The same stat: Lotus's tag may be a removed stat (`Legacy:`), a spliced element under its
/// overlay name (`SplicedViral`) or a trait it has no data for (`Pending:<Name>`).
fn same_tag(data: &RivenData, kind: &str, ours: &str, want: &str) -> bool {
    let ours = ours.strip_prefix(rivens::LEGACY_PREFIX).unwrap_or(ours);
    if ours == want {
        return true;
    }
    if let (Some(a), Some(b)) = (rivens::spliced_element(ours), rivens::spliced_element(want)) {
        return a == b;
    }
    let unknown_to_us = want.starts_with('?') || data.types.get(kind).is_some_and(|k| !k.stats.contains_key(want));
    ours.starts_with(rivens::PENDING_PREFIX) && unknown_to_us
}

/// The riven's own name matches (the weapon part is checked by the numbers fitting its
/// disposition, so a misread weapon name that's still recognised passes).
fn same_riven(card: &Card, title: &str) -> bool {
    card::norm(card.riven_name()) == card::norm(title.rsplit(' ').next().unwrap_or_default())
}

/// Works a card out as the overlay does for someone else's riven, and compares it with the truth.
fn check(data: &RivenData, case: &Case, card: Option<Card>) -> Result<(), String> {
    let card = card.ok_or("not read as a card")?;
    if !same_riven(&card, &case.expect.title) {
        return Err(format!("title {:?}", card.title));
    }
    let types = [case.kind.clone()];
    let (r, _) = card::work_out(data, &GradeSheet::default(), &card, &types, case.weapon.as_deref(), None, |_| false)
        .map_err(|e| format!("{} ({:?})", e.join("; "), card.stat_lines))?;
    let got: Vec<(String, Option<f64>)> = r
        .view
        .stats
        .iter()
        .map(|s| {
            let n = if s.unread { None } else { parse_stat(&s.text).map(|p| p.value) };
            (s.tag.clone(), n)
        })
        .collect();
    let shown = || format!("{:?}", r.view.stats.iter().map(|s| s.text.as_str()).collect::<Vec<_>>());
    if got.len() != case.expect.stats.len() {
        return Err(format!("{} stats {}", got.len(), shown()));
    }
    for ((tag, n), (want_tag, want_n)) in got.iter().zip(&case.expect.stats) {
        if !same_tag(data, &case.kind, tag, want_tag) {
            return Err(format!("{tag} for {want_tag} {}", shown()));
        }
        match (n, want_n) {
            (_, None) => {}
            (Some(a), Some(b)) if (a - b).abs() < 0.15 => {}
            _ => return Err(format!("{want_tag}: {n:?} for {want_n:?} {}", shown())),
        }
    }
    if let Some(rank) = case.expect.rank {
        if r.view.rank != rank {
            return Err(format!("rank {} for {rank}", r.view.rank));
        }
    }
    Ok(())
}

/// Scores the results against `known-failures.txt` (see the module docs).
fn score(name: &str, results: BTreeMap<String, Result<(), String>>) {
    let known_file = dir().join("known-failures.txt");
    let known: Vec<String> = std::fs::read_to_string(&known_file)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| l.split('#').next().map(str::trim).filter(|l| !l.is_empty()).map(String::from))
        .collect();
    let passed = results.values().filter(|r| r.is_ok()).count();
    println!("{name}: {passed}/{} cards worked out right", results.len());
    let mut new = Vec::new();
    for (id, r) in &results {
        match r {
            Err(why) => {
                println!("  FAIL {id}: {why}");
                if !known.contains(id) {
                    new.push(id.clone());
                }
            }
            Ok(()) if known.contains(id) => println!("  now passes (take it off known-failures.txt): {id}"),
            Ok(()) => {}
        }
    }
    if std::env::var("RIVEN_CARDS_BLESS").is_ok() {
        // Keep the other test's entries; replace this test's.
        let mut lines: Vec<String> = known.iter().filter(|k| !results.contains_key(*k)).cloned().collect();
        lines.extend(results.iter().filter_map(|(id, r)| r.as_ref().err().map(|why| format!("{id}  # {}", why.chars().take(120).collect::<String>()))));
        lines.sort();
        std::fs::write(&known_file, lines.join("\n") + "\n").unwrap();
        return;
    }
    assert!(new.is_empty(), "{} card(s) that used to work out now fail: {new:?}", new.len());
}

#[test]
fn text_cases() {
    let Some(cases) = cases() else { return };
    let data = rivens::bundled();
    let results = cases
        .iter()
        .filter(|c| !c.reads.is_empty())
        .map(|c| {
            let cards: Vec<Card> = c.reads.iter().filter_map(|lines| parse_card(lines)).collect();
            (c.id.clone(), check(&data, c, consensus(&cards)))
        })
        .collect();
    score("text", results);
}

#[test]
#[ignore = "slow: Windows OCR on every screenshot"]
fn image_cases() {
    let Some(cases) = cases() else { return };
    let data = rivens::bundled();
    let mut results = BTreeMap::new();
    for c in &cases {
        let Some(img) = &c.image else { continue };
        let shot = super::ocr::decode_file(&dir().join(img)).expect("screenshot decodes");
        // As captured, and at 55% brightness like a card still fading in (the overlay's first
        // reads of a chat link).
        for (suffix, brightness) in [("", 1.0), ("@55", 0.55)] {
            let shot = dimmed(&shot, brightness);
            let game = screen::GameWindow { hwnd: 0, x: 0, y: 0, width: shot.width as i32, height: shot.height as i32 };
            // The overlay knows which screen it's on; here, the region whose card has the title.
            let card = [INSPECT_CARD, REROLL_LEFT, REROLL_RIGHT, REROLL_CENTRE].into_iter().find_map(|region| {
                let (x, y, w, h) = region.on(&game);
                let reads = read_variants(&screen::ocr_variants(&screen::crop(&shot, x, y, w, h)));
                let cards: Vec<Card> = reads.into_iter().filter_map(Result::ok).filter_map(|l| parse_card(&l)).collect();
                consensus(&cards).filter(|card| same_riven(card, &c.expect.title))
            });
            results.insert(format!("{}{suffix}", c.id), check(&data, c, card));
        }
    }
    score("images", results);
}

fn dimmed(img: &screen::Image, brightness: f32) -> screen::Image {
    let mut bgra = img.bgra.clone();
    for p in bgra.chunks_exact_mut(4) {
        for c in &mut p[..3] {
            *c = (f32::from(*c) * brightness) as u8;
        }
    }
    screen::Image { width: img.width, height: img.height, bgra }
}

#[test]
fn tags_compare_through_overlay_names() {
    let data = rivens::bundled();
    assert!(same_tag(&data, "LotusRifleRandomModRare", "SplicedViral", "WeaponViralDamageMod"));
    assert!(same_tag(&data, "LotusShotgunRandomModRare", "Legacy:WeaponZoomFovMod", "WeaponZoomFovMod"));
    assert!(same_tag(&data, "LotusRifleRandomModRare", "Pending:Status Damage", "WeaponStatusDamage"));
    assert!(!same_tag(&data, "LotusRifleRandomModRare", "Pending:Status Damage", "WeaponCritChanceMod"));
}

