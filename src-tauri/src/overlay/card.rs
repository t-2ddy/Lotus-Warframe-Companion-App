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
    /// Stats whose number wasn't read (a card still fading in) or was misread past saving
    /// (four digits before the point): it can't be worked out.
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
            && self.stats.iter().zip(&other.stats).all(|(a, b)| (a.value - b.value).abs() < 0.05 || (a.value.is_nan() && b.value.is_nan()))
    }
}

/// Letters only, lowercase.
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphabetic()).flat_map(char::to_lowercase).collect()
}

/// No riven stat reaches four digits before the decimal point; a number that does is a misread.
const MAX_STAT: f64 = 1000.0;

/// Finds the number in a stat line: a sign (`+`, `-`, `x`) followed by digits, tolerating stray
/// spaces (`+191 .1%`). Returns (value, unit, label text).
fn stat_line(line: &str) -> Option<(f64, Unit, String)> {
    let chars: Vec<char> = line.chars().map(|c| if c == '×' { 'x' } else { c }).collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // A minus right after a letter is a hyphen ("AX-52 Toxi-vexidex"), unless the letter is
        // the lock icon read as a lone "O" ("O-35.6% Zoom"); after a digit it's the lock icon
        // read as "0" ("0-35.6% Zoom"). A plus is always a sign ("0+126.4%").
        let lone_lock = i > 0 && matches!(chars[i - 1], 'O' | 'o') && (i == 1 || chars[i - 2] == ' ');
        let after_word = i > 0 && chars[i - 1].is_alphabetic() && !lone_lock;
        let sign_ok = c == '+'
            || (matches!(c, '-' | '−' | '–') && !after_word)
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
                // "%" read as "0/0" ("+57.70/0" for "+57.7%", "+1 090/0" for "+109%"): the first
                // 0 joined the number. Nothing else on a card has a "/" after a number.
                let percent_as_slash = num.ends_with('0')
                    && num.len() > 1
                    && chars.get(j) == Some(&'/')
                    && chars.get(j + 1).is_some_and(|n| matches!(n, '0' | 'o' | 'O'));
                if percent_as_slash {
                    num.pop();
                    j += 2;
                }
                // Riven stats never reach four digits before the point, and cards show at most
                // one decimal: four or more digits without a point lost it ("+1374%" is 137.4%).
                // With a point it can't be told (see `MAX_STAT`).
                let num = num.trim_end_matches('.');
                let num = if !num.contains('.') && num.len() >= 4 {
                    format!("{}.{}", &num[..num.len() - 1], &num[num.len() - 1..])
                } else {
                    num.to_string()
                };
                let mut value: f64 = num.parse().ok()?;
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

/// The most common item (ties go to the one seen first).
fn vote<T: PartialEq + Clone>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut counts: Vec<(T, usize)> = Vec::new();
    for item in items {
        match counts.iter_mut().find(|(seen, _)| *seen == item) {
            Some((_, n)) => *n += 1,
            None => counts.push((item, 1)),
        }
    }
    let best = counts.iter().map(|(_, n)| *n).max()?;
    counts.into_iter().find(|(_, n)| *n == best).map(|(item, _)| item)
}

/// One card from several reads of it (OCR of differently cleaned-up copies, best first): each
/// field takes what most reads agree on. OCR's slips differ between reads, so the majority is
/// right far more often than any one read. `None` if no read was a card.
pub fn consensus(reads: &[Card]) -> Option<Card> {
    let title_key = vote(reads.iter().map(|c| norm(&c.title)))?;
    // Reads of the agreed title count; when no two reads agree on it (a card fading in garbles
    // the name differently each time), every read does, or the first read would win alone.
    let title_agreed = reads.len() == 1 || reads.iter().filter(|c| norm(&c.title) == title_key).count() > 1;
    let counts = |c: &Card| !title_agreed || norm(&c.title) == title_key;
    let count = vote(reads.iter().filter(|c| counts(c)).map(|c| c.stats.len()))?;
    let agreeing: Vec<&Card> = reads.iter().filter(|c| counts(c) && c.stats.len() == count).collect();
    let stats: Vec<CardStat> = (0..count)
        .map(|i| {
            // Number and unit together, to a tenth (the precision cards show). A number some
            // copies read beats none ("not read" is NaN).
            let read = agreeing.iter().filter(|c| !c.stats[i].value.is_nan()).map(|c| ((c.stats[i].value * 100.0).round() as i64, c.stats[i].unit));
            let (value, unit) = vote(read).map_or_else(
                || (f64::NAN, agreeing.first().map_or(Unit::Plain, |c| c.stats[i].unit)),
                |(v, u)| (v as f64 / 100.0, u),
            );
            // A label hidden by its icon reads as nothing in some copies: any read beats none.
            let label = vote(agreeing.iter().map(|c| c.stats[i].label.clone()).filter(|l| !l.is_empty())).unwrap_or_default();
            CardStat { value, unit, label }
        })
        .collect();
    // The title and stat lines as shown: from the first read with the most agreed numbers, and
    // between those, one whose riven name is whole (hyphenated: "Cell-ampicron", not "npicron").
    let matching = |c: &Card| c.stats.iter().zip(&stats).filter(|(a, b)| (a.value - b.value).abs() < 0.005).count();
    let whole_name = |c: &Card| {
        let last = c.title.rsplit(' ').next().unwrap_or_default();
        c.title.contains(' ') && last.contains('-') && last.chars().all(|ch| ch.is_alphabetic() || ch == '-')
    };
    let rank = |c: &Card| (matching(c), whole_name(c));
    let best = agreeing.iter().copied().reduce(|best, c| if rank(c) > rank(best) { c } else { best })?;
    // As `parse_card` counts them, after the vote: a number one copy read isn't lost.
    let lost_numbers = stats.iter().filter(|s| s.value.is_nan() || s.value.abs() >= MAX_STAT).count();
    Some(Card {
        title: best.title.clone(),
        stats,
        mastery: vote(reads.iter().filter_map(|c| c.mastery)),
        rerolls: vote(reads.iter().filter_map(|c| c.rerolls)),
        stat_lines: best.stat_lines.clone(),
        lost_numbers,
    })
}

/// A number OCR split off its line: the sign and first digits on one line, the rest (".5% Status
/// Chance") on the line before or after. Joined back into one line.
fn join_split_numbers(lines: &[String]) -> Vec<String> {
    let bare = |l: &str| {
        let mut c = l.trim().chars();
        let digits: String = c.clone().skip(1).filter(|c| *c != ' ').collect();
        matches!(c.next(), Some('+' | '-' | '−' | '–')) && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    };
    let rest = |l: &str| l.trim().starts_with('.') && l.chars().any(char::is_alphabetic);
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let (a, b) = (&lines[i], lines.get(i + 1));
        match b {
            Some(b) if bare(a) && rest(b) => {
                out.push(format!("{}{}", a.trim().replace(' ', ""), b.trim()));
                i += 2;
            }
            Some(b) if rest(a) && bare(b) => {
                out.push(format!("{}{}", b.trim().replace(' ', ""), a.trim()));
                i += 2;
            }
            _ => {
                out.push(a.clone());
                i += 1;
            }
        }
    }
    out
}

/// A stat line whose sign OCR lost with its icon ("77.7% *Impact", "59.40/0 Z m"): a percentage
/// at the start of the line.
fn unsigned_stat_line(line: &str) -> Option<(f64, Unit, String)> {
    let line = line.trim();
    if !line.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    stat_line(&format!("+{line}")).filter(|(_, unit, _)| *unit == Unit::Percent)
}

