//! Riven grades (Great / Good / OK / Bad) from the community "good riven rolls" sheet by
//! 44bananas and Xennethkeisere, the same source AlecaFrame credits.
//!
//! The sheet has one tab per weapon class (primary, secondary, melee, archgun, robotic). Each row
//! is a weapon, its wanted positives as patterns, and its harmless negatives:
//!
//! - `CD MS/TOX/DMG` → CD is required, the rest of the 3 positives come from MS, TOX or DMG.
//! - `CD AS RANGE` → all three required.
//! - `… or …` → alternative builds.
//! - Negatives `ZOOM/REC/IMP` → any of these is harmless.
//!
//! The sheet's rule of thumb: a riven is good with at least two wanted positives, a harmless
//! negative, and a third positive that isn't dead weight. The cut-offs between grades are this
//! app's reading of that rule and AlecaFrame's grade descriptions (its own code isn't public).

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::arbitration::cached_text;

const SHEET: &str = "https://docs.google.com/spreadsheets/d/1zbaeJBuBn44cbVKzJins_E3hTDpnmvOk8heYN-G8yy8";
pub const TABS: [&str; 5] = ["primary", "secondary", "melee", "archgun", "robotic"];

/// One weapon's row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rule {
    /// Alternative builds: (required codes, optional pool codes).
    pub combos: Vec<(Vec<String>, Vec<String>)>,
    pub harmless: Vec<String>,
}

/// Tab → normalised weapon name → rule.
#[derive(Debug, Default)]
pub struct GradeSheet {
    pub tabs: HashMap<String, HashMap<String, Rule>>,
}

/// Loads each tab from the cache in `dir`, downloading it every 2 weeks.
pub fn load_or_refresh(dir: &Path) -> GradeSheet {
    let _ = std::fs::create_dir_all(dir);
    let mut sheet = GradeSheet::default();
    for tab in TABS {
        let url = format!("{SHEET}/gviz/tq?tqx=out:csv&sheet={tab}");
        let valid = |t: &str| t.to_uppercase().contains("POSITIVE STATS") && parse_tab(t).len() > 5;
        if let Some(text) = cached_text(dir, &format!("{tab}.csv"), &url, valid) {
            sheet.tabs.insert(tab.to_string(), parse_tab(&text));
        }
    }
    sheet
}

/// Weapon name as the sheet writes it: lowercase, no "(Melee)"-style suffix, no "Prime".
pub fn normalise(name: &str) -> String {
    let base = name.split('(').next().unwrap_or(name);
    base.to_lowercase().split_whitespace().filter(|w| *w != "prime").collect::<Vec<_>>().join(" ")
}

