//! Turning OCR'd riven card text into a riven: parse the lines, match the stats to riven stat tags,
//! then either find the riven in the user's inventory (exact rolls) or work the rolls out
//! backwards from the shown numbers (other players' rivens and fresh rerolls).

use serde_json::json;

use crate::riven_grades::GradeSheet;
use crate::rivens::{self, RivenData, RivenType, RivenView};
use crate::translate::OwnedRiven;

/// How a number is written on the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    /// `+191.1% Critical Chance`
    Percent,
    /// `x1.45 Damage to Corpus`
    Times,
    /// `+3.4 Punch Through`, `+8.4s Combo Duration`
    Plain,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CardStat {
    /// The number as shown, signed (`-132.2`); for `Times`, the multiplier (`1.45`).
    pub value: f64,
    pub unit: Unit,
    /// The label, letters only, lowercase (`criticalchance`); may be empty or start with junk
    /// from an icon (`ypuncture`).
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    /// `Burston Crita-lexitis` (wrapped lines joined).
    pub title: String,
    pub stats: Vec<CardStat>,
    pub mastery: Option<u32>,
    pub rerolls: Option<u32>,
    /// The stat lines as read, wrapped labels joined (`-46.2% Magazine Capacity`), for showing
    /// the card before it's worked out.
    pub stat_lines: Vec<String>,
    /// Stat labels read without their number (a card still fading in): it can't be worked out.
    pub lost_numbers: usize,
}

impl Card {
    pub fn empty() -> Self {
        Card { title: String::new(), stats: Vec::new(), mastery: None, rerolls: None, stat_lines: Vec::new(), lost_numbers: 0 }
    }

    /// The riven's own name: the last word of the title.
    pub fn riven_name(&self) -> &str {
        self.title.rsplit(' ').next().unwrap_or_default()
    }
    /// The weapon part of the title.
    pub fn weapon_name(&self) -> &str {
        self.title.rsplit_once(' ').map_or("", |(w, _)| w)
    }
    /// Same riven text (for telling the old and new roll apart).
    pub fn same_as(&self, other: &Card) -> bool {
        norm(&self.title) == norm(&other.title)
            && self.stats.len() == other.stats.len()
            && self.stats.iter().zip(&other.stats).all(|(a, b)| (a.value - b.value).abs() < 0.05)
    }
}

/// Letters only, lowercase.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect()
}

/// Finds the number in a stat line: a sign (`+`, `-`, `x`) followed by digits, tolerating stray
/// spaces (`+191 .1%`). Returns (value, unit, label text).
fn stat_line(line: &str) -> Option<(f64, Unit, String)> {
    let chars: Vec<char> = line.chars().map(|c| if c == '×' { 'x' } else { c }).collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // A sign stands on its own: the hyphen in a weapon's name ("AX-52 Toxi-vexidex") isn't one.
        let after_word = i > 0 && chars[i - 1].is_alphanumeric();
        let sign_ok = (matches!(c, '+' | '-' | '−' | '–') && !after_word)
            || (matches!(c, 'x' | 'X') && (i == 0 || !chars[i - 1].is_alphabetic()));
        if sign_ok {
            let mut j = i + 1;
            while j < chars.len() && chars[j] == ' ' {
                j += 1;
            }
            if j < chars.len() && chars[j].is_ascii_digit() {
                let mut num = String::new();
                while j < chars.len() {
                    let d = chars[j];
                    if d.is_ascii_digit() || d == '.' || d == ',' {
                        num.push(if d == ',' { '.' } else { d });
                        j += 1;
                    } else if d == ' ' && chars.get(j + 1).is_some_and(|n| n.is_ascii_digit() || *n == '.') {
                        j += 1;
                    } else {
                        break;
                    }
                }
                // "%" read as "0/0" ("+57.70/0" for "+57.7%"): the first 0 joined the number.
                let percent_as_slash = num.ends_with('0')
                    && num.contains('.')
                    && chars.get(j) == Some(&'/')
                    && chars.get(j + 1).is_some_and(|n| matches!(n, '0' | 'o' | 'O'));
                if percent_as_slash {
                    num.pop();
                    j += 2;
                }
                let mut value: f64 = num.trim_end_matches('.').parse().ok()?;
                let mut unit = if matches!(c, 'x' | 'X') { Unit::Times } else { Unit::Plain };
                if percent_as_slash {
                    unit = Unit::Percent;
                }
                if matches!(c, '-' | '−' | '–') {
                    value = -value;
                }
                while j < chars.len() && chars[j] == ' ' {
                    j += 1;
                }
                if unit == Unit::Plain && !percent_as_slash && chars.get(j) == Some(&'%') {
                    unit = Unit::Percent;
                    j += 1;
                } else if unit == Unit::Plain
                    && chars.get(j) == Some(&'s')
                    && chars.get(j + 1).is_none_or(|n| *n == ' ')
                {
                    j += 1; // seconds
                }
                let label: String = chars[j..].iter().collect();
                return Some((value, unit, norm(&label)));
            }
        }
        i += 1;
    }
    None
}

/// One stat line on its own (`+191.1% Critical Chance`).
pub fn parse_stat(line: &str) -> Option<CardStat> {
    stat_line(line).map(|(value, unit, label)| CardStat { value, unit, label })
}

/// Parses OCR lines of one card. `None` unless it has a title and at least two stats.
pub fn parse_card(lines: &[String]) -> Option<Card> {
    let mut title_parts: Vec<String> = Vec::new();
    let mut stats: Vec<CardStat> = Vec::new();
    let mut stat_lines: Vec<String> = Vec::new();
    let (mut mastery, mut rerolls) = (None, None);
    let mut lost_numbers = 0;
    for raw in lines {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(n) = compact.strip_prefix("MR").and_then(ocr_number) {
            mastery = Some(n);
            continue;
        }
        // Junk after the count ("0113 \\") is the card's edge.
        let compact = compact.trim_end_matches(|c: char| !c.is_alphanumeric());
        if mastery.is_some() && compact.chars().all(|c| c.is_ascii_digit()) && !compact.is_empty() {
            // Reroll count; its icon is read as a leading 0 ("094").
            let digits = if compact.len() > 1 && compact.starts_with('0') { &compact[1..] } else { &compact[..] };
            rerolls = digits.parse().ok();
            continue;
        }
        if !stats.is_empty() && mastery.is_none() && line.starts_with('(') {
            // The rest of a label in brackets ("Critical Chance" / "(x2 for Heavy Attacks)"); its
            // "x2" isn't a stat.
            continue_label(&mut stats, &mut stat_lines, line);
            continue;
        }
        if let Some((value, unit, label)) = stat_line(line) {
            // Stats come after the title; number-like noise above it is card art. A stat needs
            // a readable label, except a bare percentage whose label hid behind an icon.
            let plausible = label.len() >= 3 || (unit == Unit::Percent && label.is_empty());
            if !title_parts.is_empty() && plausible {
                stats.push(CardStat { value, unit, label });
                stat_lines.push(line.to_string());
                continue;
            }
            if stats.is_empty() && mastery.is_none() {
                title_parts.clear(); // art: start the title again below it
            }
            continue;
        }
        if stats.is_empty() && mastery.is_none() {
            title_parts.push(line.to_string());
        } else if mastery.is_none() && is_faction_label(line) {
            // A whole label on its own ("Damage to Infested", its "x0.98" not read yet), not
            // the end of the label above.
            lost_numbers += 1;
        } else if mastery.is_none() && !compact.starts_with("MR") && looks_like_words(line.trim_end_matches(')')) {
            // A long label wraps onto its own line ("-46.2% Magazine" / "Capacity", "Fire Rate
            // (x2 for" / "Bows)").
            continue_label(&mut stats, &mut stat_lines, line);
        }
    }
    let title = title_from(&title_parts);
    (!title.is_empty() && stats.len() >= 2).then_some(Card { title, stats, mastery, rerolls, stat_lines, lost_numbers })
}