/// Parses OCR lines of one card. `None` unless it has a title and at least two stats.
pub fn parse_card(lines: &[String]) -> Option<Card> {
    let mut title_parts: Vec<String> = Vec::new();
    let mut stats: Vec<CardStat> = Vec::new();
    let mut stat_lines: Vec<String> = Vec::new();
    let (mut mastery, mut rerolls) = (None, None);
    let mut lost_numbers = 0;
    for raw in &join_split_numbers(lines) {
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
                if value.abs() >= MAX_STAT {
                    lost_numbers += 1; // misread past saving: read the card again
                }
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
            // the end of the label above. Kept with no number: it still counts as a stat (the
            // others' numbers depend on how many there are), shown as "not read". The card is
            // read again while it settles.
            lost_numbers += 1;
            stats.push(CardStat { value: f64::NAN, unit: Unit::Times, label: norm(line) });
            stat_lines.push(line.to_string());
        } else if let Some((value, unit, label)) = unsigned_stat_line(line).filter(|_| mastery.is_none() && stats.len() >= 3) {
            // A number without its sign after three stats: the negative (a riven has at most
            // three positives), its "-" lost with the icon.
            stats.push(CardStat { value: -value, unit, label });
            stat_lines.push(format!("-{line}"));
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
    if let Some((tag, _)) = keys.iter().filter(|(_, k)| tail_ok(k)).max_by_key(|(_, k)| k.len()) {
        return Some(tag.clone());
    }
    // Misread letters ("Mulitshot", "Critica1 Damage"): a label a letter away (two for long
    // ones), allowing for icon junk before it, if only one stat is that close.
    let close = |k: &str| {
        let allowed = if k.len() >= 8 { 2 } else { 1 };
        let ocr: Vec<char> = ocr.chars().collect();
        (0..=3.min(ocr.len().saturating_sub(4)))
            .map(|skip| edit_distance(&ocr[skip..], &k.chars().collect::<Vec<_>>()) + skip.min(1))
            .min()
            .filter(|d| *d <= allowed)
    };
    let mut near: Vec<(usize, &String)> = keys.iter().filter_map(|(t, k)| close(k).map(|d| (d, t))).collect();
    near.sort();
    match near.as_slice() {
        [(d, t), rest @ ..] if rest.iter().all(|(d2, t2)| d2 > d || t2 == t) => Some((*t).clone()),
        _ => None,
    }
}

/// Letters to add, drop or change to turn one word into the other.
fn edit_distance(a: &[char], b: &[char]) -> usize {
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.iter().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = (prev + usize::from(ca != cb)).min(row[j] + 1).min(cur + 1);
            prev = cur;
        }
    }
    row[b.len()]
}