/// Splits CSV text into rows (quoted fields, doubled quotes, commas and newlines inside quotes).
pub fn csv_rows(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let (mut row, mut field, mut quoted) = (Vec::new(), String::new(), false);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            ('\r', false) => {}
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Parses one tab. The positive and negative columns are found from the header row.
pub fn parse_tab(text: &str) -> HashMap<String, Rule> {
    let rows = csv_rows(text);
    let Some(header) = rows.first() else { return HashMap::new() };
    let col = |name: &str| header.iter().position(|h| h.trim().eq_ignore_ascii_case(name));
    let (Some(pos), Some(neg)) = (col("POSITIVE STATS:"), col("NEGATIVE STATS:")) else { return HashMap::new() };
    rows[1..]
        .iter()
        .filter_map(|r| {
            let weapon = normalise(r.first()?);
            let positives = r.get(pos)?.trim();
            if weapon.is_empty() || positives.is_empty() {
                return None;
            }
            Some((weapon, Rule { combos: parse_combos(positives), harmless: codes(r.get(neg).map_or("", |s| s))}))
        })
        .collect()
}

fn codes(s: &str) -> Vec<String> {
    s.split(|c: char| c == '/' || c == ',' || c.is_whitespace())
        .filter(|c| !c.is_empty())
        .map(|c| c.trim().to_uppercase())
        .collect()
}

fn parse_combos(s: &str) -> Vec<(Vec<String>, Vec<String>)> {
    s.split(" or ")
        .flat_map(|c| c.split(" OR "))
        .map(|combo| {
            let (mut required, mut pool) = (Vec::new(), Vec::new());
            for group in combo.split_whitespace() {
                if group.contains('/') {
                    pool.extend(codes(group));
                } else {
                    required.push(group.to_uppercase());
                }
            }
            (required, pool)
        })
        .filter(|(r, p)| !r.is_empty() || !p.is_empty())
        .collect()
}

/// Stats wanted on every weapon whatever its row says: the combined elements and weak point
/// stats are spliced riven traits newer than the sheet. (Melee rivens never roll weak point.)
const ALWAYS_WANTED: [&str; 8] = [
    "WeaponBlastDamageMod",
    "WeaponCorrosiveDamageMod",
    "WeaponViralDamageMod",
    "WeaponGasDamageMod",
    "WeaponMagneticDamageMod",
    "WeaponRadiationDamageMod",
    "WeaponWeakpointCriticalChance",
    "WeaponWeakpointDamage",
];
/// Of those, the ones every weapon's ideal positives list as best in slot.
const ALWAYS_BEST: [&str; 2] = ["CORR", "VIRAL"];
/// Never wanted, whatever a row says: the new damage-to-faction traits.
const NEVER_WANTED: [&str; 3] = ["WeaponFactionDamageOrokin", "WeaponFactionDamageScaldra", "WeaponFactionDamageTechrot"];

/// A sheet code that stands only for [`NEVER_WANTED`] stats (DTO, DTS, DTT): left out of builds.
fn excluded(code: &str) -> bool {
    let tags = code_tags(code);
    !tags.is_empty() && tags.iter().all(|t| NEVER_WANTED.contains(t))
}

/// A positive the weapon wants: on its row, or one of [`ALWAYS_WANTED`], and never [`NEVER_WANTED`].
fn wanted_by(codes: &[&String], tag: &str) -> bool {
    !NEVER_WANTED.contains(&tag) && (ALWAYS_WANTED.contains(&tag) || codes.iter().any(|c| matches(c, tag)))
}

/// Riven stat tags a sheet code stands for.
pub fn code_tags(code: &str) -> &'static [&'static str] {
    match code {
        "BLAST" => &["WeaponBlastDamageMod"],
        "CORR" | "CORRO" => &["WeaponCorrosiveDamageMod"],
        "VIRAL" => &["WeaponViralDamageMod"],
        "GAS" => &["WeaponGasDamageMod"],
        "MAGN" => &["WeaponMagneticDamageMod"],
        "RAD" => &["WeaponRadiationDamageMod"],
        "WPCC" => &["WeaponWeakpointCriticalChance"],
        "WPD" | "WEAKDMG" => &["WeaponWeakpointDamage"],
        "SDMG" => &["WeaponStatusDamage"],
        "SLAMDAM" => &["WeaponMeleeSlamDamageMod"],
        "DTO" => &["WeaponFactionDamageOrokin"],
        "DTS" => &["WeaponFactionDamageScaldra"],
        "DTT" => &["WeaponFactionDamageTechrot"],
        "CD" => &["WeaponCritDamageMod"],
        "CC" => &["WeaponCritChanceMod"],
        "DMG" => &["WeaponDamageAmountMod", "WeaponMeleeDamageMod"],
        "MS" => &["WeaponFireIterationsMod"],
        "FR" | "AS" => &["WeaponFireRateMod"],
        "RLS" => &["WeaponReloadSpeedMod"],
        "TOX" => &["WeaponToxinDamageMod"],
        "HEAT" => &["WeaponFireDamageMod"],
        "COLD" => &["WeaponFreezeDamageMod"],
        "ELEC" => &["WeaponElectricityDamageMod"],
        "ELEMENT" => &["WeaponToxinDamageMod", "WeaponFireDamageMod", "WeaponFreezeDamageMod", "WeaponElectricityDamageMod"],
        "DTC" => &["WeaponFactionDamageCorpus", "WeaponMeleeFactionDamageCorpus"],
        "DTG" => &["WeaponFactionDamageGrineer", "WeaponMeleeFactionDamageGrineer"],
        "DTI" => &["WeaponFactionDamageInfested", "WeaponMeleeFactionDamageInfested"],
        "PUNC" => &["WeaponArmorPiercingDamageMod"],
        "IMP" => &["WeaponImpactDamageMod"],
        "SLASH" => &["WeaponSlashDamageMod"],
        "MAG" => &["WeaponClipMaxMod"],
        "AMMO" => &["WeaponAmmoMaxMod"],
        "REC" | "RECOIL" => &["WeaponRecoilReductionMod"],
        "SC" => &["WeaponStunChanceMod"],
        "SD" => &["WeaponProcTimeMod"],
        "PT" => &["WeaponPunctureDepthMod"],
        "PFS" => &["WeaponProjectileSpeedMod"],
        "ZOOM" => &["WeaponZoomFovMod"],
        "RANGE" => &["WeaponMeleeRangeIncMod"],
        "IC" => &["WeaponMeleeComboInitialBonusMod"],
        "EFF" => &["WeaponMeleeComboEfficiencyMod"],
        "SLIDE" => &["SlideAttackCritChanceMod"],
        "FIN" => &["WeaponMeleeFinisherDamageMod"],
        _ => &[],
    }
}

