//! Turns OCR'd lines of an in-game item tooltip (sent by DarkerLink) into an equippable item.
//!
//! DarkerLink screenshots the area around the cursor, runs Windows OCR, and sends every text line with
//! its position and its average text colour. Here we find the tooltip's item name, then read the rarity,
//! base stats (→ roll) and enchantments (blue text) below it.

use crate::data::{GameDb, ItemFamily};
use crate::stats::Enchant;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct OcrLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// average colour of the text pixels
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedItem {
    pub item: String,
    pub rarity: String,
    /// shared base-stat roll 0..1 (1.0 when the item has no ranged stats)
    pub roll: f64,
    pub enchants: Vec<Enchant>,
    /// human readable notes about guesses / unread lines
    pub notes: Vec<String>,
}

const RARITY_WORDS: [(&str, &str); 9] = [
    ("poor", "Poor"), ("common", "Common"), ("uncommon", "Uncommon"), ("rare", "Rare"), ("epic", "Epic"),
    ("legendary", "Legend"), ("legend", "Legend"), ("unique", "Unique"), ("artifact", "Artifact"),
];
/// approximate in-game name colours
const RARITY_COLORS: [(&str, [f64; 3]); 8] = [
    ("Poor", [150.0, 150.0, 150.0]), ("Common", [235.0, 232.0, 225.0]), ("Uncommon", [110.0, 210.0, 90.0]),
    ("Rare", [80.0, 160.0, 255.0]), ("Epic", [190.0, 110.0, 255.0]), ("Legend", [255.0, 175.0, 60.0]),
    ("Unique", [245.0, 230.0, 170.0]), ("Artifact", [255.0, 100.0, 70.0]),
];

pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect()
}

fn levenshtein(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

fn is_blue(l: &OcrLine) -> bool {
    let (r, g, b) = (l.r as i32, l.g as i32, l.b as i32);
    b > r + 30 && b + 10 >= g
}

/// How well a line names an item: (score, item id). Higher is better, 0 = no match.
fn match_name<'a>(db: &'a GameDb, text: &str) -> Option<(f64, &'a ItemFamily)> {
    let n = norm(text);
    if n.len() < 3 {
        return None;
    }
    let mut best: Option<(f64, &ItemFamily)> = None;
    for f in db.items.values() {
        if f.variants.values().all(|v| v.props.is_empty()) {
            continue; // not gear
        }
        let fname = norm(&f.name);
        if fname.len() < 3 {
            continue;
        }
        let score = if n == fname {
            100.0 + fname.len() as f64
        } else if n.contains(&fname) && fname.len() >= 5 {
            // e.g. a rarity word glued to the name
            60.0 + fname.len() as f64
        } else {
            // "rn" is the classic OCR misread of "m"
            let d = levenshtein(&n, &fname).min(levenshtein(&n.replace("rn", "m"), &fname.replace("rn", "m")));
            let allowed = (fname.len() / 5).max(1);
            if d <= allowed { 80.0 - d as f64 * 5.0 + fname.len() as f64 * 0.1 } else { 0.0 }
        };
        if score > 0.0 && best.map(|b| score > b.0).unwrap_or(true) {
            best = Some((score, f));
        }
    }
    best
}

/// Display label → stat id (game names plus a few aliases seen on tooltips).
fn stat_labels(db: &GameDb) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = db
        .property_types
        .iter()
        .filter(|(k, p)| !p.name.is_empty() && !k.contains('_'))
        .map(|(k, p)| (norm(&p.name), k.clone()))
        .collect();
    for (alias, stat) in [
        ("weapondamage", "PhysicalWeaponDamage"),
        ("magicweapondamage", "MagicalWeaponDamage"),
        ("movementspeed", "MoveSpeed"),
        ("magicresistance", "MagicRegistance"),
        ("allattributes", "Primitive"),
        ("additionalweapondamage", "PhysicalWeaponDamageAdd"),
        ("maxhealthadd", "MaxHealthAdd"),
    ] {
        if db.property_types.contains_key(stat) {
            v.push((alias.into(), stat.into()));
        }
    }
    // longest label first so "Physical Power Bonus" wins over "Physical Power"
    v.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
    v
}