/// "Damage to Infested" / "Damage vs Corpus": a faction stat's label, which never wraps.
fn is_faction_label(line: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    words.len() == 3 && words[0].eq_ignore_ascii_case("damage") && matches!(words[1].to_lowercase().as_str(), "to" | "vs" | "vs.")
}

/// The label part of a stat line as shown: `+139.9% Slam Attack Damage` → `Slam Attack Damage`.
fn label_text(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let start = (0..chars.len())
        .find(|&i| {
            let c = chars[i];
            let times = matches!(c, 'x' | 'X') && chars.get(i + 1).is_some_and(char::is_ascii_digit);
            c.is_alphabetic() && !times
        })
        .unwrap_or(chars.len());
    chars[start..].iter().collect::<String>().trim().to_string()
}

/// A number OCR may have read with look-alike letters ("IO" for "10", "l5" for "15").
fn ocr_number(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 3 {
        return None;
    }
    let digits: String = s
        .chars()
        .map(|c| match c {
            'O' | 'o' | 'D' => '0',
            'I' | 'l' | '|' | 'i' => '1',
            'S' | 's' => '5',
            'B' => '8',
            c => c,
        })
        .collect();
    digits.parse().ok()
}

/// Joins a wrapped line onto the last stat's label.
fn continue_label(stats: &mut [CardStat], lines: &mut [String], line: &str) {
    if let Some(last) = stats.last_mut() {
        last.label.push_str(&norm(line));
    }
    if let Some(last) = lines.last_mut() {
        last.push(' ');
        last.push_str(line);
    }
}

/// The title from the lines above the stats. Anything higher up is card art, so start from the
/// last line and only reach up for a wrapped name: a line ending in `-` ("Burston Acri-" +
/// "hexasus"), or the weapon on its own line when the last line is just the riven's name
/// ("Plague Kripath" + "Acricron").
fn title_from(lines: &[String]) -> String {
    let Some(last) = lines.last() else { return String::new() };
    let mut title = last.clone();
    for prev in lines.iter().rev().skip(1) {
        if prev.ends_with('-') {
            title = format!("{prev}{title}");
        } else if !title.contains(' ') && looks_like_words(prev) {
            title = format!("{prev} {title}");
        } else {
            break;
        }
    }
    title
}

/// Letters and spaces (a weapon name), not OCR noise from the card art.
fn looks_like_words(s: &str) -> bool {
    s.len() >= 3 && s.chars().all(|c| c.is_alphabetic() || matches!(c, ' ' | '&' | '-' | '\'')) && s.chars().filter(|c| c.is_alphabetic()).count() >= 3
}

/// A data label (`|val|% Critical Chance`) as letters for matching (`criticalchance`).
fn label_key(label: &str) -> String {
    let rest = label.replace("|val|", "");
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('%').or_else(|| rest.strip_prefix("s ")).unwrap_or(rest);
    norm(rest)
}

/// Every label a card can show for this riven type, as (tag, letters): the type's stats, spliced
/// elements (`viral`, `viraldamage`) and the not-yet-graded spliced traits (`statusdamage`,
/// `damagetoorokin` / `damagevsorokin`).
fn label_keys(kind: &RivenType) -> Vec<(String, String)> {
    let mut keys: Vec<(String, String)> =
        kind.stats.iter().map(|(tag, s)| (tag.clone(), label_key(&s.label))).filter(|(_, k)| !k.is_empty()).collect();
    if kind.stats.contains_key("WeaponFireDamageMod") {
        for e in rivens::SPLICED_ELEMENTS {
            keys.push((format!("Spliced{e}"), norm(e)));
            keys.push((format!("Spliced{e}"), norm(&format!("{e} Damage"))));
        }
    }
    for name in rivens::SPLICED_PENDING {
        let tag = format!("{}{name}", rivens::PENDING_PREFIX);
        keys.push((tag.clone(), norm(name)));
        if name.contains(" to ") {
            keys.push((tag, norm(&name.replace(" to ", " vs "))));
        }
    }
    keys
}

/// The stat tag for an OCR'd label. Tries, in order: the exact label; a label with junk before
/// it (an icon read as a letter, `ypuncture`), longest first so `criticaldamage` beats `damage`
/// and `statusdamage` beats both; a cut-off label (`criticalcha`), only if exactly one label
/// starts that way.
pub fn match_label(kind: &RivenType, ocr: &str) -> Option<String> {
    if ocr.len() < 3 {
        return None;
    }
    let keys = label_keys(kind);
    if let Some((tag, _)) = keys.iter().find(|(_, k)| k == ocr) {
        return Some(tag.clone());
    }
    // Icon junk is a letter or two ("ypuncture"). A longer prefix is a different stat
    // ("slamattackdamage" isn't Damage).
    let junk_ok = |k: &str| ocr.ends_with(k) && ocr.len() - k.len() <= 3;
    if let Some((tag, _)) = keys.iter().filter(|(_, k)| junk_ok(k)).max_by_key(|(_, k)| k.len()) {
        return Some(tag.clone());
    }
    let mut cut: Vec<&String> = keys.iter().filter(|(_, k)| ocr.len() >= 6 && k.starts_with(ocr)).map(|(t, _)| t).collect();
    cut.dedup();
    if cut.len() == 1 {
        return Some(cut[0].clone());
    }
    // Last: a little junk after a label (the card's edge, a misread MR line). After the cut-off
    // rule, so "criticalchancefor" is Critical Chance for Slide Attack, not Critical Chance.
    let tail_ok = |k: &str| k.len() >= 6 && ocr.starts_with(k) && ocr.len() - k.len() <= 4;
    keys.iter().filter(|(_, k)| tail_ok(k)).max_by_key(|(_, k)| k.len()).map(|(t, _)| t.clone())
}