/// The tag of a stat another riven type rolls but `kind` doesn't (`zoom` on a shotgun riven →
/// `WeaponZoomFovMod`).
fn legacy_tag(data: &RivenData, kind: &RivenType, label: &str) -> Option<String> {
    let mut others: Vec<(&String, &RivenType)> = data.types.iter().collect();
    others.sort_by_key(|(k, _)| *k);
    others.into_iter().find_map(|(_, other)| {
        match_label(other, label).filter(|t| rivens::legacy_stat(data, kind, t).is_some())
    })
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
    /// As the card shows it: the numbers at the riven's rank (the tests check these).
    #[cfg_attr(not(test), allow(dead_code))]
    pub view: RivenView,
    /// The same rolls with the numbers at max rank (what the overlay shows).
    pub max: RivenView,
    /// The rank wasn't known, so it was worked out from the numbers.
    pub rank_assumed: bool,
    /// Misread numbers that were repaired or left as "not read", for overlay.log.
    pub notes: Vec<String>,
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
/// that; fall back to the logged weapon, then to a name OCR misread by a letter or two.
pub fn weapon_path(data: &RivenData, card: &Card, logged: Option<&str>) -> Option<String> {
    let want = norm(card.weapon_name());
    data.weapons
        .iter()
        .filter(|(_, w)| norm(&w.name) == want)
        .map(|(p, _)| p.clone())
        .min()
        .or_else(|| logged.filter(|p| data.weapons.contains_key(*p)).map(String::from))
        .or_else(|| misread_weapon(data, &want))
}

/// The weapon a misread name is closest to ("Maqistar", "Catabo/yst"): within a letter (two for
/// names of 8+ letters), and only if no other weapon is as close. Short names are left alone.
fn misread_weapon(data: &RivenData, want: &str) -> Option<String> {
    let allowed = match want.chars().count() {
        0..=4 => return None,
        5..=7 => 1,
        _ => 2,
    };
    let want: Vec<char> = want.chars().collect();
    let mut best: Option<(usize, String, String)> = None; // (distance, name, path)
    let mut tie = false;
    for (path, w) in &data.weapons {
        let name = norm(&w.name);
        let d = edit_distance(&want, &name.chars().collect::<Vec<_>>());
        if d > allowed {
            continue;
        }
        match &best {
            Some((bd, bn, bp)) if d == *bd => {
                if name != *bn {
                    tie = true;
                } else if path < bp {
                    best = Some((d, name, path.clone()));
                }
            }
            Some((bd, _, _)) if d > *bd => {}
            _ => {
                best = Some((d, name, path.clone()));
                tie = false;
            }
        }
    }
    best.filter(|_| !tie).map(|(_, _, path)| path)
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
    // Matched at the riven's own rank (the card's numbers), shown at max rank.
    let (r, _) = owned
        .iter()
        .filter_map(|r| rivens::view_with(data, sheet, r, rivens::own_rank(r), disposition).map(|v| (r, v)))
        .filter(|(_, v)| norm(&format!("{} {}", v.weapon, v.name)) == want)
        .min_by(|(_, a), (_, b)| stat_distance(a, card).total_cmp(&stat_distance(b, card)))?;
    rivens::view_with(data, sheet, r, rivens::MAX_RANK, disposition)
}

/// How far a riven's shown numbers are from the card's (sum of differences).
fn stat_distance(v: &RivenView, card: &Card) -> f64 {
    let nums = |text: &str| stat_line(text).map(|(n, _, _)| n);
    v.stats
        .iter()
        .zip(&card.stats)
        .map(|(s, c)| if c.value.is_nan() { 0.0 } else { nums(&s.text).map_or(100.0, |n| (n - c.value).abs()) })
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
    rebuild_at(data, sheet, riven_type, card, logged_weapon, rank, None)
}

/// The disposition the data gives the card's weapon: the logged weapon's, else the base weapon's.
fn listed_disposition(data: &RivenData, card: &Card, logged_weapon: Option<&str>) -> Option<f64> {
    shown_disposition(data, card, logged_weapon)
        .or_else(|| weapon_path(data, card, logged_weapon).and_then(|p| data.weapons.get(&p)).map(|w| w.disposition))
}

/// `rebuild` with the numbers worked out at `disposition` instead of the data's.
fn rebuild_at(
    data: &RivenData,
    sheet: &GradeSheet,
    riven_type: &str,
    card: &Card,
    logged_weapon: Option<&str>,
    rank: Option<u32>,
    disposition: Option<f64>,
) -> Result<Identified, String> {
    let kind = data.types.get(riven_type).ok_or_else(|| format!("unknown riven type {riven_type}"))?;
    let compat = weapon_path(data, card, logged_weapon).ok_or_else(|| format!("unknown weapon \"{}\"", card.weapon_name()))?;
    let disposition = disposition
        .or_else(|| shown_disposition(data, card, logged_weapon))
        .unwrap_or_else(|| data.weapons.get(&compat).map_or(1.0, |w| w.disposition));

    // Tags: from the label; a readable label that matches nothing is a trait the data doesn't
    // have yet (a new splice), shown as read and ungraded so the rest of the riven still works
    // out; a missing label (next to a red icon) from the name's stat parts not already used.
    // A readable label this type doesn't roll and that isn't spliced: a stat DE has since
    // removed from the type (kept as read, "no longer rolls"), under its own tag when another
    // type still rolls it.
    let mut tags: Vec<Option<String>> = card.stats.iter().map(|s| match_label(kind, &s.label)).collect();
    for (i, t) in tags.iter_mut().enumerate() {
        if t.is_some() {
            continue;
        }
        if let Some(tag) = legacy_tag(data, kind, &card.stats[i].label) {
            *t = Some(format!("{}{tag}", rivens::LEGACY_PREFIX));
        } else if card.stats[i].label.len() >= 6 {
            let name = card.stat_lines.get(i).map_or_else(|| card.stats[i].label.clone(), |l| label_text(l));
            *t = Some(format!("{}{name}", rivens::LEGACY_PREFIX));
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
    let junk = &card.stats[i].label;
    if !junk.is_empty() && junk.len() <= 3 && card.stats[i].value < 0.0 {
        // The negative's label as a few letters ("59.40/0 Z m" for "-59.4% Zoom"): a stat whose
        // name has those letters in order, if exactly one such stat fits.
        let mut fitted: Vec<Identified> = Vec::new();
        for (tag, s) in &kind.stats {
            if tags.iter().flatten().any(|u| u == tag) || !in_order(junk, &label_key(&s.label)) {
                continue;
            }
            let mut tried = tags.clone();
            tried[i] = Some(tag.clone());
            if tried.iter().all(Option::is_some) {
                let tried: Vec<String> = tried.into_iter().flatten().collect();
                if let Ok(r) = rebuild_tagged(data, sheet, kind, riven_type, card, compat.clone(), disposition, rank, &tried) {
                    fitted.push(r);
                }
            }
        }
        return match fitted.len() {
            1 => Ok(fitted.remove(0)),
            _ => Err(not_recognised()),
        };
    }
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

/// Every letter of `short` appears in `long`, in order ("zm" in "zoom").
fn in_order(short: &str, long: &str) -> bool {
    let mut rest = long.chars();
    short.chars().all(|c| rest.any(|l| l == c))
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

/// Works a card out as a riven that isn't the user's own: each riven type in turn, then, when the
/// base weapon's numbers don't fit and the game didn't say which variant the card is shown with,
/// the weapon's variants (`owned` ones first: Kompressa 1.15 vs Kompressa Prime 0.6). Returns
/// the riven and the variant's name when a variant's numbers fitted; else why each try failed.
pub fn work_out(
    data: &RivenData,
    sheet: &GradeSheet,
    card: &Card,
    types: &[String],
    weapon: Option<&str>,
    rank: Option<u32>,
    owned: impl Fn(&str) -> bool,
) -> Result<(Identified, Option<String>), Vec<String>> {
    let mut errors = Vec::new();
    let mut repaired = None;
    for kind in types {
        match rebuild(data, sheet, kind, card, weapon, rank) {
            Ok(r) if r.notes.is_empty() => return Ok((r, None)),
            // Worked out only by repairing or dropping a number: a variant whose numbers fit as
            // read is more likely right (a wrong disposition throws every number off).
            Ok(r) => {
                repaired.get_or_insert((r, None));
            }
            Err(why) => errors.push(format!("{kind}: {why}")),
        }
    }
    if types.is_empty() {
        errors.push("riven type unknown".into());
    }
    if variant_unknown(data, card, weapon) {
        let base = weapon_path(data, card, None).unwrap_or_default();
        let mut variants: Vec<&str> = data.family(&base).into_iter().map(|(p, _)| p).filter(|p| *p != base).collect();
        variants.sort_by_key(|p| !owned(p));
        for v in variants {
            for kind in types {
                if let Ok(r) = rebuild(data, sheet, kind, card, Some(v), rank) {
                    let name = data.weapons.get(v).map(|w| w.name.clone());
                    if r.notes.is_empty() {
                        return Ok((r, name));
                    }
                    repaired.get_or_insert((r, name));
                }
            }
        }
    }
    // The data's disposition may be behind a change: the wiki and the game's files have each
    // lagged one (Haalvu's export, Coda Bassocyst's wiki page, October 2026). A disposition a
    // step or two away (DE moves them by 0.05–0.1) where every number fits as read beats
    // repairing or dropping one. Only one side can fit when the listed value doesn't.
    if let Some(listed) = listed_disposition(data, card, weapon) {
        for step in [0.05, -0.05, 0.1, -0.1] {
            let d = ((listed + step) * 100.0).round() / 100.0;
            for kind in types {
                if let Ok(mut r) = rebuild_at(data, sheet, kind, card, weapon, rank, Some(d)) {
                    if r.notes.is_empty() {
                        r.notes.push(format!("numbers fit disposition {d}, not the data's {listed}"));
                        return Ok((r, None));
                    }
                }
            }
        }
    }
    repaired.ok_or(errors)
}

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
    // known numbers, and stats whose number couldn't be read, still count towards the buff/curse
    // totals but aren't worked out.
    let mut lines = Vec::new();
    let mut pending = Vec::new(); // (card position, tag, is_buff)
    let mut unread: Vec<Unread> = Vec::new();
    let mut buff_at = Vec::new(); // is_buff for each card stat, in card order
    let named = name_tags(kind, card.riven_name());
    for (i, (stat, tag)) in card.stats.iter().zip(tags).enumerate() {
        let Some(info) = rivens::stat_info(kind, tag) else {
            let is_buff = if stat.unit == Unit::Times { stat.value >= 1.0 } else { stat.value >= 0.0 };
            pending.push((i, tag.clone(), is_buff));
            buff_at.push(is_buff);
            continue;
        };
        let raw = raw_value(stat, &info.label);
        // A curse goes against the stat's usual direction; a negative-only stat is always one.
        // Without a number, the riven's name says (it lists every positive).
        let is_buff = if stat.value.is_nan() {
            named.contains(tag)
        } else {
            !rivens::curse_only(&info) && raw.signum() == info.value.signum()
        };
        buff_at.push(is_buff);
        if damaged(stat, tag, &info.label) {
            unread.push(Unread { at: Some(i), tag: tag.clone(), is_buff, info });
            continue;
        }
        let base = if is_buff { info.value } else { rivens::curse_base(&info) };
        lines.push(Line { tag: tag.clone(), raw, buff: is_buff, base, step: display_step(stat, &info.label), at: i, info });
    }
    // The riven's name lists every positive: one the card read missed (an icon can hide a
    // whole line) is added from it.
    let positives = lines.iter().filter(|l| l.buff).count() + pending.iter().filter(|p| p.2).count() + unread.iter().filter(|u| u.is_buff).count();
    if named.len() > positives {
        let missing: Vec<&String> = named.iter().filter(|t| !tags.contains(t)).collect();
        if missing.len() >= named.len() - positives {
            for tag in missing.into_iter().take(named.len() - positives) {
                if let Some(info) = rivens::stat_info(kind, tag) {
                    unread.push(Unread { at: None, tag: tag.clone(), is_buff: true, info });
                }
            }
        }
    }
    let nb = lines.iter().filter(|l| l.buff).count() + pending.iter().filter(|p| p.2).count() + unread.iter().filter(|u| u.is_buff).count();
    let nc = lines.len() + pending.len() + unread.len() - nb;
    if nb < 2 || nc > 1 {
        return Err(format!("{nb} positives and {nc} negatives"));
    }
    const BUFF: [f64; 6] = [0.0, 1.0, 0.66000003, 0.5, 0.40000001, 0.34999999];
    const CURSE_BUFF: [f64; 6] = [0.0, 1.0, 0.33000001, 0.5, 1.25, 1.5];
    let atten = 1.5 * disposition * 10.0;
    // A stat's full value (at the middle of its 90–110% range) at rank `r`.
    let full = |base: f64, buff: bool, r: u32| {
        let level = f64::from(r + 1);
        if buff {
            base * atten * 1.25f64.powi(nc as i32) * BUFF[nb.min(5)] * level
        } else {
            -base * atten * CURSE_BUFF[nb.min(5)] * BUFF[nc.min(5)] * level
        }
    };
    // Each stat's roll (0 = 90% of its full value, 1 = 110%), and how far that can be off
    // because the card rounds the number (x0.98 on a rank 0 riven is anywhere in ±0.01).
    let rolls = |lines: &[Line], r: u32| -> Vec<(f64, f64)> {
        lines
            .iter()
            .map(|l| {
                let full = full(l.base, l.buff, r);
                ((l.raw / full - 0.9) / 0.2, l.step / full.abs() / 0.2)
            })
            .collect()
    };
    let fits = |f: &[(f64, f64)]| f.iter().all(|(x, off)| (-0.12 - off..=1.12 + off).contains(x));
    let rank_for = |lines: &[Line]| match rank {
        Some(r) => fits(&rolls(lines, r)).then_some(r),
        None => (0..=rivens::MAX_RANK).rev().find(|r| fits(&rolls(lines, *r))),
    };
    let mut notes = Vec::new();
    if rank_for(&lines).is_none() {
        // One number OCR misread: try the usual slips on each, and keep a repair only if it's
        // the single one that makes every number fit.
        let mut fixes: Vec<(usize, f64, f64)> = Vec::new(); // (line, shown, raw)
        for (k, l) in lines.iter().enumerate() {
            let stat = &card.stats[l.at];
            if stat.unit == Unit::Times {
                continue;
            }
            let per_shown = l.raw / stat.value;
            for shown in repairs(stat.value) {
                let mut tried = lines.clone();
                tried[k].raw = shown * per_shown;
                if rank_for(&tried).is_some() && !fixes.iter().any(|f| f.0 == k && (f.1 - shown).abs() < 0.05) {
                    fixes.push((k, shown, shown * per_shown));
                }
            }
        }
        if let [(k, shown, raw)] = fixes[..] {
            notes.push(format!("read {} as {shown}", card.stat_lines.get(lines[k].at).map_or("?", String::as_str)));
            lines[k].raw = raw;
        } else {
            // No single repair: if exactly one number keeps the rest from fitting, it's shown
            // as "not read" (it still counts as a stat) and the rest is worked out.
            let lone: Vec<usize> = (0..lines.len())
                .filter(|k| {
                    let mut rest = lines.clone();
                    rest.remove(*k);
                    !rest.is_empty() && rank_for(&rest).is_some()
                })
                .collect();
            if let [k] = lone[..] {
                let l = lines.remove(k);
                notes.push(format!("couldn't read {} (no repair fits)", card.stat_lines.get(l.at).map_or("?", String::as_str)));
                unread.push(Unread { at: Some(l.at), tag: l.tag, is_buff: l.buff, info: l.info });
            }
        }
    }
    let (rank, rank_assumed) = match rank {
        // A known rank (rerolls keep it) must fit too: otherwise the rolls would be clamped to
        // their range and shown as numbers the card doesn't have.
        Some(r) if rank_for(&lines).is_none() => {
            let at_r: Vec<f64> = rolls(&lines, r).into_iter().map(|(x, _)| x).collect();
            return Err(format!("numbers don't fit rank {r} (rolls: {at_r:?})"));
        }
        Some(r) => (r, false),
        None => {
            let at_8: Vec<f64> = rolls(&lines, 8).into_iter().map(|(x, _)| x).collect();
            let r = rank_for(&lines).ok_or_else(|| format!("numbers don't fit any rank (rolls at rank 8: {at_8:?})"))?;
            (r, true)
        }
    };
    let f: Vec<f64> = rolls(&lines, rank).into_iter().map(|(x, _)| x).collect();
    let roll = |x: f64| (x.clamp(0.0, 1.0) * f64::from(0x3FFF_FFFFu32)).round() as u64;
    let mut buffs: Vec<_> = lines.iter().zip(&f).filter(|(l, _)| l.buff).map(|(l, x)| json!({"Tag": l.tag, "Value": roll(*x)})).collect();
    let mut curses: Vec<_> = lines.iter().zip(&f).filter(|(l, _)| !l.buff).map(|(l, x)| json!({"Tag": l.tag, "Value": roll(*x)})).collect();
    for (_, tag, is_buff) in &pending {
        let entry = json!({"Tag": tag, "Value": 0});
        if *is_buff { buffs.push(entry) } else { curses.push(entry) }
    }
    for u in &unread {
        let entry = json!({"Tag": format!("{}{}", rivens::UNREAD_PREFIX, u.tag), "Value": 0});
        if u.is_buff { buffs.push(entry) } else { curses.push(entry) }
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
    // The riven with its numbers at rank `show`: the card's rank, and max rank for the overlay.
    let build = |show: u32| -> Result<RivenView, String> {
        let mut view = rivens::view_with(data, sheet, &owned, show, Some(disposition)).ok_or("couldn't build the riven")?;
        view.name = card.riven_name().to_string(); // the game's name, as shown
        if !pending.is_empty() || !unread.is_empty() {
            // An unread stat's range at this rank, from its worst to its best roll.
            let unread: Vec<(Option<usize>, rivens::StatView)> = unread
                .iter()
                .map(|u| {
                    let base = if u.is_buff { u.info.value } else { rivens::curse_base(&u.info) };
                    let at = |x: f64| rivens::stat_text(&u.tag, &u.info.label, full(base, u.is_buff, show) * (0.9 + 0.2 * x));
                    let (worst, best) = if u.is_buff { (at(0.0), at(1.0)) } else { (at(1.0), at(0.0)) };
                    let view = rivens::StatView {
                        tag: u.tag.clone(),
                        text: rivens::label_name(&u.info.label),
                        good: u.is_buff,
                        quality: 0.0,
                        letter: "?".into(),
                        worst,
                        best,
                        wanted: None,
                        pending: true,
                        legacy: false,
                        unread: true,
                    };
                    (u.at, view)
                })
                .collect();
            add_pending(data, kind, &mut view, card, &pending, unread, &buff_at);
            // Graded again with these back in place (the sheet judges a negative Zoom or a missed
            // Toxin by its tag); spliced traits still leave it ungraded.
            if !view.grade_pending {
                rivens::apply_grade(&mut view, sheet);
            }
        }
        Ok(view)
    };
    Ok(Identified { view: build(rank)?, max: build(rivens::MAX_RANK)?, rank_assumed, notes })
}

/// A card stat worked out from its number: `raw` as the riven data counts it (0.995 for
/// "+99.5%"), `base` the data's value for it as a buff or curse, `step` how far the card's
/// rounding can move it, `at` its place on the card.
#[derive(Clone)]
struct Line {
    tag: String,
    raw: f64,
    buff: bool,
    base: f64,
    step: f64,
    at: usize,
    info: rivens::StatInfo,
}

/// What a misread number may have been: a lost or extra decimal place ("+1 1 .7%" for 117%), a
/// lost or extra leading 1, or a 7 read for a 1 or the other way round ("+745.7%" for 145.7%).
fn repairs(shown: f64) -> Vec<f64> {
    let sign = if shown < 0.0 { -1.0 } else { 1.0 };
    let text = format!("{:.1}", shown.abs());
    let text = text.strip_suffix(".0").unwrap_or(&text).to_string();
    let mut out = vec![shown.abs() * 10.0, shown.abs() / 10.0];
    let mut texts = vec![format!("1{text}")];
    if let Some(rest) = text.strip_prefix('1').filter(|r| !r.is_empty() && !r.starts_with('.')) {
        texts.push(rest.to_string());
    }
    for (i, c) in text.char_indices() {
        let swap = match c {
            '7' => '1',
            '1' => '7',
            _ => continue,
        };
        texts.push(format!("{}{swap}{}", &text[..i], &text[i + 1..]));
    }
    out.extend(texts.iter().filter_map(|t| t.parse::<f64>().ok()));
    out.into_iter().filter(|v| *v > 0.0 && *v < MAX_STAT).map(|v| sign * (v * 10.0).round() / 10.0).collect()
}

/// A stat whose number couldn't be read: `at` its place on the card, or `None` if the card read
/// missed the whole line.
struct Unread {
    at: Option<usize>,
    tag: String,
    is_buff: bool,
    info: rivens::StatInfo,
}

/// A number OCR damaged past using: not read at all (a faction line without its "x0.98"), four
/// digits before the point, a percentage read without its "%" ("+1 f Electricity" for
/// "+1xx.x% Electricity", the digits lost with the icon), or a faction multiplier without its
/// "x".
fn damaged(stat: &CardStat, tag: &str, label: &str) -> bool {
    if stat.value.is_nan() || stat.value.abs() >= MAX_STAT {
        return true;
    }
    if tag.contains("FactionDamage") {
        return stat.unit != Unit::Times;
    }
    label.contains("|val|%") && stat.unit != Unit::Percent
}

/// Puts the stats that aren't worked out back in their places on the card: spliced and removed
/// traits with the card's numbers, unread ones as "not read" (a missed positive goes after the
/// other positives).
fn add_pending(
    data: &RivenData,
    kind: &RivenType,
    view: &mut RivenView,
    card: &Card,
    pending: &[(usize, String, bool)],
    mut unread: Vec<(Option<usize>, rivens::StatView)>,
    buff_at: &[bool],
) {
    let (mut buffs, mut curses): (Vec<_>, Vec<_>) = std::mem::take(&mut view.stats).into_iter().partition(|s| s.good);
    buffs.reverse();
    curses.reverse();
    for (i, stat) in card.stats.iter().enumerate() {
        if let Some((_, tag, is_buff)) = pending.iter().find(|p| p.0 == i) {
            // A removed stat goes back under its own tag (for the grading sheet) and name.
            let legacy = tag.strip_prefix(rivens::LEGACY_PREFIX);
            let known = legacy.and_then(|t| rivens::legacy_stat(data, kind, t));
            let shown_tag = if known.is_some() { legacy.unwrap_or(tag).to_string() } else { tag.clone() };
            let name = match &known {
                Some(s) => rivens::label_name(&s.label),
                None => tag.trim_start_matches(rivens::PENDING_PREFIX).trim_start_matches(rivens::LEGACY_PREFIX).to_string(),
            };
            let number = match stat.unit {
                Unit::Times => format!("x{:.2}", stat.value),
                Unit::Percent => format!("{:+}%", stat.value),
                Unit::Plain => format!("{:+}", stat.value),
            };
            view.stats.push(rivens::StatView {
                tag: shown_tag,
                text: format!("{number} {name}"),
                good: *is_buff,
                quality: 0.0,
                letter: "?".into(),
                worst: String::new(),
                best: String::new(),
                wanted: None,
                pending: true,
                legacy: legacy.is_some(),
                unread: false,
            });
        } else if let Some(pos) = unread.iter().position(|(at, _)| *at == Some(i)) {
            view.stats.push(unread.remove(pos).1);
        } else if let Some(s) = if buff_at[i] { buffs.pop() } else { curses.pop() } {
            view.stats.push(s);
        }
    }
    view.stats.extend(buffs.into_iter().rev().chain(curses.into_iter().rev()));
    for (_, missed) in unread {
        let first_curse = view.stats.iter().position(|s| !s.good).unwrap_or(view.stats.len());
        view.stats.insert(if missed.good { first_curse } else { view.stats.len() }, missed);
    }
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
    fn misread_weapon_names_find_the_weapon() {
        // riven_misfits.jsonl: "Maqistar" and "Catabo/yst" ended as "unknown weapon".
        let d = rivens::data();
        let d = &*d;
        let path = |title: &str| weapon_path(d, &card_titled(title), None).and_then(|p| d.weapons.get(&p).map(|w| w.name.clone()));
        assert_eq!(path("Maqistar Acri-forticron").as_deref(), Some("Magistar"));
        assert_eq!(path("Catabo/yst Deci-critaada").as_deref(), Some("Catabolyst"));
        // Too short to guess, or nothing close.
        assert_eq!(path("Boaf Acri-satidex"), None);
        assert_eq!(path("Zzzzzzzz Acri-satidex"), None);
    }

    fn card_titled(title: &str) -> Card {
        Card { title: title.into(), ..Card::empty() }
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
    fn a_nearby_disposition_beats_dropping_a_number() {
        // overlay.log 2026-10-13: the wiki has Coda Bassocyst at 0.55, but the game's files (and
        // the card's numbers) say 0.6. At 0.55, Viral alone was out of range and shown "not read".
        let d = rivens::data();
        let d = &*d;
        let c = card("Coda Bassocyst Cari- / critapha / +47% 6 Heat / +55.2% \"Viral / +51.8% Critical Chance / -20.2% Zoom / MR 13 / 07");
        let types = ["LotusShotgunRandomModRare".to_string()];
        let (r, _) = work_out(d, &GradeSheet::default(), &c, &types, None, None, |_| false).unwrap();
        assert_eq!((r.view.rank, r.view.disposition), (8, 0.6));
        assert!(r.view.stats.iter().all(|s| !s.unread));
        assert_eq!(r.notes, ["numbers fit disposition 0.6, not the data's 0.55"]);
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
    fn misread_labels_match_the_closest_stat() {
        let d = rivens::data();
        let d = &*d;
        let rifle = &d.types["LotusRifleRandomModRare"];
        let tag = |ocr: &str| match_label(rifle, ocr);
        assert_eq!(tag("mulitshot").as_deref(), Some("WeaponFireIterationsMod"));
        assert_eq!(tag("multisnot").as_deref(), Some("WeaponFireIterationsMod"));
        assert_eq!(tag("criticadamage").as_deref(), Some("WeaponCritDamageMod"), "\"Critica1 Damage\"");
        assert_eq!(tag("criticaldarnage").as_deref(), Some("WeaponCritDamageMod"), "rn for m");
        assert_eq!(tag("vmultishot").as_deref(), Some("WeaponFireIterationsMod"), "icon junk");
        // Real traits stay themselves, and something new isn't forced onto a stat.
        assert_eq!(tag("statusdamage").as_deref(), Some("Pending:Status Damage"));
        assert_eq!(tag("statuschance").as_deref(), Some("WeaponStunChanceMod"));
        assert_eq!(tag("brandnewthing"), None);
    }

    #[test]
    fn several_reads_vote() {
        // reroll (1).png read four ways (2026-10-10): each slip is in a different read.
        let reads: Vec<Card> = [
            "Burston Crita-lexitis / +1 91 .1% Critical Chance / +3.4 Punch Through / +137.4% Critical Damage / O -132.2% VPuncture / MR 15 / 094",
            "Burston Crita-lexitis / +191 Critical Chance / +34 Punch Through / +137.4% Critical Damage / O -132.2% Y Puncture / MR 15",
            "Burston Crita-lexitis / +191.10/0 Critical Chance / +3.4 punch Through / +1374% Critical Damage / O -132.2% MPuncture / MR 15 / 094",
            "Burston Crita-lexitis / +191.1% Critical Chance / +3.4 Punch Through / +137.4% Critical Damage / 0-132.2% YPuncture , / MR13",
        ]
        .iter()
        .filter_map(|t| parse_card(&t.split(" / ").map(String::from).collect::<Vec<_>>()))
        .collect();
        let c = consensus(&reads).unwrap();
        let values: Vec<(f64, Unit)> = c.stats.iter().map(|s| (s.value, s.unit)).collect();
        assert_eq!(values, [(191.1, Unit::Percent), (3.4, Unit::Plain), (137.4, Unit::Percent), (-132.2, Unit::Percent)]);
        assert_eq!((c.title.as_str(), c.mastery, c.rerolls), ("Burston Crita-lexitis", Some(15), Some(94)));
        // A label only some reads saw (the Slash icon hides it in others) is kept.
        let slash: Vec<Card> = ["Burston Acri-hexasus / +143.2% / +159.2% Critical Damage", "Burston Acri-hexasus / +143.2% qSlash / +159.2% Critical Damage"]
            .iter()
            .filter_map(|t| parse_card(&t.split(" / ").map(String::from).collect::<Vec<_>>()))
            .collect();
        assert_eq!(consensus(&slash).unwrap().stats[0].label, "qslash");
        assert!(consensus(&[]).is_none());
    }

    #[test]
    fn removed_stats_are_shown_as_no_longer_rolling() {
        // riven_misfits.jsonl 2026-10-10: shotgun rivens can't roll Zoom any more, but this
        // Sobek riven has it ("stat not recognised: zoom").
        let d = rivens::data();
        let d = &*d;
        let c = card("Sobek Acri-satitox / +6.9% z Toxin / +7.8% Critical Damage / +9.6% Multishot / -2.6% Zoom / MR 16 / 056 \\");
        let kuva = Some("/Lotus/Weapons/Grineer/KuvaLich/LongGuns/Sobek/KuvaSobek");
        let r = rebuild(d, &GradeSheet::default(), "LotusShotgunRandomModRare", &c, kuva, None).unwrap();
        let zoom = &r.view.stats[3];
        assert_eq!((zoom.tag.as_str(), zoom.text.as_str(), zoom.good), ("WeaponZoomFovMod", "-2.6% Zoom", false));
        assert!(zoom.legacy && zoom.pending && zoom.letter == "?");
        assert!(r.view.stats[..3].iter().all(|s| !s.pending && s.letter != "?"), "{:?}", r.view.stats);
        assert!(!r.view.grade_pending, "a removed stat doesn't stop the grade");
        // A label no type has (and not a known splice) is kept the same way.
        let odd = card("Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage / +60.2% Brand New Thing / -90% Projectile Speed");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &odd, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
        assert!(r.view.stats[2].legacy && r.view.stats[2].text == "+60.2% Brand New Thing");
        // Known splices are still "collecting data", not removed.
        let melee = &d.types["PlayerMeleeWeaponRandomModRare"];
        assert_eq!(match_label(melee, "slamattackdamage").as_deref(), Some("Pending:Slam Attack Damage"));
    }

    #[test]
    fn negative_only_stats_are_curses() {
        // riven_misfits.jsonl 2026-10-10: "Chance to Gain Combo Count" is stored with a negative
        // value, so "-12.4%" matched its sign and was counted as a positive (4 positives, no
        // negative), and nothing fit.
        let d = rivens::data();
        let d = &*d;
        let c = card("Magistar Laci-acrium / +1 1.5% Critical Damage / +17.8% Slam Attack / Damage / +3.1 Initial Combo / -12.4% Chance to Gain / Combo Count / MR 15 / 073");
        assert_eq!(c.stats[0].value, 11.5);
        let magistar = Some("/Lotus/Weapons/Tenno/Melee/Maces/PaladinMace/PaladinMaceWeapon");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &c, magistar, None).unwrap();
        let combo = r.view.stats.iter().find(|s| s.tag == "WeaponMeleeComboPointsOnHitMod").unwrap();
        assert!(!combo.good, "it's the curse");
        assert_eq!(combo.text, "-12.4% Chance to Gain Combo Count", "shown negative, as on the card");
        assert_eq!(r.view.rank, 0);
    }

    #[test]
    fn whole_percentages_read_with_a_slash() {
        // riven_misfits.jsonl 2026-10-10: "+109%" read as "+1 090/0" became a plain 1090.
        let d = rivens::data();
        let d = &*d;
        let c = card("Torid Crita-zetitox / +198.7% Critical Chance / +1 090/0 z Toxin / -111.7% Weapon Recoil / -47.6% Magazine / Capacity / MR 12 / 0201 \\");
        assert_eq!(c.stats[1], CardStat { value: 109.0, unit: Unit::Percent, label: "ztoxin".into() });
        let torid = Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &c, torid, None).unwrap();
        let recoil = r.view.stats.iter().find(|s| s.tag == "WeaponRecoilReductionMod").unwrap();
        assert!(recoil.good, "less recoil is a positive");
        let magazine = r.view.stats.iter().find(|s| s.tag == "WeaponClipMaxMod").unwrap();
        assert!(!magazine.good, "the one negative");
        assert_eq!(r.view.rank, 8);
    }

    #[test]
    fn stats_never_reach_four_digits() {
        // A lost decimal point: four digits without one is the last digit after the point.
        let c = card("Burston Crita-lexitis / +191.1% Critical Chance / +1374% Critical Damage / MR 15");
        assert_eq!((c.stats[1].value, c.stats[1].unit, c.lost_numbers), (137.4, Unit::Percent, 0));
        assert_eq!(card("Torid Crita-zetitox / +198.7% Critical Chance / +1 090/0 z Toxin").stats[1].value, 109.0);
        // Four digits before a point can't be fixed: read the card again rather than grade it.
        let bad = card("Burston Crita-lexitis / +1191.1% Critical Chance / +137.4% Critical Damage / MR 15");
        assert_eq!(bad.lost_numbers, 1);
        // Three digits are fine.
        assert_eq!(card("Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage").lost_numbers, 0);
    }

    #[test]
    fn weapons_newer_than_the_export() {
        // riven_misfits.jsonl 2026-10-10: Corufell Prime (0.5) isn't in the export, so the card
        // was worked out at Corufell's 1.05 and nothing fit. It comes from the wiki now.
        let d = rivens::data();
        let d = &*d;
        let c = card("Corufell Igni-acrinus / +34.8% Critical Damage / +36.6% 6Heat / +20.8% Additional Combo / Count Chance / MR 15 / 07");
        let prime = "/Lotus/Weapons/Tenno/Archwing/Melee/PrimeCorufell/PrimeCorufellScytheWeapon";
        assert_eq!(d.weapons[prime].disposition, 0.5);
        assert!(variant_unknown(d, &c, None), "Corufell and Corufell Prime differ");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &c, Some(prime), None).unwrap();
        assert_eq!((r.view.rank, r.view.disposition), (8, 0.5));
        assert!(r.view.stats.iter().all(|s| s.letter != "?"));
    }

    #[test]
    fn a_positive_the_card_read_missed_comes_from_the_name() {
        // overlay.log 2026-10-10: "Laci-toxicron" names three positives, but no read had the
        // Toxin line, so the card was worked out as 2 positives and nothing fit.
        let d = rivens::data();
        let d = &*d;
        let c = card("Hate Laci-toxicron / +7.3% Additional Combo / Count Chance / +19.5% Critical Chance / (x2 for Heavy Attacks) / -10.3% Critical Chance / for Slide Attack / MR 12 / 029 \\");
        let hate = Some("/Lotus/Weapons/Tenno/Melee/Scythe/StalkerScytheWeapon");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &c, hate, None).unwrap();
        assert_eq!(r.view.rank, 0);
        let order: Vec<(&str, bool)> = r.view.stats.iter().map(|s| (s.tag.as_str(), s.unread)).collect();
        assert_eq!(order, [
            ("WeaponMeleeComboBonusOnHitMod", false),
            ("WeaponCritChanceMod", false),
            ("WeaponToxinDamageMod", true),
            ("SlideAttackCritChanceMod", false),
        ], "the missed positive goes after the others, before the curse");
        let toxin = &r.view.stats[2];
        assert_eq!((toxin.text.as_str(), toxin.good, toxin.letter.as_str()), ("Toxin", true, "?"));
        assert!(toxin.worst.ends_with("% Toxin") && toxin.best.ends_with("% Toxin"), "{} {}", toxin.worst, toxin.best);
        assert!(r.view.stats.iter().filter(|s| !s.unread).all(|s| s.letter != "?"));
        // A whole card: nothing added.
        let full = card("Torid Crita-acriata / +198.6% Critical Chance / +152.1% Critical Damage / +188.2% Damage / -90% Projectile Speed / MR 13 / 013");
        let r = rebuild(d, &GradeSheet::default(), "LotusRifleRandomModRare", &full, Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon"), None).unwrap();
        assert!(r.view.stats.iter().all(|s| !s.unread) && r.view.stats.len() == 4);
    }

    #[test]
    fn a_damaged_number_is_not_read() {
        // riven_misfits.jsonl 2026-10-10: the Electricity icon took the digits and the "%" with
        // it ("+1 f Electricity" for "+1xx.x%"), and a plain 1 fit nothing.
        let d = rivens::data();
        let d = &*d;
        let c = card("Nami Solo Locti- / visitio / +2.7 Range / +1 f Electricity / +206% Melee Damage / -114.9% Chance to Gain / Combo Count / MR8 / 0119 \\");
        let nami = Some("/Lotus/Weapons/Tenno/Melee/Swords/CutlassAndPoignard/TennoCutlass");
        let r = rebuild(d, &GradeSheet::default(), "PlayerMeleeWeaponRandomModRare", &c, nami, None).unwrap();
        assert_eq!(r.view.rank, 8);
        let elec = &r.view.stats[1];
        assert_eq!((elec.tag.as_str(), elec.text.as_str(), elec.unread), ("WeaponElectricityDamageMod", "Electricity", true));
        // The range it has to be in at rank 8 on Nami Solo (disposition 1.43).
        assert_eq!((elec.worst.as_str(), elec.best.as_str()), ("+108.6% Electricity", "+132.7% Electricity"));
        assert_eq!(r.view.stats.iter().filter(|s| s.letter != "?").count(), 3);
        assert!(!r.view.stats[3].good, "the curse is still the curse");
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

/// OCR slips around icons and line breaks (riven_misfits.jsonl, 2026-10-09 to 10-12; the riven
/// card regression set has these and more).
#[cfg(test)]
mod ocr_slip_tests {
    use super::*;

    fn card(s: &str) -> Card {
        parse_card(&s.split(" / ").map(String::from).collect::<Vec<_>>()).unwrap()
    }
    const TORID: Option<&str> = Some("/Lotus/Weapons/ClanTech/Bio/BioWeapon");
    const RIFLE: &str = "LotusRifleRandomModRare";

    fn texts(r: &Identified) -> Vec<(&str, bool)> {
        r.view.stats.iter().map(|s| (s.text.as_str(), s.unread)).collect()
    }

    #[test]
    fn a_number_split_over_two_lines_is_joined() {
        // The Slash icon pushed "+1" below ".5% Status Chance": the line was dropped, so the
        // card had a stat too few. Joined, it's a stat whose number fits nothing: "not read".
        let c = card("Burston Eni-scidex / +163.10/0 (v Slash / .5% Status Chance / +1 / +313.2% Weak Point / Damage / -115% YPuncture / / MR9 / 075");
        assert_eq!(c.stat_lines[1], "+1.5% Status Chance");
        assert_eq!(c.stats.len(), 4);
        let d = rivens::data();
        let r = rebuild(&d, &GradeSheet::default(), RIFLE, &c, Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston"), None).unwrap();
        assert_eq!(r.view.rank, 8);
        assert_eq!(texts(&r)[0], ("+163.1% Slash", false));
        assert!(r.view.stats[1].unread && r.view.stats[1].tag == "WeaponStunChanceMod");
        assert_eq!(r.notes.len(), 1, "{:?}", r.notes);
    }

    #[test]
    fn a_faction_line_without_its_number_still_counts() {
        // "Damage to Corpus" read without its "x0.xx": leaving it out changed the other numbers'
        // multiplier, so nothing fit.
        let c = card("Vectis Hexa-critatis / +142% Critical Chance / +90.2% Status Chance / +105.3% Critical Damage / Damage to Corpus / MR 12 / 052 \\");
        assert_eq!((c.stats.len(), c.lost_numbers), (4, 1));
        let d = rivens::data();
        let r = rebuild(&d, &GradeSheet::default(), RIFLE, &c, Some("/Lotus/Weapons/Tenno/LongGuns/PrimeVectis/PrimeVectisRifle"), None).unwrap();
        let corpus = &r.view.stats[3];
        assert!(corpus.unread && !corpus.good && corpus.tag == "WeaponFactionDamageCorpus", "{:?}", texts(&r));
        assert_eq!(r.view.stats.iter().filter(|s| !s.unread).count(), 3);
    }

    #[test]
    fn a_read_number_beats_a_lost_one_in_the_vote() {
        let lost = card("Vectis Hexa-critatis / +142% Critical Chance / +90.2% Status Chance / +105.3% Critical Damage / Damage to Corpus");
        let read = card("Vectis Hexa-critatis / +142% Critical Chance / +90.2% Status Chance / +105.3% Critical Damage / x0.55 Damage to Corpus");
        let c = consensus(&[lost.clone(), lost, read]).unwrap();
        assert_eq!((c.stats[3].value, c.lost_numbers), (0.55, 0));
    }

    #[test]
    fn a_negative_that_lost_its_sign_and_label() {
        // "-59.4% Zoom" read as "59.40/0 Z m": the only stat whose name has a z then an m, and
        // whose numbers fit.
        let c = card("Torid Sati-caricron / +178.2% Critical Chance / +117.8% \"Viral / +120.6% Multishot / 59.40/0 Z m / MR 10 / 033");
        assert_eq!((c.stats[3].value, c.stat_lines[3].as_str()), (-59.4, "-59.40/0 Z m"));
        let d = rivens::data();
        let r = rebuild(&d, &GradeSheet::default(), RIFLE, &c, TORID, None).unwrap();
        assert_eq!(r.view.stats[3].tag, "WeaponZoomFovMod");
        assert_eq!(texts(&r)[3], ("-59.4% Zoom", false));
    }

    #[test]
    fn a_misread_digit_is_repaired_when_only_one_repair_fits() {
        // The user's Torid Crita-acriata with its Critical Damage's 1 read as a 7.
        let c = card("Torid Crita-acriata / +198.6% Critical Chance / +752.1% Critical Damage / +188.2% Damage / -90% Projectile Speed / MR 13 / 013");
        let d = rivens::data();
        let r = rebuild(&d, &GradeSheet::default(), RIFLE, &c, TORID, None).unwrap();
        assert_eq!(texts(&r)[1], ("+152.1% Critical Damage", false));
        assert_eq!(r.view.rank, 8);
        assert_eq!(r.notes, ["read +752.1% Critical Damage as 152.1"]);
    }

    #[test]
    fn a_number_with_several_possible_repairs_is_not_read() {
        // "+1 1 .70/0 Z Toxin": 117% and 111.7% both fit, so neither is guessed.
        let c = card("Torid Sati-caritox / +119.9% Multishot / +114.8% ? Viral / +1 1 .70/0 Z Toxin / -53.8% Zoom / MR 11 / 0 223");
        let d = rivens::data();
        let r = rebuild(&d, &GradeSheet::default(), RIFLE, &c, TORID, None).unwrap();
        let toxin = &r.view.stats[2];
        assert!(toxin.unread && toxin.tag == "WeaponToxinDamageMod", "{:?}", texts(&r));
        assert_eq!(r.view.stats.iter().filter(|s| !s.unread).count(), 3);
    }

    #[test]
    fn repairs_cover_the_usual_slips() {
        let r = repairs(745.7);
        assert!(r.contains(&145.7) && r.contains(&74.6) && !r.contains(&745.7), "{r:?}");
        let r = repairs(11.7);
        assert!(r.contains(&117.0) && r.contains(&111.7), "{r:?}");
        assert!(repairs(-52.1).iter().all(|v| *v < 0.0));
    }
}

#[cfg(test)]
mod reroll_rank {
    use super::*;

    /// overlay.log 2026-10-14: a new roll read while fading in ("-36 4% Zoom" and worse), at the
    /// rerolled riven's known rank 8. It came out at disposition 1.45 with clamped rolls (+266.5%
    /// Damage, -47.4% Zoom); the card says +258.8% and -36.4% at Burston Prime's 1.35.
    #[test]
    fn a_known_rank_never_clamps_numbers_that_dont_fit() {
        let reads = [
            "Bucslc\u{201e} u, idaata / +264.3% Critical Chance / +258.8% Damage / -36 4% Zoom / 0101 / MR 15",
            "uta / +264.3% Critical Chance / +258 8% Damage / -36 Zoom / MR 15 / 0 101",
            "-00 / Burston untaata / +264.3% Critical Chance / +258.8% Damage / -3610/0 Zoom / MR 15 / 0101",
            "Burslc,i u' ilaata / +264.3% Critical Chance / +258.8% Damage / -3610/0 Zoom / MR 15 / 0101",
        ];
        let cards: Vec<Card> =
            reads.iter().filter_map(|r| parse_card(&r.split(" / ").map(String::from).collect::<Vec<_>>())).collect();
        let card = consensus(&cards).unwrap();
        let data = crate::rivens::bundled();
        let sheet = GradeSheet::default();
        let weapon = Some("/Lotus/Weapons/Tenno/LongGuns/PrimeBurston/PrimeBurston");
        let (r, _) = work_out(&data, &sheet, &card, &["LotusRifleRandomModRare".to_string()], weapon, Some(8), |_| false).unwrap();
        let shown: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        // Zoom was read -364, -36, -361 and -361 (the card says -36.4): the vote's -361 is
        // repaired to -36.1, which fits too. What matters is nothing stretched to fit.
        assert_eq!(shown[..2], ["+264.3% Critical Chance", "+258.8% Damage"]);
        assert!(matches!(shown[2], "-36.4% Zoom" | "-36.1% Zoom"), "{}", shown[2]);
        assert_eq!((r.view.disposition, r.view.rank), (1.35, 8));
    }

    /// overlay.log 2026-10-14: the reroll screen shows an unranked riven at max rank. The new
    /// roll of a rank-0 Xoris riven ("6Heat": the element icon read as a 6) failed when worked
    /// out at its rank 0, and fits at rank 8.
    #[test]
    fn the_reroll_screen_shows_max_rank() {
        let reads = [
            "Xoris Ignius / +62.8% 6Heat / +50.9% Heavy Attack / Efficiency / MR8 / 01",
            "Xoris Ignius / +62.8% / +50.9% Heavy Attack / Efficiency / MR8 / 01",
            "Xoris Ignius / +62.8% 6 Heat / +50.9% Heavy Attack / Efficiency / 01",
            "Xoris Ignius / +62.8% 6Heat / +50.9% Heavy Attack / Efficiency / MR8 / 01",
        ];
        let cards: Vec<Card> =
            reads.iter().filter_map(|r| parse_card(&r.split(" / ").map(String::from).collect::<Vec<_>>())).collect();
        let card = consensus(&cards).unwrap();
        let data = crate::rivens::bundled();
        let sheet = GradeSheet::default();
        let weapon = Some("/Lotus/Weapons/Corpus/Melee/Glaive/CrpGhostCatcherGlaive/CrpGhostCatcherGlaive");
        let types = ["PlayerMeleeWeaponRandomModRare".to_string()];
        assert!(work_out(&data, &sheet, &card, &types, weapon, Some(0), |_| false).is_err());
        let (r, _) = work_out(&data, &sheet, &card, &types, weapon, Some(crate::rivens::MAX_RANK), |_| false).unwrap();
        let shown: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(shown, ["+62.8% Heat", "+50.9% Heavy Attack Efficiency"]);
    }

    /// overlay.log 2026-10-14: the locked trait's lock icon read as a "0" against the plus
    /// ("0+126.4% Critical" / "Chance"). The line was dropped, its "Chance" joined Electricity's
    /// label, and Critical Chance came back from the name without a number.
    #[test]
    fn a_lock_icon_read_as_zero_keeps_the_stat() {
        let reads = [
            "Ax-52 Crita- / vexidra / +72.3% Electricity / 0+126.4% Critical / Chance / +43.1% Fire Rate (x2 for / Bows) / -35.6% Zoom / MR 12 / 072",
            "-vexidra / Ax-52 / +7230 ' / Aectricity / 0+126.4% Critical / Chance / +43.1% Fire Rate (x2 for / Bows) / -35.6% Zoom / MR 12 / 072",
            "Ax-52 Crita-vexidra / +72.3% f Electricity / 0+126.4% Critical / Chance / +43.1% Fire Rate (x2 for / Bows) / -35.6% Zoom / MR 12 / 072",
            "Ax-52 Crita-vexidra / +72.3% 4 Electricity / 0+126.4% Critical / Chance / +43.10/0 Fire Rate (x2 for / Bows) / -35.6% Zoom / MR 12 / 072",
        ];
        let cards: Vec<Card> =
            reads.iter().filter_map(|r| parse_card(&r.split(" / ").map(String::from).collect::<Vec<_>>())).collect();
        let card = consensus(&cards).unwrap();
        let data = crate::rivens::bundled();
        let weapon = Some("/Lotus/Weapons/Lasria/AK47/TC2024AK47Weapon");
        let types = ["LotusRifleRandomModRare".to_string()];
        let (r, _) = work_out(&data, &GradeSheet::default(), &card, &types, weapon, Some(crate::rivens::MAX_RANK), |_| false).unwrap();
        let shown: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(shown, ["+72.3% Electricity", "+126.4% Critical Chance", "+43.1% Fire Rate (x2 for Bows)", "-35.6% Zoom"]);
        assert!(r.view.stats.iter().all(|s| !s.pending));
    }

    /// overlay.log 2026-10-14: a locked negative read as "0-35.6% Zoom" (lock icon as "0") was
    /// dropped, and with four different names read, the first read (the one without Zoom) won
    /// alone. The three positives then "fit" only at a shifted disposition, every number off.
    #[test]
    fn a_locked_negative_and_a_garbled_name_still_read() {
        let reads = [
            "Ax-52 Ceinmpicron / +37.6% Ammo Maximum / +68.2% *Cold / +1 12.9% Critical Chance / 0-35.6% Zoom / MR 12 / 077",
            "npicron / Ax-52 / +37.6% Ammo Maximum / +68.2% *Cold / +1 12.9% Critical Chance / O -35.6% Zoom / MR 12 / 077",
            "Ax-52 / +37.6% Ammo Maximum / +68.2% *Cold / +1 12.9% Critical Chance / O -35.6% Zoom / MR 12 / 077",
            "Ax-52 Cell-ampicron / +37.6% Ammo Maximum / +68.2% *Cold / +112.9% Critical Chance / O -35.6% Zoom / MR 12 / 077",
        ];
        let cards: Vec<Card> =
            reads.iter().filter_map(|r| parse_card(&r.split(" / ").map(String::from).collect::<Vec<_>>())).collect();
        assert_eq!(cards[0].stats.last().map(|s| s.value), Some(-35.6), "0-35.6% Zoom");
        let card = consensus(&cards).unwrap();
        assert_eq!(card.title, "Ax-52 Cell-ampicron");
        let data = crate::rivens::bundled();
        let weapon = Some("/Lotus/Weapons/Lasria/AK47/TC2024AK47Weapon");
        let types = ["LotusRifleRandomModRare".to_string()];
        let (r, _) = work_out(&data, &GradeSheet::default(), &card, &types, weapon, Some(crate::rivens::MAX_RANK), |_| false).unwrap();
        let shown: Vec<&str> = r.view.stats.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(shown, ["+37.6% Ammo Maximum", "+68.2% Cold", "+112.9% Critical Chance", "-35.6% Zoom"]);
        assert!(r.notes.is_empty(), "{:?}", r.notes);
    }

    /// The lock icon read as a lone "O" against the minus; a weapon's hyphen still isn't a sign.
    #[test]
    fn lock_icon_letters_and_weapon_hyphens() {
        assert_eq!(stat_line("O-35.6% Zoom").map(|s| s.0), Some(-35.6));
        assert_eq!(stat_line("AX-52 Toxi-vexidex"), None);
    }
}