/// Splits "Move Speed -20", "+3 Strength" or "Physical Damage Reduction 2.5%" into (label, value, is_pct).
fn split_stat(text: &str) -> Option<(String, f64, bool)> {
    let toks: Vec<&str> = text.split_whitespace().collect();
    for (i, t) in toks.iter().enumerate() {
        let pct = t.ends_with('%');
        let core = t.trim_end_matches('%').trim_start_matches('(').trim_end_matches(')');
        // common OCR swaps inside numbers
        let fixed: String = core.chars().map(|c| match c { 'O' | 'o' => '0', 'l' | 'I' | '|' => '1', ',' => '.', c => c }).collect();
        let digits = fixed.trim_start_matches(['+', '-']);
        if digits.is_empty() || !digits.chars().next().unwrap().is_ascii_digit() || !digits.chars().all(|c| c.is_ascii_digit() || c == '.') {
            continue;
        }
        let Ok(v) = fixed.parse::<f64>() else { continue };
        let label: Vec<&str> = toks.iter().enumerate().filter(|(j, _)| *j != i).map(|(_, s)| *s).collect();
        let label = label.join(" ");
        if norm(&label).len() >= 3 {
            return Some((label, v, pct));
        }
    }
    None
}

fn display_scale(db: &GameDb, stat: &str) -> f64 {
    db.ratio(stat) * if db.is_pct(stat) { 100.0 } else { 1.0 }
}