/// Stat tags spelled out by a riven's name: prefixes start each `-` part, the suffix ends it
/// (`Acri-hexasus` → Critical Damage, Status Chance, Slash).
pub fn name_tags(kind: &RivenType, riven_name: &str) -> Vec<String> {
    let lower = riven_name.to_lowercase();
    let parts: Vec<&str> = lower.split('-').collect();
    let last = parts.last().copied().unwrap_or_default();
    let mut tags: Vec<(usize, String)> = Vec::new();
    // Only one suffix ends the name: the longest that fits, so "loctisus" is Slash ("sus"), not
    // Heavy Attack Efficiency ("us").
    let suffix_len = kind
        .stats
        .values()
        .map(|s| s.suffix.to_lowercase())
        .filter(|sfx| !sfx.is_empty() && last.ends_with(sfx.as_str()) && last.len() > sfx.len())
        .map(|sfx| sfx.len())
        .max();
    for (tag, s) in &kind.stats {
        let p = s.prefix.to_lowercase();
        let sfx = s.suffix.to_lowercase();
        if !p.is_empty() {
            if let Some(i) = parts.iter().position(|part| part.starts_with(&p) && part.len() >= p.len()) {
                // A prefix can't also be the whole last part (that's prefix + suffix).
                tags.push((i, tag.clone()));
                continue;
            }
        }
        if !sfx.is_empty() && last.ends_with(&sfx) && last.len() > sfx.len() && Some(sfx.len()) == suffix_len {
            tags.push((usize::MAX, tag.clone()));
        }
    }
    tags.sort();
    tags.dedup_by(|a, b| a.1 == b.1);
    tags.into_iter().map(|(_, t)| t).collect()
}

/// A riven worked out from a card: either the user's own (exact) or rebuilt from the numbers.
#[derive(Debug, Clone)]
pub struct Identified {
    pub view: RivenView,
    /// The rank wasn't known, so it was worked out from the numbers.
    pub rank_assumed: bool,
}

/// One step of the number as the card shows it, in raw units: `x0.98` (0.01), `+13.5%` (0.001),
/// `+0.3 Range` (0.1).
fn display_step(stat: &CardStat, data_label: &str) -> f64 {
    match stat.unit {
        Unit::Times => 0.01,
        _ if data_label.contains("|val|%") => 0.001,
        _ => 0.1,
    }
}

fn raw_value(stat: &CardStat, data_label: &str) -> f64 {
    match stat.unit {
        Unit::Times => stat.value - 1.0,
        _ if data_label.contains("|val|%") => stat.value / 100.0,
        _ => stat.value,
    }
}

/// The riven's weapon path for a card. The title uses the base weapon name ("Burston"), so match
/// that; fall back to the logged weapon.
pub fn weapon_path(data: &RivenData, card: &Card, logged: Option<&str>) -> Option<String> {
    let want = norm(card.weapon_name());
    data.weapons
        .iter()
        .filter(|(_, w)| norm(&w.name) == want)
        .map(|(p, _)| p.clone())
        .min()
        .or_else(|| logged.filter(|p| data.weapons.contains_key(*p)).map(String::from))
}

/// The disposition the card's numbers use: the logged ("Fits in") weapon's, when it's in the same
/// weapon family as the title (Burston Prime for a Burston riven, Sancti Magistar for Magistar).
pub fn shown_disposition(data: &RivenData, card: &Card, logged: Option<&str>) -> Option<f64> {
    let logged = logged?;
    let w = data.weapons.get(logged)?;
    let same_base = weapon_path(data, card, None).is_some_and(|title| {
        let base = |p: &str| data.weapons.get(p).and_then(|w| w.base.clone()).unwrap_or_else(|| p.to_string());
        base(&title) == base(logged)
    });
    (same_base || norm(&w.name).starts_with(&norm(card.weapon_name()))).then_some(w.disposition)
}

/// The weapon named in the item-details screen's "Fits in" box (its OCR lines), from the family
/// of the card's weapon. A long name may wrap, so neighbouring lines are tried joined too. The
/// whole name must match, so "Latron Prime" isn't read as "Latron".
pub fn fits_in_weapon(data: &RivenData, card: &Card, lines: &[String]) -> Option<String> {
    let title = weapon_path(data, card, None)?;
    let mut texts: Vec<String> = lines.iter().map(|l| norm(l)).collect();
    texts.extend(lines.windows(2).map(|w| norm(&format!("{} {}", w[0], w[1]))));
    data.family(&title)
        .into_iter()
        .filter(|(_, w)| texts.contains(&norm(&w.name)))
        .max_by_key(|(_, w)| w.name.len())
        .map(|(p, _)| p.to_string())
}

/// The card's numbers depend on which variant it's shown with (Latron 1.4, Latron Prime 1.3),
/// and the log hasn't named one in the title's family.
pub fn variant_unknown(data: &RivenData, card: &Card, logged: Option<&str>) -> bool {
    shown_disposition(data, card, logged).is_none() && weapon_path(data, card, None).is_some_and(|p| data.variants_differ(&p))
}

/// Finds the card among the user's rivens (weapon + name, then the closest stat values), with
/// the numbers worked out for the weapon it's shown with.
pub fn find_own(
    data: &RivenData,
    sheet: &GradeSheet,
    owned: &[OwnedRiven],
    card: &Card,
    logged_weapon: Option<&str>,
) -> Option<RivenView> {
    let want = norm(&card.title);
    let disposition = shown_disposition(data, card, logged_weapon);
    owned
        .iter()
        .filter_map(|r| rivens::view_with(data, sheet, r, rivens::own_rank(r), disposition))
        .filter(|v| norm(&format!("{} {}", v.weapon, v.name)) == want)
        .min_by(|a, b| stat_distance(a, card).total_cmp(&stat_distance(b, card)))
}

/// How far a riven's shown numbers are from the card's (sum of differences).
fn stat_distance(v: &RivenView, card: &Card) -> f64 {
    let nums = |text: &str| stat_line(text).map(|(n, _, _)| n);
    v.stats
        .iter()
        .zip(&card.stats)
        .map(|(s, c)| nums(&s.text).map_or(100.0, |n| (n - c.value).abs()))
        .sum::<f64>()
        + 100.0 * (v.stats.len() as f64 - card.stats.len() as f64).abs()
}

