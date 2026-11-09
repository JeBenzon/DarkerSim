//! Gear generator: dresses a class for an archetype at a quality level.
//!
//! quality 0 = naked "Timmy", 0.1 = normal kit, 0.5 = good, 1.0 = best in slot,
//! above 1 = "BISBIS" (max rarity, perfect rolls, best enchants: theoretical strongest).

use crate::data::{Archetype, GameDb, ItemFamily, Variant};
use crate::stats::{Enchant, Equipped, Loadout, SLOTS};
use std::collections::HashMap;

/// Small deterministic RNG (xorshift64*), so a seed always gives the same kit.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// uniform in [0, 1)
    pub fn f(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    /// roughly normal, mean 0, sd ~1
    pub fn normal(&mut self) -> f64 {
        (0..4).map(|_| self.f()).sum::<f64>() * 1.732 - 3.464
    }
    pub fn pick_weighted(&mut self, weights: &[f64]) -> Option<usize> {
        let total: f64 = weights.iter().filter(|w| **w > 0.0).sum();
        if total <= 0.0 {
            return None;
        }
        let mut r = self.f() * total;
        for (i, w) in weights.iter().enumerate() {
            if *w <= 0.0 {
                continue;
            }
            if r < *w {
                return Some(i);
            }
            r -= w;
        }
        weights.iter().rposition(|w| *w > 0.0)
    }
}

const ARMOR_SLOTS: [&str; 5] = ["Head", "Chest", "Legs", "Hands", "Foot"];
const DEFAULT_ENCHANTS: [u32; 8] = [0, 0, 1, 2, 3, 4, 5, 5];

/// Value of one stat for scoring, in "points" (percent stats in percent points).
fn points(db: &GameDb, stat: &str, raw: f64) -> f64 {
    let v = raw * db.ratio(stat);
    if db.is_pct(stat) { v * 100.0 } else { v }
}

fn priority_of(arch: &Archetype, stat: &str) -> f64 {
    let key = if stat == "PhysicalWeaponDamage" || stat == "MagicalWeaponDamage" { "WeaponDamage" } else { stat };
    arch.priority.get(key).copied().unwrap_or(0.0)
}

fn variant_score(db: &GameDb, arch: &Archetype, v: &Variant) -> f64 {
    v.props.iter().map(|p| priority_of(arch, &p.stat) * points(db, &p.stat, p.max)).sum()
}

fn usable(db: &GameDb, arch: &Archetype, fam: &ItemFamily, rarity: &str) -> bool {
    db.class_can_use(fam, rarity, &arch.cls)
        || fam.armor_type.as_ref().is_some_and(|t| arch.ignore_class_restriction.contains(t))
}

pub fn enchant_count(db: &GameDb, fam: &ItemFamily, rarity: &str) -> u32 {
    let idx = db.rarity_index(rarity);
    fam.wiki
        .as_ref()
        .and_then(|w| w.numenchants.get(&(idx + 1).to_string()).copied())
        .unwrap_or(DEFAULT_ENCHANTS[idx.min(7)])
}

fn candidates<'a>(db: &'a GameDb, arch: &Archetype, slot_key: &str, slot: &str) -> Vec<&'a ItemFamily> {
    let any_usable = |f: &ItemFamily| f.variants.keys().any(|r| usable(db, arch, f, r));
    let with_props = |f: &ItemFamily| f.variants.values().any(|v| !v.props.is_empty());
    match slot_key {
        "weapon" | "offhand" => {
            let wanted = if slot_key == "weapon" { &arch.weapons } else { &arch.offhand };
            let list: Vec<&ItemFamily> =
                wanted.iter().filter_map(|id| db.items.get(id)).filter(|f| any_usable(f) && with_props(f)).collect();
            if !list.is_empty() || slot_key == "offhand" {
                return list;
            }
            let mut all: Vec<&ItemFamily> = db
                .items
                .values()
                .filter(|f| f.slot.as_deref() == Some(slot) && GameDb::is_weapon(f) && any_usable(f))
                .collect();
            all.sort_by(|a, b| a.id.cmp(&b.id));
            all
        }
        _ => {
            let mut base: Vec<&ItemFamily> = db
                .items
                .values()
                .filter(|f| f.slot.as_deref() == Some(slot) && any_usable(f) && with_props(f))
                .collect();
            base.sort_by(|a, b| a.id.cmp(&b.id));
            if ARMOR_SLOTS.contains(&slot) && !arch.armor.is_empty() {
                let filtered: Vec<&ItemFamily> = base
                    .iter()
                    .copied()
                    .filter(|f| f.armor_type.as_ref().is_some_and(|t| arch.armor.contains(t)))
                    .collect();
                if !filtered.is_empty() {
                    return filtered;
                }
            }
            base
        }
    }
}

/// Rarity index the slot aims for: q 0.1 ~ Common, 0.5 ~ Rare, 1.0 = Legendary, >1 = highest that exists.
fn target_rarity(q: f64, rng: &mut Rng) -> usize {
    if q > 1.0 {
        return 7;
    }
    let mut t = 1.0 + q * 4.0;
    if q < 1.0 {
        t += rng.normal() * 0.6;
    }
    t.round().clamp(0.0, 5.0) as usize
}