pub fn parse_tooltip(db: &GameDb, lines: &[OcrLine]) -> Result<ParsedItem, String> {
    // 1. the item name: best match, preferring big text (the tooltip title is the largest line)
    let mut name_hit: Option<(f64, &ItemFamily, &OcrLine)> = None;
    for l in lines {
        if let Some((s, f)) = match_name(db, &l.text) {
            let s = s + l.h * 0.5;
            if name_hit.map(|b| s > b.0).unwrap_or(true) {
                name_hit = Some((s, f, l));
            }
        }
    }
    let Some((_, fam, name_line)) = name_hit else {
        return Err("no item name found — hover an item so its tooltip is visible".into());
    };
    let h = name_line.h.max(8.0);
    // 2. tooltip column: lines below the name, horizontally overlapping it
    let (left, right) = (name_line.x - 14.0 * h, name_line.x + name_line.w + 14.0 * h);
    let mut col: Vec<&OcrLine> = lines
        .iter()
        .filter(|l| l.y > name_line.y + 1.0 && l.y < name_line.y + 60.0 * h && l.x >= left && l.x + l.w <= right)
        .collect();
    col.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap());

    let mut notes = vec![];
    // 3. rarity: a rarity word under the title, else the title colour
    let rarity = col
        .iter()
        .take(4)
        .flat_map(|l| l.text.split_whitespace().map(norm).collect::<Vec<_>>())
        .find_map(|w| RARITY_WORDS.iter().find(|(k, _)| *k == w).map(|(_, r)| r.to_string()))
        .unwrap_or_else(|| {
            let c = [name_line.r as f64, name_line.g as f64, name_line.b as f64];
            let r = RARITY_COLORS
                .iter()
                .min_by(|a, b| dist(a.1, c).partial_cmp(&dist(b.1, c)).unwrap())
                .map(|x| x.0)
                .unwrap_or("Common");
            notes.push(format!("rarity guessed from the name colour ({r})"));
            r.to_string()
        });
    let rarity = if fam.variants.contains_key(&rarity) {
        rarity
    } else {
        let alt = db.variants_ordered(fam).last().map(|(r, _)| r.to_string()).unwrap_or_default();
        notes.push(format!("{} has no {rarity} version, using {alt}", fam.name));
        alt
    };
    let base = &fam.variants[&rarity].props;

    // 4. stats
    let labels = stat_labels(db);
    let mut rolls = vec![];
    let mut enchants: Vec<Enchant> = vec![];
    let mut seen_base: HashMap<String, bool> = HashMap::new();
    for l in &col {
        let Some((label, val, _pct)) = split_stat(&l.text) else { continue };
        let ln = norm(&label);
        let Some(stat) = labels.iter().find(|(n, _)| ln == *n).or_else(|| labels.iter().find(|(n, _)| n.len() >= 6 && ln.ends_with(n.as_str()))).map(|x| x.1.clone()) else {
            continue;
        };
        let raw = val / display_scale(db, &stat);
        let base_prop = base.iter().find(|p| p.stat == stat);
        let enchant = is_blue(l) || base_prop.is_none() || seen_base.contains_key(&stat);
        if !enchant {
            let p = base_prop.unwrap();
            seen_base.insert(stat.clone(), true);
            if p.max != p.min {
                rolls.push(((raw - p.min) / (p.max - p.min)).clamp(0.0, 1.0));
            }
        } else if !enchants.iter().any(|e| e.stat == stat) {
            enchants.push(Enchant { stat, raw: (raw * 1000.0).round() / 1000.0 });
        }
    }
    let roll = if rolls.is_empty() { 1.0 } else { rolls.iter().sum::<f64>() / rolls.len() as f64 };
    let cap = crate::gen::enchant_count(db, fam, &rarity) as usize;
    if enchants.len() > cap {
        notes.push(format!("read {} enchantments, {} allows {cap}", enchants.len(), rarity));
    }
    Ok(ParsedItem { item: fam.id.clone(), rarity, roll, enchants, notes })
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> GameDb {
        let s = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../app/data/game.json")).unwrap();
        GameDb::from_json(&s).unwrap()
    }
    fn line(text: &str, y: f64, h: f64, rgb: (u8, u8, u8)) -> OcrLine {
        OcrLine { text: text.into(), x: 100.0, y, w: 200.0, h, r: rgb.0, g: rgb.1, b: rgb.2 }
    }

    #[test]
    fn reads_an_epic_arming_sword() {
        let db = db();
        let white = (225, 220, 210);
        let blue = (110, 160, 240);
        let lines = vec![
            line("Strength 15", 20.0, 12.0, white), // character sheet noise above
            line("Arming Sword", 100.0, 22.0, (190, 110, 255)),
            line("Epic", 130.0, 14.0, (190, 110, 255)),
            line("One Handed Sword", 150.0, 13.0, white),
            line("Physical Weapon Damage 36", 180.0, 13.0, white),
            line("Move Speed -20", 200.0, 13.0, white),
            line("Strength +2", 230.0, 13.0, blue),
            line("Physical Power Bonus 1.5%", 250.0, 13.0, blue),
            line("+3 Agility", 270.0, 13.0, blue),
        ];
        let p = parse_tooltip(&db, &lines).unwrap();
        assert_eq!(p.item, "ArmingSword");
        assert_eq!(p.rarity, "Epic");
        assert!((p.roll - 2.0 / 3.0).abs() < 1e-6, "roll {}", p.roll);
        let e: Vec<(String, f64)> = p.enchants.iter().map(|e| (e.stat.clone(), e.raw)).collect();
        assert_eq!(e, vec![("Strength".into(), 2.0), ("PhysicalDamageBonus".into(), 15.0), ("Agility".into(), 3.0)]);
    }

    #[test]
    fn tolerates_ocr_typos_and_colour_rarity() {
        let db = db();
        let lines = vec![line("Arnning Sword", 60.0, 22.0, (255, 176, 58)), line("Armlng Sword", 100.0, 22.0, (255, 176, 58)), line("Physical Weapon Damage 3O", 140.0, 13.0, (220, 220, 220))];
        let p = parse_tooltip(&db, &lines).unwrap();
        assert_eq!(p.item, "ArmingSword");
        assert_eq!(p.rarity, "Legend");
    }

    /// real Windows OCR output (DarkerLink --ocr) of a rendered test tooltip
    #[test]
    fn reads_real_ocr_output() {
        #[derive(Deserialize)]
        struct Payload { lines: Vec<OcrLine> }
        let s = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/ocr_arming_sword.json")).unwrap();
        let p: Payload = serde_json::from_str(&s).unwrap();
        let r = parse_tooltip(&db(), &p.lines).unwrap();
        assert_eq!((r.item.as_str(), r.rarity.as_str()), ("ArmingSword", "Epic"));
        assert_eq!(r.enchants.iter().map(|e| e.stat.as_str()).collect::<Vec<_>>(), vec!["Strength", "PhysicalDamageBonus", "Agility"]);
    }

    #[test]
    fn no_item_is_an_error() {
        assert!(parse_tooltip(&db(), &[line("Hello world", 10.0, 12.0, (255, 255, 255))]).is_err());
    }
}