/// Rebuilds a riven from the card's numbers (warframe-riven-info's `unparseBuff`/`unparseCurse`).
/// `riven_type` like `LotusRifleRandomModRare`. `rank` if known (rerolls keep the riven's rank),
/// else the highest rank where every roll lands inside its 90–110% range.
pub fn rebuild(
    data: &RivenData,
    sheet: &GradeSheet,
    riven_type: &str,
    card: &Card,
    logged_weapon: Option<&str>,
    rank: Option<u32>,
) -> Result<Identified, String> {
    let kind = data.types.get(riven_type).ok_or_else(|| format!("unknown riven type {riven_type}"))?;
    let compat = weapon_path(data, card, logged_weapon).ok_or_else(|| format!("unknown weapon \"{}\"", card.weapon_name()))?;
    let disposition =
        shown_disposition(data, card, logged_weapon).unwrap_or_else(|| data.weapons.get(&compat).map_or(1.0, |w| w.disposition));

    // Tags: from the label; a readable label that matches nothing is a trait the data doesn't
    // have yet (a new splice), shown as read and ungraded so the rest of the riven still works
    // out; a missing label (next to a red icon) from the name's stat parts not already used.
    let mut tags: Vec<Option<String>> = card.stats.iter().map(|s| match_label(kind, &s.label)).collect();
    for (i, t) in tags.iter_mut().enumerate() {
        if t.is_none() && card.stats[i].label.len() >= 6 {
            let name = card.stat_lines.get(i).map_or_else(|| card.stats[i].label.clone(), |l| label_text(l));
            *t = Some(format!("{}{name}", rivens::PENDING_PREFIX));
        }
    }
    let mut spare: Vec<String> =
        name_tags(kind, card.riven_name()).into_iter().filter(|t| !tags.iter().flatten().any(|u| u == t)).collect();
    for t in tags.iter_mut().filter(|t| t.is_none()) {
        if !spare.is_empty() {
            *t = Some(spare.remove(0));
        }
    }
    let Some(i) = tags.iter().position(Option::is_none) else {
        let tags: Vec<String> = tags.into_iter().flatten().collect();
        return rebuild_tagged(data, sheet, kind, riven_type, card, compat, disposition, rank, &tags);
    };
    // A label hidden by its icon that the name doesn't give (a curse): one of the icon stats,
    // the first whose numbers fit. Slash first: its icon hides the label entirely.
    let not_recognised = || format!("stat not recognised: \"{}\" ({})", card.stats[i].label, card.stats[i].value);
    if !card.stats[i].label.is_empty() || card.stats[i].unit != Unit::Percent {
        return Err(not_recognised());
    }
    let mut first_err = None;
    for icon in ICON_STATS.iter().filter(|t| kind.stats.contains_key(**t) && !tags.iter().flatten().any(|u| u == *t)) {
        let mut tried = tags.clone();
        tried[i] = Some(icon.to_string());
        if tried.iter().any(Option::is_none) {
            return Err(not_recognised());
        }
        let tried: Vec<String> = tried.into_iter().flatten().collect();
        match rebuild_tagged(data, sheet, kind, riven_type, card, compat.clone(), disposition, rank, &tried) {
            Ok(r) => return Ok(r),
            Err(e) => {
                first_err.get_or_insert(e);
            }
        }
    }
    Err(first_err.unwrap_or_else(not_recognised))
}

/// Stats shown with an icon before the label, which OCR may lose with it; most likely first.
const ICON_STATS: [&str; 7] = [
    "WeaponSlashDamageMod",
    "WeaponImpactDamageMod",
    "WeaponArmorPiercingDamageMod",
    "WeaponFireDamageMod",
    "WeaponFreezeDamageMod",
    "WeaponElectricityDamageMod",
    "WeaponToxinDamageMod",
];

/// `rebuild` once every card stat has a tag.
#[allow(clippy::too_many_arguments)]
fn rebuild_tagged(
    data: &RivenData,
    sheet: &GradeSheet,
    kind: &RivenType,
    riven_type: &str,
    card: &Card,
    compat: String,
    disposition: f64,
    rank: Option<u32>,
    tags: &[String],
) -> Result<Identified, String> {

    // Buff or curse: a curse goes against the stat's usual direction. Spliced traits without
    // known numbers still count towards the buff/curse totals but aren't worked out.
    let mut lines = Vec::new();
    let mut pending = Vec::new(); // (card position, tag, is_buff)
    let mut buff_at = Vec::new(); // is_buff for each card stat, in card order
    for (i, (stat, tag)) in card.stats.iter().zip(tags).enumerate() {
        let Some(info) = rivens::stat_info(kind, tag) else {
            let is_buff = if stat.unit == Unit::Times { stat.value >= 1.0 } else { stat.value >= 0.0 };
            pending.push((i, tag.clone(), is_buff));
            buff_at.push(is_buff);
            continue;
        };
        let raw = raw_value(stat, &info.label);
        let is_buff = raw.signum() == info.value.signum();
        lines.push((tag.clone(), raw, is_buff, info.value, display_step(stat, &info.label)));
        buff_at.push(is_buff);
    }
    let nb = lines.iter().filter(|l| l.2).count() + pending.iter().filter(|p| p.2).count();
    let nc = lines.len() + pending.len() - nb;
    if nb < 2 || nc > 1 {
        return Err(format!("{nb} positives and {nc} negatives"));
    }
    const BUFF: [f64; 6] = [0.0, 1.0, 0.66000003, 0.5, 0.40000001, 0.34999999];
    const CURSE_BUFF: [f64; 6] = [0.0, 1.0, 0.33000001, 0.5, 1.25, 1.5];
    let atten = 1.5 * disposition * 10.0;
    // Each stat's roll (0 = 90% of its full value, 1 = 110%), and how far that can be off
    // because the card rounds the number (x0.98 on a rank 0 riven is anywhere in ±0.01).
    let rolls = |r: u32| -> Vec<(f64, f64)> {
        let level = f64::from(r + 1);
        lines
            .iter()
            .map(|(_, raw, buff, base, step)| {
                let full = if *buff {
                    base * atten * 1.25f64.powi(nc as i32) * BUFF[nb.min(5)] * level
                } else {
                    -base * atten * CURSE_BUFF[nb.min(5)] * BUFF[nc.min(5)] * level
                };
                ((raw / full - 0.9) / 0.2, step / full.abs() / 0.2)
            })
            .collect()
    };
    let fits = |f: &[(f64, f64)]| f.iter().all(|(x, off)| (-0.12 - off..=1.12 + off).contains(x));
    let (rank, rank_assumed) = match rank {
        Some(r) => (r, false),
        None => {
            let found = (0..=rivens::MAX_RANK).rev().find(|r| fits(&rolls(*r)));
            let at_8: Vec<f64> = rolls(8).into_iter().map(|(x, _)| x).collect();
            let r = found.ok_or_else(|| format!("numbers don't fit any rank (rolls at rank 8: {at_8:?})"))?;
            (r, true)
        }
    };
    let f: Vec<f64> = rolls(rank).into_iter().map(|(x, _)| x).collect();
    let roll = |x: f64| (x.clamp(0.0, 1.0) * f64::from(0x3FFF_FFFFu32)).round() as u64;
    let mut buffs: Vec<_> = lines.iter().zip(&f).filter(|(l, _)| l.2).map(|(l, x)| json!({"Tag": l.0, "Value": roll(*x)})).collect();
    let mut curses: Vec<_> = lines.iter().zip(&f).filter(|(l, _)| !l.2).map(|(l, x)| json!({"Tag": l.0, "Value": roll(*x)})).collect();
    for (_, tag, is_buff) in &pending {
        let entry = json!({"Tag": tag, "Value": 0});
        if *is_buff { buffs.push(entry) } else { curses.push(entry) }
    }
    let mut fp = json!({"compat": compat, "lvl": rank, "buffs": buffs, "curses": curses});
    if let Some(m) = card.mastery {
        fp["lvlReq"] = json!(m);
    }
    if let Some(n) = card.rerolls {
        fp["rerolls"] = json!(n);
    }
    let owned = OwnedRiven {
        id: format!("screen:{}", card.title),
        item_type: format!("/Lotus/Upgrades/Mods/Randomized/{riven_type}"),
        fingerprint: fp.to_string(),
    };
    let mut view = rivens::view_with(data, sheet, &owned, rank, Some(disposition)).ok_or("couldn't build the riven")?;
    view.name = card.riven_name().to_string(); // the game's name, as shown
    if !pending.is_empty() {
        add_pending(&mut view, card, &pending, &buff_at);
    }
    Ok(Identified { view, rank_assumed })
}

