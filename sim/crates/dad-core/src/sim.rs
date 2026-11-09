//! High-level simulation: one attack choice against a target, and median TTK across target tiers.

use crate::damage::{ability_damage, spell_ttk, weapon_info, weapon_ttk, Ttk};
use crate::data::{Archetype, GameDb};
use crate::gen::generate;
use crate::stats::{compute_stats, Loadout};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AttackChoice {
    /// primary combo with the weapon in this slot; zone 0 = sweet spot
    Weapon { slot: String, zone: usize },
    /// a damage ability (spell/skill) cast every `interval` seconds
    Ability { id: String, interval: f64, splash: bool },
}

pub fn ttk(db: &GameDb, atk_lo: &Loadout, tgt_lo: &Loadout, choice: &AttackChoice, loc: usize) -> Option<Ttk> {
    let atk = compute_stats(db, atk_lo);
    let tgt = compute_stats(db, tgt_lo);
    match choice {
        AttackChoice::Weapon { slot, zone } => {
            let w = weapon_info(db, atk_lo, slot)?;
            Some(weapon_ttk(db, &w, &atk, &tgt, *zone, loc))
        }
        AttackChoice::Ability { id, interval, splash } => {
            let r = ability_damage(db, atk_lo, &atk, &tgt, id);
            if r.is_empty() {
                return None;
            }
            Some(spell_ttk(&r, tgt.max_health, loc, *interval, *splash))
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct TierResult {
    pub tier: String,
    pub quality: f64,
    pub samples: usize,
    pub median_seconds: Option<f64>,
    pub p10_seconds: Option<f64>,
    pub p90_seconds: Option<f64>,
    pub median_hits: f64,
    pub median_hp: f64,
    /// share of targets that survived the simulated window
    pub survived: f64,
}

fn quantile(sorted: &[f64], q: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let i = ((sorted.len() - 1) as f64 * q).round() as usize;
    Some(sorted[i])
}

/// Generate `n` random targets per archetype pool at `quality` and report the TTK spread.
pub fn tier_ttk(
    db: &GameDb,
    archetypes: &[Archetype],
    atk_lo: &Loadout,
    choice: &AttackChoice,
    loc: usize,
    tier: &str,
    quality: f64,
    n: usize,
    seed: u64,
) -> TierResult {
    let mut secs = vec![];
    let mut hits = vec![];
    let mut hps = vec![];
    let mut survived = 0usize;
    let mut done = 0usize;
    for i in 0..n {
        if archetypes.is_empty() {
            break;
        }
        let arch = &archetypes[i % archetypes.len()];
        let tgt = generate(db, arch, quality, seed.wrapping_add(i as u64 * 7919));
        hps.push(compute_stats(db, &tgt).max_health);
        let Some(r) = ttk(db, atk_lo, &tgt, choice, loc) else { continue };
        done += 1;
        hits.push(r.hits as f64);
        match r.seconds {
            Some(s) => secs.push(s),
            None => survived += 1,
        }
    }
    let srt = |v: &mut Vec<f64>| v.sort_by(|a, b| a.total_cmp(b));
    srt(&mut secs);
    srt(&mut hits);
    srt(&mut hps);
    TierResult {
        tier: tier.to_string(),
        quality,
        samples: done,
        median_seconds: quantile(&secs, 0.5),
        p10_seconds: quantile(&secs, 0.1),
        p90_seconds: quantile(&secs, 0.9),
        median_hits: quantile(&hits, 0.5).unwrap_or(0.0),
        median_hp: quantile(&hps, 0.5).unwrap_or(0.0),
        survived: if done > 0 { survived as f64 / done as f64 } else { 0.0 },
    }
}