fn matches(code: &str, tag: &str) -> bool {
    code_tags(code).contains(&tag)
}

/// Sheet tab for a riven type label.
pub fn tab_for(kind: &str) -> &'static str {
    match kind {
        "Rifle" | "Shotgun" => "primary",
        "Pistol" | "Kitgun" => "secondary",
        "Melee" | "Zaw" => "melee",
        "Archgun" => "archgun",
        _ => "robotic",
    }
}

/// The sheet's row for a weapon (riven type label like "Rifle", weapon name like "Burston Prime").
pub fn rule_for<'a>(sheet: &'a GradeSheet, kind: &str, weapon: &str) -> Option<&'a Rule> {
    sheet.tabs.get(tab_for(kind)).and_then(|t| t.get(&normalise(weapon)))
}

/// Readable name for a sheet code, as the overlay's "best attributes" shows it.
pub fn code_name(code: &str) -> String {
    match code {
        "BLAST" => "Blast",
        "CORR" | "CORRO" => "Corrosive",
        "VIRAL" => "Viral",
        "GAS" => "Gas",
        "MAGN" => "Magnetic",
        "RAD" => "Radiation",
        "WPCC" => "Weak point crit chance",
        "WPD" | "WEAKDMG" => "Weak point damage",
        "SDMG" => "Status damage",
        "SLAMDAM" => "Slam damage",
        "DTO" => "Damage to Orokin",
        "DTS" => "Damage to Scaldra",
        "DTT" => "Damage to Techrot",
        "CD" => "Crit damage",
        "CC" => "Crit chance",
        "DMG" => "Damage",
        "MS" => "Multishot",
        "FR" => "Fire rate",
        "AS" => "Attack speed",
        "RLS" => "Reload speed",
        "TOX" => "Toxin",
        "HEAT" => "Heat",
        "COLD" => "Cold",
        "ELEC" => "Electricity",
        "ELEMENT" => "Any element",
        "DTC" => "Damage to Corpus",
        "DTG" => "Damage to Grineer",
        "DTI" => "Damage to Infested",
        "PUNC" => "Puncture",
        "IMP" => "Impact",
        "SLASH" => "Slash",
        "MAG" => "Magazine",
        "AMMO" => "Ammo max",
        "REC" | "RECOIL" => "Recoil",
        "SC" => "Status chance",
        "SD" => "Status duration",
        "PT" => "Punch through",
        "PFS" => "Projectile speed",
        "ZOOM" => "Zoom",
        "RANGE" => "Range",
        "IC" => "Initial combo",
        "EFF" => "Heavy attack efficiency",
        "SLIDE" => "Slide crit",
        "FIN" => "Finisher damage",
        other => other,
    }
    .to_string()
}