/// Puts the not-yet-graded spliced traits back in their places on the card, as shown.
fn add_pending(view: &mut RivenView, card: &Card, pending: &[(usize, String, bool)], buff_at: &[bool]) {
    let (mut buffs, mut curses): (Vec<_>, Vec<_>) = std::mem::take(&mut view.stats).into_iter().partition(|s| s.good);
    buffs.reverse();
    curses.reverse();
    for (i, stat) in card.stats.iter().enumerate() {
        if let Some((_, tag, is_buff)) = pending.iter().find(|p| p.0 == i) {
            let name = tag.trim_start_matches(rivens::PENDING_PREFIX);
            let number = match stat.unit {
                Unit::Times => format!("x{:.2}", stat.value),
                Unit::Percent => format!("{:+}%", stat.value),
                Unit::Plain => format!("{:+}", stat.value),
            };
            view.stats.push(rivens::StatView {
                tag: tag.clone(),
                text: format!("{number} {name}"),
                good: *is_buff,
                quality: 0.0,
                letter: "?".into(),
                worst: String::new(),
                best: String::new(),
                wanted: None,
                pending: true,
            });
        } else if let Some(s) = if buff_at[i] { buffs.pop() } else { curses.pop() } {
            view.stats.push(s);
        }
    }
    view.stats.extend(buffs.into_iter().rev().chain(curses.into_iter().rev()));
    let graded: Vec<f64> = view.stats.iter().filter(|s| !s.pending).map(|s| s.quality).collect();
    view.perfectness = if graded.is_empty() { 0.0 } else { 100.0 * graded.iter().sum::<f64>() / graded.len() as f64 };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(s: &str) -> Vec<String> {
        s.split(" / ").map(String::from).collect()
    }

    #[test]
    fn parses_inspect_card() {
        // Windows OCR of docs/alecaExamples/Tools/inspectRiven.png.
        let c = parse_card(&lines(
            "Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage / +188.2% Damage / -90% Projectile Speed / MR 13 / 013",
        ))
        .unwrap();
        assert_eq!(c.title, "Torid Crita-acriata");
        assert_eq!((c.weapon_name(), c.riven_name()), ("Torid", "Crita-acriata"));
        assert_eq!(c.stats.len(), 4);
        assert_eq!(c.stats[3], CardStat { value: -90.0, unit: Unit::Percent, label: "projectilespeed".into() });
        assert_eq!((c.mastery, c.rerolls), (Some(13), Some(13)));
    }

    #[test]
    fn parses_reroll_cards_with_ocr_quirks() {
        // Windows OCR of reroll (2).png: wrapped name, lock and icon junk, a missing label.
        let c = parse_card(&lines(
            "Burston Acri- / hexasus / +143.2% / +159.2% Critical Damage / +117.3% Status Chance / O -132.2% YPuncture / MR15 / 095",
        ))
        .unwrap();
        assert_eq!(c.title, "Burston Acri-hexasus");
        assert_eq!(c.stats[0], CardStat { value: 143.2, unit: Unit::Percent, label: String::new() });
        assert_eq!(c.stats[3].value, -132.2);
        assert_eq!(c.stats[3].label, "ypuncture");
        assert_eq!((c.mastery, c.rerolls), (Some(15), Some(95)));
        // Dimmed side card: a stray space inside the number.
        let side = parse_card(&lines("Burston Crita-lexitis / +191 .1% Critical Chance / +3.4 Punch Through / +137.4% Critical Damage / O -132.2% Y Puncture / MR 15")).unwrap();
        assert_eq!(side.stats[0].value, 191.1);
        assert_eq!(side.stats[1], CardStat { value: 3.4, unit: Unit::Plain, label: "punchthrough".into() });
    }

    #[test]
    fn parses_faction_and_seconds() {
        let c = parse_card(&lines("Korrudo Tempi-deciada / x1.43 Damage to Infested / +95.8% Status Duration / +8.4s Combo Duration")).unwrap();
        assert_eq!(c.stats[0], CardStat { value: 1.43, unit: Unit::Times, label: "damagetoinfested".into() });
        assert_eq!(c.stats[2], CardStat { value: 8.4, unit: Unit::Plain, label: "comboduration".into() });
    }

    #[test]
    fn title_ignores_card_art_and_joins_wrapped_names() {
        let art_then_title = parse_card(&lines("x7 ,~ / Torid Crita-acriata / +198.6% Critical Chance / +1.2% Damage")).unwrap();
        assert_eq!(art_then_title.title, "Torid Crita-acriata");
        let weapon_wrapped = parse_card(&lines("Plague Kripath / Acricron / +101.5% Critical Damage / +190.6% Critical Chance")).unwrap();
        assert_eq!(weapon_wrapped.title, "Plague Kripath Acricron");
        let three = parse_card(&lines("Plague Kripath / Pura- / acricron / +101.5% Critical Damage / +190.6% Critical Chance")).unwrap();
        assert_eq!(three.title, "Plague Kripath Pura-acricron");
    }

    #[test]
    fn joins_wrapped_stat_labels() {
        // error1.png: "Magazine Capacity" wraps on a 4-stat Torid riven.
        let card = parse_card(&lines(
            "Torid Acri-caricron / +196.1% Critical Chance / +117.9% \"Viral / +160.3% Critical Damage / -46.2% Magazine / Capacity / MR 12 / 283",
        ))
        .unwrap();
        assert_eq!(card.stats.len(), 4);
        assert_eq!(card.stats[3].label, "magazinecapacity");
        assert_eq!(card.stats[1].label, "viral");
        assert_eq!((card.mastery, card.rerolls), (Some(12), Some(283)));
    }

    #[test]
    fn joins_bracketed_and_wrapped_melee_labels() {
        // error2.png: OCR of a Magistar riven.
        let card = parse_card(&lines(
            "Magistar Acri- / paracron / +110.3% Critical Damage / +28.7 Initial Combo / +209.3% Critical Chance / (x2 for Heavy Attacks) / -129% Critical Chance for / Slide Attack / MR 10 / 0113 \\",
        ))
        .unwrap();
        assert_eq!(card.title, "Magistar Acri-paracron");
        let labels: Vec<&str> = card.stats.iter().map(|s| s.label.as_str()).collect();
        assert_eq!(labels, ["criticaldamage", "initialcombo", "criticalchancexforheavyattacks", "criticalchanceforslideattack"]);
        assert_eq!((card.mastery, card.rerolls), (Some(10), Some(113)));
    }

    #[test]
    fn keeps_wrapped_lines_as_read() {
        // Latron riven, 2026-10-09: "Fire Rate (x2 for" / "Bows)".
        let card = parse_card(&lines(
            "Latron Cari-critadra / +196.3% Critical Chance / +119.7% \"Viral / +70.5% Fire Rate (x2 for / Bows) / -127.6% YPuncture / MR 8",
        ))
        .unwrap();
        assert_eq!(card.stats[2].label, "fireratexforbows");
        assert_eq!(card.stat_lines[2], "+70.5% Fire Rate (x2 for Bows)");
        assert_eq!(card.stat_lines.len(), 4);
    }

    #[test]
    fn variants_need_the_logged_weapon() {
        let d = rivens::data();
        let d = &*d;
        let latron = parse_card(&lines("Latron Cari-critadra / +196.3% Critical Chance / +119.7% Viral")).unwrap();
        assert!(variant_unknown(d, &latron, None));
        assert!(!variant_unknown(d, &latron, Some("/Lotus/Weapons/Tenno/Rifle/LatronPrime")));
        assert_eq!(shown_disposition(d, &latron, Some("/Lotus/Weapons/Tenno/Rifle/LatronPrime")), Some(1.3));
        // A weapon from another family (the previous riven's) doesn't count.
        assert!(variant_unknown(d, &latron, Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston")));
        // Sancti Magistar shares Magistar's base though its name doesn't start with it.
        let magistar = parse_card(&lines("Magistar Acri-paracron / +110.3% Critical Damage / +28.7 Initial Combo")).unwrap();
        assert_eq!(shown_disposition(d, &magistar, Some("/Lotus/Weapons/Syndicates/NewLoka/Melee/NLMagistar")), Some(1.25));
        let toxo = parse_card(&lines("Dual Toxocyst Crita-croniata / +95.7% Fire Rate / +196.5% Critical Chance")).unwrap();
        assert!(!variant_unknown(d, &toxo, None));
    }

    #[test]
    fn the_weapon_gives_the_riven_type() {
        // overlay.log 2026-10-09: read 1.9 s after a Burston (rifle) riven, the stale rifle type
        // made "numbers don't fit any rank". The weapon's own type fits at rank 8.
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines(
            "Dual Toxocyst Crita- / croniata / +95.7% Fire Rate / +196.5% Critical Chance / +261.4% Damage / -87.5% Ammo Maximum / MR 16 / 029 \\",
        ))
        .unwrap();
        let weapon = weapon_path(d, &card, None).unwrap();
        assert_eq!(d.kind_of(&weapon), Some("LotusPistolRandomModRare"));
        let sheet = GradeSheet::default();
        assert!(rebuild(d, &sheet, "LotusRifleRandomModRare", &card, None, None).is_err());
        let r = rebuild(d, &sheet, "LotusPistolRandomModRare", &card, None, None).unwrap();
        let letters: Vec<&str> = r.view.stats.iter().map(|s| s.letter.as_str()).collect();
        assert_eq!((r.view.rank, letters), (8, vec!["B", "A-", "C", "A-"]));
    }

    #[test]
    fn weapon_names_with_numbers_are_titles() {
        // overlay.log 2026-10-10: "Ax-52" read as a "-52" stat, so the card had no title and
        // wasn't taken for a riven ("other lilac text").
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines(
            "Ax-52 Toxi-vexidex / +8.7% z Toxin / +7.4% Status Chance / +8.5% f Electricity / -12.7% Damage / MR 11",
        ))
        .unwrap();
        assert_eq!(card.title, "Ax-52 Toxi-vexidex");
        assert_eq!(card.stats.len(), 4);
        assert_eq!(card.stats[3], CardStat { value: -12.7, unit: Unit::Percent, label: "damage".into() });
        let weapon = weapon_path(d, &card, None).unwrap();
        assert_eq!(d.kind_of(&weapon), Some("LotusRifleRandomModRare"));
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &card, None, None).unwrap();
        assert_eq!(r.view.stats.len(), 4);
    }

    #[test]
    fn fits_in_names_the_variant() {
        let d = rivens::data();
        let d = &*d;
        let latron = parse_card(&lines("Latron Cari-critadra / +196.3% Critical Chance / +119.7% Viral")).unwrap();
        let read = |text: &str| fits_in_weapon(d, &latron, &lines(text));
        assert_eq!(read("FITS IN / Latron Prime").as_deref(), Some("/Lotus/Weapons/Tenno/Rifle/LatronPrime"));
        assert_eq!(read("FITS IN / Latron").as_deref(), Some("/Lotus/Weapons/Tenno/Rifle/SemiAutoRifle"));
        assert_eq!(read("Latron / Wraith").as_deref(), Some("/Lotus/Weapons/Tenno/LongGuns/WraithLatron/WraithLatron"), "wrapped");
        assert_eq!(read("FITS IN / Boltor Prime"), None, "another family");
        assert_eq!(read("FITS IN"), None);
    }

    #[test]
    fn new_spliced_traits_dont_stop_the_rest() {
        // overlay.log 2026-10-09: "Slam Attack Damage" was read as Damage, so nothing fit.
        let d = rivens::data();
        let d = &*d;
        let melee = &d.types["PlayerMeleeWeaponRandomModRare"];
        assert_eq!(match_label(melee, "slamattackdamage").as_deref(), Some("Pending:Slam Attack Damage"));
        assert_eq!(match_label(melee, "reloadwhileholstered").as_deref(), Some("Pending:Reload While Holstered"));
        let card = parse_card(&lines(
            "Magistar Crita- / acrinus / +114.4% Critical Damage / +139.9% Slam Attack Damage / +235.3% Critical Chance / (x2 for Heavy Attacks) / -91.3% Status Duration / MR 9",
        ))
        .unwrap();
        let magistar = Some("/Lotus/Weapons/Tenno/Melee/Maces/PaladinMace/PaladinMaceWeapon");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &card, magistar, None).unwrap();
        let pending: Vec<&str> = r.view.stats.iter().filter(|s| s.pending).map(|s| s.text.as_str()).collect();
        assert_eq!(pending, ["+139.9% Slam Attack Damage"]);
        assert_eq!(r.view.stats.iter().filter(|s| !s.pending).count(), 3);
    }

    #[test]
    fn unknown_labels_wait_for_data() {
        // A trait no list has yet: shown as read, ungraded; the rest still works out.
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines("Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage / +60.2% Brand New Thing / -90% Projectile Speed")).unwrap();
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &card, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
        let pending: Vec<&str> = r.view.stats.iter().filter(|s| s.pending).map(|s| s.text.as_str()).collect();
        assert_eq!(pending, ["+60.2% Brand New Thing"]);
        assert_eq!(label_text("x1.43 Damage to Infested"), "Damage to Infested");
    }

    #[test]
    fn misread_mr_line_and_percent() {
        // overlay.log 2026-10-09, Laetum: "MR 10" read as "MRIO" was joined onto the wrapped
        // "Magazine / Capacity", which then matched nothing ("collecting data").
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines(
            "Laetum Toxiata / +57.70/0 \u{2022}a. Toxin / +124.7% Damage / -12.7% Magazine / Capacity / 0160 / MRIO",
        ))
        .unwrap();
        assert_eq!(card.mastery, Some(10));
        assert_eq!(card.stats[2].label, "magazinecapacity");
        assert_eq!(card.stats[0], CardStat { value: 57.7, unit: Unit::Percent, label: "atoxin".into() });
        // The Toxin label lost entirely: a bare percentage, filled from the name.
        let bare = parse_card(&lines("Laetum Toxiata / +57.70/0 / +124.7% Damage / -12.7% Magazine / Capacity / MR 10 / 0160 \\")).unwrap();
        assert_eq!(bare.stats[0], CardStat { value: 57.7, unit: Unit::Percent, label: String::new() });
        let r = rebuild(d, &GradeSheet::default(), "LotusPistolRandomModRare", &bare, None, None).unwrap();
        let texts: Vec<(&str, bool)> = r.view.stats.iter().map(|s| (s.text.as_str(), s.pending)).collect();
        assert!(texts.contains(&("-12.7% Magazine Capacity", false)), "{texts:?}");
        assert!(texts.iter().any(|(t, p)| t.ends_with("Toxin") && !p), "{texts:?}");
        // Trailing junk on a label still matches.
        let pistol = &d.types["LotusPistolRandomModRare"];
        assert_eq!(match_label(pistol, "magazinecapacitymrio").as_deref(), Some("WeaponClipMaxMod"));
        assert_eq!(ocr_number("IO"), Some(10));
    }

    #[test]
    fn rejects_non_cards() {
        assert!(parse_card(&lines("ITEM DETAILS / Rifle Riven Mod / TRADEABLE")).is_none());
    }

    #[test]
    fn labels_and_name_parts_map_to_tags() {
        let d = rivens::data();
        let d = &*d;
        let rifle = &d.types["LotusRifleRandomModRare"];
        assert_eq!(match_label(rifle, "criticaldamage").as_deref(), Some("WeaponCritDamageMod"));
        assert_eq!(match_label(rifle, "ypuncture").as_deref(), Some("WeaponArmorPiercingDamageMod"), "icon junk before the label");
        assert_eq!(match_label(rifle, "damage").as_deref(), Some("WeaponDamageAmountMod"));
        let tags = name_tags(rifle, "Acri-hexasus");
        assert!(tags.contains(&"WeaponCritDamageMod".to_string()));
        assert!(tags.contains(&"WeaponStunChanceMod".to_string()));
        assert!(tags.contains(&"WeaponSlashDamageMod".to_string()), "{tags:?}");
    }

    #[test]
    fn rebuild_reproduces_a_known_riven() {
        // The user's Torid Crita-acriata (rank 8): rebuilding from the card's numbers must give
        // back the same stats and a rank of 8.
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines(
            "Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage / +188.2% Damage / -90% Projectile Speed / MR 13 / 013",
        ))
        .unwrap();
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &card, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
        assert_eq!(r.view.rank, 8);
        let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["+198.6% Critical Chance", "+152.1% Critical Damage", "+188.2% Damage", "-90% Projectile Speed"]);
        assert!(!r.view.stats[3].good, "the negative is a curse");
        assert_eq!(r.view.name, "Crita-acriata");
    }

    #[test]
    fn rebuild_fills_a_missing_label_from_the_name() {
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(&lines(
            "Burston Acri- / hexasus / +143.2% / +159.2% Critical Damage / +117.3% Status Chance / O -132.2% YPuncture / MR15 / 095",
        ))
        .unwrap();
        let prime = Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &card, prime, Some(8)).unwrap();
        assert_eq!(r.view.compat, "/Lotus/Weapons/Tenno/Rifle/BurstRifle", "the riven is for the Burston family");
        assert_eq!(r.view.stats[0].tag, "WeaponSlashDamageMod");
        assert_eq!(r.view.stats[0].text, "+143.2% Slash");
        assert_eq!(r.view.stats[3].text, "-132.2% Puncture");
    }
}