/// Nearest usable rarity of an item to the target, with its distance.
fn nearest_rarity(db: &GameDb, fam: &ItemFamily, arch: &Archetype, target: usize) -> Option<(usize, String)> {
    db.variants_ordered(fam)
        .into_iter()
        .filter(|(r, v)| usable(db, arch, fam, r) && !v.props.is_empty())
        .map(|(r, _)| ((db.rarity_index(r) as i64 - target as i64).unsigned_abs() as usize, r.to_string()))
        .min_by_key(|(d, r)| (*d, std::cmp::Reverse(db.rarity_index(r))))
}

fn roll_for(q: f64, rng: &mut Rng) -> f64 {
    if q >= 1.0 { 1.0 } else { (q + (rng.f() - 0.5) * 0.6).clamp(0.0, 1.0) }
}

fn pick_enchants(db: &GameDb, arch: &Archetype, fam: &ItemFamily, rarity: &str, q: f64, rng: &mut Rng) -> Vec<Enchant> {
    let n = enchant_count(db, fam, rarity) as usize;
    let Some(pool) = fam.secondary_pool.as_ref().and_then(|k| db.secondary_pools.get(k)) else { return vec![] };
    let base_stats: Vec<&str> = fam
        .variants
        .get(rarity)
        .map(|v| v.props.iter().map(|p| p.stat.as_str()).collect())
        .unwrap_or_default();
    let mut left: Vec<_> = pool.iter().filter(|p| !base_stats.contains(&p.stat.as_str())).collect();
    let mut out = vec![];
    for _ in 0..n {
        if left.is_empty() {
            break;
        }
        let idx = if q >= 1.0 {
            // best by priority × max value
            (0..left.len())
                .max_by(|a, b| {
                    let s = |i: usize| priority_of(arch, &left[i].stat) * points(db, &left[i].stat, left[i].max).max(1.0);
                    s(*a).total_cmp(&s(*b))
                })
                .unwrap()
        } else {
            let w: Vec<f64> = left
                .iter()
                .map(|p| (priority_of(arch, &p.stat) + 0.3).powf(1.0 + 4.0 * q) * (p.weight.max(1.0) / 1000.0))
                .collect();
            rng.pick_weighted(&w).unwrap_or(0)
        };
        let p = left.remove(idx);
        let roll = roll_for(q, rng);
        out.push(Enchant { stat: p.stat.clone(), raw: (p.min + (p.max - p.min) * roll).round() });
    }
    out
}

pub fn generate(db: &GameDb, arch: &Archetype, quality: f64, seed: u64) -> Loadout {
    let mut rng = Rng::new(seed);
    let q = quality.max(0.0);
    let mut lo = Loadout { class: arch.cls.clone(), ..Default::default() };
    lo.buffs = arch.buffs.iter().filter(|b| db.buffs.contains_key(*b)).cloned().collect();
    if q <= 0.0 {
        return lo;
    }
    let fill = (q * 10.0).min(1.0);
    let mut two_handed = false;
    for (key, _, slot) in SLOTS {
        if *key == "offhand" && two_handed {
            continue;
        }
        if *key != "weapon" && rng.f() >= fill {
            continue;
        }
        let cands = candidates(db, arch, key, slot);
        if cands.is_empty() {
            continue;
        }
        // only items that exist closest to the target rarity compete
        // (so a low-tier kit can't pull in Legendary-only sets)
        let target = target_rarity(q, &mut rng);
        let near: Vec<(usize, String, &ItemFamily)> =
            cands.iter().filter_map(|f| nearest_rarity(db, f, arch, target).map(|(d, r)| (d, r, *f))).collect();
        let Some(best_d) = near.iter().map(|n| n.0).min() else { continue };
        let mut scored: Vec<(f64, &ItemFamily, String)> = near
            .into_iter()
            .filter(|n| n.0 == best_d)
            .filter_map(|(_, r, f)| {
                let mut s = variant_score(db, arch, f.variants.get(&r)?);
                if matches!(*key, "weapon" | "offhand") {
                    // archetype list order is preference order
                    let list = if *key == "weapon" { &arch.weapons } else { &arch.offhand };
                    if let Some(pos) = list.iter().position(|id| *id == f.id) {
                        s += 1000.0 - pos as f64 * 10.0;
                    }
                }
                Some((s, f, r))
            })
            .collect();
        if scored.is_empty() {
            continue;
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        let k = if q >= 1.0 { 1 } else { 3.min(scored.len()) };
        let (_, fam, rarity) = scored.swap_remove((rng.f() * k as f64) as usize);
        if *key == "weapon" && fam.hand.as_deref() == Some("TwoHanded") {
            two_handed = true;
        }
        let enchants = pick_enchants(db, arch, fam, &rarity, q, &mut rng);
        lo.gear.insert(key.to_string(), Equipped { item: fam.id.clone(), roll: roll_for(q, &mut rng), rarity, enchants });
    }
    lo
}

/// Target quality tiers used for "median TTK".
pub const TIERS: [(&str, f64); 5] =
    [("Timmy (naked)", 0.0), ("Low tier", 0.1), ("Average", 0.3), ("Good", 0.5), ("BIS", 1.0)];

pub fn archetypes_from_json(s: &str) -> Result<Vec<Archetype>, serde_json::Error> {
    serde_json::from_str(s)
}

/// Archetypes indexed by id.
pub fn by_id(list: &[Archetype]) -> HashMap<&str, &Archetype> {
    list.iter().map(|a| (a.id.as_str(), a)).collect()
}