/// A weapon's best riven stats from the sheet, marked with what one riven has: for the overlay
/// panel and the Rivens popup.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestStats {
    /// Alternative builds (the sheet's "… or …").
    pub builds: Vec<BestBuild>,
    /// Best on every weapon besides its builds: Corrosive and Viral.
    pub always: Vec<BestStat>,
    /// Negatives that do no harm on this weapon.
    pub harmless: Vec<BestStat>,
    /// The build this riven comes closest to.
    pub matched: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestBuild {
    /// Stats the build needs.
    pub required: Vec<BestStat>,
    /// Stats that fill its other positive slots well.
    pub optional: Vec<BestStat>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BestStat {
    pub name: String,
    /// The riven has it (as a positive, or for `harmless`, as its negative).
    pub has: bool,
}

/// The weapon's best stats with what this riven has (positive tags `buffs`, negative `curses`).
pub fn best_stats(rule: &Rule, buffs: &[&str], curses: &[&str]) -> BestStats {
    let stat = |code: &String, tags: &[&str]| BestStat { name: code_name(code), has: tags.iter().any(|t| matches(code, t)) };
    let builds: Vec<BestBuild> = rule
        .combos
        .iter()
        .map(|(req, pool)| BestBuild {
            required: req.iter().filter(|c| !excluded(c)).map(|c| stat(c, buffs)).collect(),
            optional: pool.iter().filter(|c| !excluded(c)).map(|c| stat(c, buffs)).collect(),
        })
        .collect();
    // Closest build: most required stats, then most stats overall.
    let score = |b: &BestBuild| {
        let req = b.required.iter().filter(|s| s.has).count();
        (req == b.required.len(), req, req + b.optional.iter().filter(|s| s.has).count())
    };
    let matched = (0..builds.len()).max_by_key(|&i| (score(&builds[i]), std::cmp::Reverse(i))).unwrap_or(0);
    let always = ALWAYS_BEST.iter().map(|c| stat(&c.to_string(), buffs)).collect();
    BestStats { harmless: rule.harmless.iter().map(|c| stat(c, curses)).collect(), builds, always, matched }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub grade: &'static str,
    /// Per positive (in order): wanted by the matching build.
    pub wanted: Vec<bool>,
    /// The negative is on the weapon's harmless list (or there is none).
    pub harmless_negative: bool,
}

/// Grades a riven's positives and negative against a weapon's rule.
pub fn grade(rule: &Rule, buffs: &[&str], curses: &[&str]) -> Graded {
    let harmless_negative = curses.iter().all(|c| rule.harmless.iter().any(|h| matches(h, c)));

    // Best build: one with its required stats first, then the most wanted positives.
    let mut best: Option<(bool, usize, Vec<bool>)> = None;
    for (required, pool) in &rule.combos {
        let has_required = required.iter().filter(|r| !excluded(r)).all(|r| buffs.iter().any(|b| matches(r, b)));
        let codes: Vec<&String> = required.iter().chain(pool).collect();
        let wanted: Vec<bool> = buffs.iter().map(|b| wanted_by(&codes, b)).collect();
        let count = wanted.iter().filter(|w| **w).count();
        if best.as_ref().is_none_or(|(r, c, _)| (has_required, count) > (*r, *c)) {
            best = Some((has_required, count, wanted));
        }
    }
    let (has_required, count, wanted) = best.unwrap_or((false, 0, vec![false; buffs.len()]));

    // Great: a full build and a harmless negative. Good: a build's required stats plus another
    // wanted one, harmless negative (third positive is dead weight). OK: some wanted stats but a
    // missing required stat or a harmful negative. Bad: nothing wanted, or one wanted stat with a
    // harmful negative.
    let grade = match () {
        _ if count == 0 => "Bad",
        _ if has_required && harmless_negative && count == buffs.len() => "Great",
        _ if has_required && harmless_negative && count >= 2 => "Good",
        _ if harmless_negative || count >= 2 => "OK",
        _ => "Bad",
    };
    Graded { grade, wanted, harmless_negative }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TAB: &str = "\"WEAPON\",\"POSITIVE STATS:\",\"\",\"NEGATIVE STATS:\",\"Notes:\"\n\
        \"torid\",\"CD MS/CC/DMG/FR/TOX\",\"\",\"ZOOM/REC/MAG\",\"(NOTE: a, b)\"\n\
        \"kronen\",\"CD AS RANGE or CC/CD/IC/DMG\",\"\",\"EFF/SLIDE/FIN\",\"\"\n";

    #[test]
    fn best_stats_mark_what_the_riven_has() {
        let rules = parse_tab(TAB);
        let kronen = &rules["kronen"];
        // Crit chance + crit damage + initial combo, impact negative.
        let b = best_stats(kronen, &["WeaponCritChanceMod", "WeaponCritDamageMod", "WeaponMeleeComboInitialBonusMod"], &["WeaponImpactDamageMod"]);
        assert_eq!(b.builds.len(), 2);
        assert_eq!(b.matched, 1, "the CC/CD/IC/DMG build fits better than CD AS RANGE");
        let names: Vec<(&str, bool)> = b.builds[0].required.iter().map(|s| (s.name.as_str(), s.has)).collect();
        assert_eq!(names, [("Crit damage", true), ("Attack speed", false), ("Range", false)]);
        assert!(b.harmless.iter().all(|s| !s.has), "impact isn't harmless on Kronen");
        let torid = best_stats(&rules["torid"], &["WeaponCritDamageMod", "WeaponViralDamageMod"], &["WeaponZoomFovMod"]);
        assert!(torid.harmless.iter().any(|s| s.name == "Zoom" && s.has));
        let always: Vec<(&str, bool)> = torid.always.iter().map(|s| (s.name.as_str(), s.has)).collect();
        assert_eq!(always, [("Corrosive", false), ("Viral", true)]);
    }

    #[test]
    fn sheet_codes_for_new_stats() {
        // Rows as the sheet now writes them (Larkspur, Jat Kittag).
        let rules = parse_tab(
            "\"WEAPON\",\"POSITIVE STATS:\",\"NEGATIVE STATS:\"\n\
             \"larkspur\",\"MS DMG/TOX/SC/FR/CORRO/VIRAL/SDMG/WEAKDMG\",\"ZOOM\"\n\
             \"jat kittag\",\"TOX DTG/DTC/DTI/DTT/DTO/DTS/SLAMDAM\",\"ZOOM\"\n",
        );
        let lark = best_stats(&rules["larkspur"], &["WeaponCorrosiveDamageMod"], &[]);
        let names: Vec<&str> = lark.builds[0].optional.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Damage", "Toxin", "Status chance", "Fire rate", "Corrosive", "Viral", "Status damage", "Weak point damage"]);
        assert!(lark.builds[0].optional.iter().any(|s| s.name == "Corrosive" && s.has), "CORRO is Corrosive");
        let jat = best_stats(&rules["jat kittag"], &[], &[]);
        let names: Vec<&str> = jat.builds[0].optional.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Damage to Grineer", "Damage to Corpus", "Damage to Infested", "Slam damage"], "new factions left out");
        let g = grade(&rules["larkspur"], &["WeaponFireIterationsMod", "WeaponStatusDamage"], &["WeaponZoomFovMod"]);
        assert_eq!(g.grade, "Great", "SDMG is Status Damage");
    }

    #[test]
    fn combined_elements_always_wanted_new_factions_never() {
        let t = parse_tab(TAB);
        let torid = &t["torid"];
        // CD + MS + Gas (not on Torid's row), harmless zoom: every positive wanted.
        let g = grade(torid, &["WeaponCritDamageMod", "WeaponFireIterationsMod", "WeaponGasDamageMod"], &["WeaponZoomFovMod"]);
        assert_eq!((g.grade, g.wanted.as_slice()), ("Great", &[true, true, true][..]));
        // Weak point damage counts too.
        let g = grade(torid, &["WeaponCritDamageMod", "WeaponWeakpointDamage"], &["WeaponZoomFovMod"]);
        assert_eq!(g.grade, "Great");
        // Damage to Techrot never does: dead weight, like punch through.
        let g = grade(torid, &["WeaponCritDamageMod", "WeaponFireIterationsMod", "WeaponFactionDamageTechrot"], &["WeaponZoomFovMod"]);
        assert_eq!((g.grade, g.wanted.as_slice()), ("Good", &[true, true, false][..]));
    }

    #[test]
    fn parses_rows_and_combos() {
        let t = parse_tab(TAB);
        assert_eq!(t["torid"].combos, [(vec!["CD".to_string()], ["MS", "CC", "DMG", "FR", "TOX"].map(String::from).to_vec())]);
        assert_eq!(t["kronen"].combos.len(), 2);
        assert_eq!(t["kronen"].harmless, ["EFF", "SLIDE", "FIN"]);
    }

    #[test]
    fn normalises_names() {
        assert_eq!(normalise("Vinquibus (Melee)"), "vinquibus");
        assert_eq!(normalise("Kronen Prime"), "kronen");
        assert_eq!(normalise("Dual Toxocyst"), "dual toxocyst");
    }

    #[test]
    fn grades() {
        let t = parse_tab(TAB);
        let torid = &t["torid"];
        let g = |b: &[&str], c: &[&str]| grade(torid, b, c).grade;
        // CD + MS + DMG, harmless zoom negative.
        assert_eq!(g(&["WeaponCritDamageMod", "WeaponFireIterationsMod", "WeaponDamageAmountMod"], &["WeaponZoomFovMod"]), "Great");
        // Third positive is dead weight (punch through).
        assert_eq!(g(&["WeaponCritDamageMod", "WeaponFireIterationsMod", "WeaponPunctureDepthMod"], &["WeaponZoomFovMod"]), "Good");
        // All wanted but a harmful negative (multishot).
        assert_eq!(g(&["WeaponCritDamageMod", "WeaponDamageAmountMod"], &["WeaponFireIterationsMod"]), "OK");
        // No required CD, harmless negative.
        assert_eq!(g(&["WeaponFireIterationsMod", "WeaponToxinDamageMod"], &[]), "OK");
        // No required CD, harmful negative, but two wanted positives.
        assert_eq!(g(&["WeaponFireIterationsMod", "WeaponCritChanceMod"], &["WeaponFactionDamageInfested"]), "OK");
        // One wanted positive and a harmful negative.
        assert_eq!(g(&["WeaponToxinDamageMod", "WeaponPunctureDepthMod"], &["WeaponCritChanceMod"]), "Bad");
        // Nothing wanted.
        assert_eq!(g(&["WeaponPunctureDepthMod", "WeaponZoomFovMod"], &["WeaponCritChanceMod"]), "Bad");
        // Kronen's second build (CC/CD/IC/DMG pool only).
        let k = grade(&t["kronen"], &["WeaponCritChanceMod", "WeaponCritDamageMod"], &[]);
        assert_eq!((k.grade, k.wanted.as_slice()), ("Great", &[true, true][..]));
    }

    #[test]
    fn csv_handles_quotes() {
        let rows = csv_rows("a,\"b, c\",\"say \"\"hi\"\"\"\r\nd,e,f\n");
        assert_eq!(rows, [vec!["a", "b, c", "say \"hi\""], vec!["d", "e", "f"]]);
    }
}