#[cfg(test)]
mod spliced_tests {
    use super::*;

    fn card(s: &str) -> Card {
        parse_card(&s.split(" / ").map(String::from).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn spliced_element_is_graded_like_heat() {
        // From the user's overlay.log (2026-10-09): a chat-linked Torid with spliced Viral.
        let d = rivens::data();
        let d = &*d;
        let c = card("Torid Cari-critatis / +196.1% Critical Chance / +117.9% \"Viral / +144.9% Critical Damage / -87.9% Projectile Speed");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &c, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
        let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["+196.1% Critical Chance", "+117.9% Viral", "+144.9% Critical Damage", "-87.9% Projectile Speed"]);
        assert!(r.view.stats.iter().all(|s| !s.pending));
        assert_ne!(r.view.stats[1].letter, "?");
    }

    #[test]
    fn other_spliced_traits_wait_for_data() {
        // Status Damage must not be read as Damage; the other stats still get letters.
        let d = rivens::data();
        let d = &*d;
        let rifle = &d.types["LotusRifleRandomModRare"];
        assert_eq!(match_label(rifle, "statusdamage").as_deref(), Some("Pending:Status Damage"));
        assert_eq!(match_label(rifle, "damagevsorokin").as_deref(), Some("Pending:Damage to Orokin"));
        let c = card("Verglas Plaga-critatis / +134.1% Critical Damage / +185.1% Critical Chance / +1 19.3% Status Damage / -46.7% Reload Speed");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &c, None, None).unwrap();
        let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["+134.1% Critical Damage", "+185.1% Critical Chance", "+119.3% Status Damage", "-46.7% Reload Speed"]);
        assert!(r.view.stats[2].pending && r.view.stats[2].good);
        assert!(r.view.stats.iter().filter(|s| !s.pending).all(|s| s.letter != "?"));
    }
}

#[cfg(test)]
mod own_tests {
    use super::*;

    #[test]
    fn shown_weapon_sets_the_disposition() {
        // reroll (1).png shows a Burston riven while "Fits in: Burston Prime" (1.35, not 1.45).
        let d = rivens::data();
        let d = &*d;
        let card = parse_card(
            &"Burston Crita-lexitis / +191.1% Critical Chance / +3.4 Punch Through / MR15"
                .split(" / ")
                .map(String::from)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let prime = Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston");
        assert_eq!(shown_disposition(d, &card, prime), Some(1.35));
        assert_eq!(shown_disposition(d, &card, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon")), None, "other weapon");
    }
}

#[cfg(test)]
mod slash_icon_tests {
    use super::*;

    fn card(s: &str) -> Card {
        parse_card(&s.split(" / ").map(String::from).collect::<Vec<_>>()).unwrap()
    }

    #[test]
    fn hidden_slash_buff_comes_from_the_longest_suffix() {
        // riven_misfits.jsonl 2026-10-10: the Slash label hid behind its icon; "loctisus" ends in
        // both "sus" (Slash) and "us" (Heavy Attack Efficiency), and the wrong one was taken.
        let d = rivens::data();
        let d = &*d;
        let melee = &d.types["PlayerMeleeWeaponRandomModRare"];
        let tags = name_tags(melee, "Crita-loctisus");
        assert!(tags.contains(&"WeaponSlashDamageMod".to_string()), "{tags:?}");
        assert!(!tags.contains(&"WeaponMeleeComboEfficiencyMod".to_string()), "{tags:?}");
        let c = card("Dual Heat Swords / Crita-loctisus / +29.2% Critical Chance / (x2 for Heavy Attacks) / +0.3 Range / +17.9% / -10.4% Status Chance / MR 10");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &c, None, None).unwrap();
        assert_eq!(r.view.stats[2].text, "+17.9% Slash");
    }

    #[test]
    fn wiki_disposition_fits_a_changed_weapon() {
        // riven_misfits.jsonl 2026-10-10: Haalvu went 0.5 → 0.6 in September 2026; the export
        // still said 0.5, so nothing fit. The bundled data now takes the wiki's 0.6.
        let d = rivens::data();
        let d = &*d;
        let c = card("Haalvu Acri-toxicron / +85.3% Critical Chance / +52.2% 'Toxin / +74.2% Critical Damage / -27% Zoom / MR 12 / 016 \\");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &c, None, None).unwrap();
        assert_eq!(r.view.rank, 8);
        let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(texts, ["+85.3% Critical Chance", "+52.2% Toxin", "+74.2% Critical Damage", "-27% Zoom"]);
    }

    #[test]
    fn a_label_without_its_number_is_not_a_wrap() {
        // riven_misfits.jsonl 2026-10-10: read while fading in, "x0.98" was missing and
        // "Damage to Infested" was joined onto Critical Chance as a new trait.
        let early = card("Kompressa Critado / +7.9% *Cold / +13.5% Critical Chance / Damage to Infested / MR 13");
        assert_eq!(early.stats[1].label, "criticalchance");
        assert_eq!(early.stat_lines[1], "+13.5% Critical Chance");
        assert_eq!(early.lost_numbers, 1);
        let full = card("Kompressa Critado / +7.9% *Cold / +13.5% Critical Chance / x0.98 Damage to Infested / MR 13 / 02");
        assert_eq!(full.lost_numbers, 0);
        assert_eq!(full.stats[2], CardStat { value: 0.98, unit: Unit::Times, label: "damagetoinfested".into() });
        // Its numbers are Kompressa Prime's (0.6), not Kompressa's (1.15): the overlay tries
        // the variants when the base weapon doesn't fit.
        let d = rivens::data();
        let d = &*d;
        let sheet = GradeSheet::default();
        assert!(variant_unknown(d, &full, None));
        assert!(rebuild(d, &sheet, "LotusPistolRandomModRare", &full, None, None).is_err());
        let prime = Some("/Lotus/Weapons/Tenno/Pistols/PrimeKompressa/PrimeKompressa");
        let r = rebuild(d, &sheet, "LotusPistolRandomModRare", &full, prime, None).unwrap();
        assert_eq!((r.view.rank, r.view.disposition), (0, 0.6));
        // Wrapped labels still join.
        assert_eq!(card("Torid Acri-caricron / +196.1% Critical Chance / -46.2% Magazine / Capacity").lost_numbers, 0);
    }

    #[test]
    fn hidden_slash_curse_is_tried_as_an_icon_stat() {
        // riven_misfits.jsonl 2026-10-10: "-114% Slash" read as "-114%"; curses aren't in the name.
        let d = rivens::data();
        let d = &*d;
        let c = card("Ocucor Acri-corican / +108.9% Critical Damage / +108.8% OCorrosive / +144.2% Multishot / -114% / MR 15 / 0 394");
        let r = rebuild(d, &GradeSheet::default(), "LotusPistolRandomModRare", &c, None, None).unwrap();
        let texts: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert!(texts.contains(&"-114% Slash"), "{texts:?}");
        assert!(r.view.stats.iter().all(|s| !s.pending), "{texts:?}");
    }
}
