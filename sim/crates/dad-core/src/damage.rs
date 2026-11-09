//! Damage: weapon swings, spell components (hit / splash / damage over time) and time-to-kill.
//!
//! Weapon formula (community-documented, wiki "Damage"):
//!   ((((Base + BuffWeaponDmg) × Combo × ImpactZone + GearWeaponDmg) × (1 + PowerBonus) + AdditionalDmg)
//!     × HitLocation × DRmult × (1 − ProjectileDR)) + TrueDmg
//!   DRmult = max(1 − DR·(1 − min(Pen, 1)), 1 − DR)
//!
//! Spells: (Base + ABR·WeaponMagicalDmg) × (1 + ABR·MagicalPowerBonus) + ABR·AdditionalMagicalDmg,
//! then hit location (direct hits only), magical DR, projectile DR and true damage.

use crate::data::{Attack, Component, GameDb};
use crate::stats::{prop_value, weapon_in, Loadout, Stats};
use serde::Serialize;

/// (label, hitbox constant)
pub const HIT_LOCS: [(&str, &str); 6] = [
    ("Head", "Hitbox_Head"),
    ("Body", "Hitbox_Body"),
    ("Arm", "Hitbox_Arm"),
    ("Hand", "Hitbox_Hand"),
    ("Leg", "Hitbox_Leg"),
    ("Foot", "Hitbox_Foot"),
];
const HIT_DEFAULTS: [f64; 6] = [150.0, 100.0, 80.0, 70.0, 60.0, 50.0];

pub fn loc_index(name: &str) -> usize {
    HIT_LOCS.iter().position(|(n, _)| *n == name).unwrap_or(1)
}

pub fn location_mult(db: &GameDb, loc: usize, atk: &Stats, tgt: &Stats) -> f64 {
    let (name, key) = HIT_LOCS[loc];
    let m = db.c(key, HIT_DEFAULTS[loc]) / 100.0;
    if name == "Head" {
        (m + atk.headshot_bonus - tgt.headshot_reduction).max(0.0)
    } else {
        m
    }
}

pub fn dr_mult(dr: f64, pen: f64) -> f64 {
    (1.0 - dr * (1.0 - pen.min(1.0))).max(1.0 - dr)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Breakdown {
    pub magical: bool,
    pub base: f64,
    pub buff_weapon: f64,
    pub combo: f64,
    pub zone: f64,
    pub gear_weapon: f64,
    pub power_bonus: f64,
    pub additional: f64,
    pub pre_location: f64,
    pub loc_mult: f64,
    pub dr: f64,
    pub pen: f64,
    pub dr_mult: f64,
    pub projectile_mult: f64,
    pub true_dmg: f64,
    pub total: f64,
}

/// Facts about the weapon held in a slot.
#[derive(Debug, Clone)]
pub struct WeaponInfo {
    pub name: String,
    pub base: f64,
    pub magical: bool,
    pub projectile: bool,
    pub attacks: Vec<Attack>,
}

pub fn weapon_info(db: &GameDb, lo: &Loadout, slot: &str) -> Option<WeaponInfo> {
    let (fam, eq) = weapon_in(db, lo, slot)?;
    let v = fam.variants.get(&eq.rarity)?;
    let phys = v.props.iter().find(|p| p.stat == "PhysicalWeaponDamage");
    let mag = v.props.iter().find(|p| p.stat == "MagicalWeaponDamage");
    let magical = phys.is_none() && mag.is_some();
    let base = prop_value(db, phys.or(mag)?, eq.roll);
    let projectile = fam.weapon_types.iter().any(|t| t.contains("Bow") || t.contains("Crossbow"));
    let mut attacks = fam.attacks.clone();
    if attacks.is_empty() {
        attacks.push(Attack { name: "Hit".into(), group: "Primary".into(), combo: 1.0, zones: vec![1.0], ..Default::default() });
    }
    Some(WeaponInfo { name: fam.name.clone(), base, magical, projectile, attacks })
}

/// Magical weapon damage an equipped staff/book/orb adds to spells.
fn spell_weapon_damage(db: &GameDb, lo: &Loadout) -> f64 {
    ["weapon", "offhand"]
        .iter()
        .filter_map(|s| {
            let eq = lo.gear.get(&crate::stats::active_key(lo, s))?;
            let v = db.items.get(&eq.item)?.variants.get(&eq.rarity)?;
            v.props.iter().find(|p| p.stat == "MagicalWeaponDamage").map(|p| prop_value(db, p, eq.roll))
        })
        .fold(0.0, f64::max)
}

pub fn weapon_hit(
    db: &GameDb,
    w: &WeaponInfo,
    atk: &Stats,
    tgt: &Stats,
    attack: &Attack,
    zone_idx: usize,
    loc: usize,
) -> Breakdown {
    let mut b = Breakdown { magical: w.magical, base: w.base, combo: attack.combo, ..Default::default() };
    b.zone = attack.zones.get(zone_idx.min(attack.zones.len().saturating_sub(1))).copied().unwrap_or(1.0);
    b.buff_weapon = atk.g("BuffWeaponDamage");
    if w.magical {
        b.gear_weapon = atk.g("MagicalWeaponDamageAdd");
        b.power_bonus = atk.magical_power_bonus;
        b.additional = atk.g("MagicalDamageAdd");
        b.true_dmg = atk.g("MagicalDamageTrue");
        b.pen = atk.magic_penetration;
        b.dr = tgt.magical_reduction;
    } else {
        b.gear_weapon = atk.g("PhysicalWeaponDamageAdd");
        b.power_bonus = atk.physical_power_bonus;
        b.additional = atk.g("PhysicalDamageAdd");
        b.true_dmg = atk.g("PhysicalDamageTrue");
        b.pen = atk.armor_penetration;
        b.dr = tgt.physical_reduction;
    }
    b.pre_location =
        ((b.base + b.buff_weapon) * b.combo * b.zone + b.gear_weapon) * (1.0 + b.power_bonus) + b.additional;
    b.loc_mult = location_mult(db, loc, atk, tgt);
    b.dr_mult = dr_mult(b.dr, b.pen);
    b.projectile_mult = if w.projectile { 1.0 - tgt.projectile_reduction } else { 1.0 };
    b.total = (b.pre_location * b.loc_mult * b.dr_mult * b.projectile_mult + b.true_dmg).max(0.0);
    b
}

// ---------------------------------------------------------------- spells

#[derive(Debug, Clone, Serialize)]
pub struct ComponentResult {
    pub id: String,
    pub kind: String,
    pub magical: bool,
    /// damage before location / reduction
    pub powered: f64,
    /// direct hit: damage per hit location (HIT_LOCS order)
    pub per_location: Option<[f64; 6]>,
    /// splash: single value (no hit location)
    pub flat: Option<f64>,
    /// damage over time
    pub per_tick: Option<f64>,
    pub ticks: Option<u32>,
    pub tick_interval: Option<f64>,
    pub dot_total: Option<f64>,
    pub dr_mult: f64,
}

/// Components that apply for this loadout: perk-gated versions replace their base version.
pub fn active_components<'a>(comps: &'a [Component], perks: &[String]) -> Vec<&'a Component> {
    let replaced: Vec<String> = comps
        .iter()
        .filter_map(|c| {
            let p = c.requires_perk.as_ref()?;
            perks.contains(p).then(|| c.id.strip_suffix(&format!("_{p}")).unwrap_or(&c.id).to_string())
        })
        .collect();
    comps
        .iter()
        .filter(|c| match &c.requires_perk {
            Some(p) => perks.contains(p),
            None => !replaced.contains(&c.id),
        })
        .collect()
}

pub fn component_damage(db: &GameDb, lo: &Loadout, atk: &Stats, tgt: &Stats, c: &Component) -> ComponentResult {
    let magical = c.damage_type != "physical";
    let s = if c.scaling == 0.0 && c.is_true { 0.0 } else { c.scaling };
    let (weapon_dmg, power, add, tru, pen, dr) = if magical {
        (
            spell_weapon_damage(db, lo) + atk.g("MagicalWeaponDamageAdd"),
            atk.magical_power_bonus,
            atk.g("MagicalDamageAdd"),
            atk.g("MagicalDamageTrue"),
            atk.magic_penetration,
            tgt.magical_reduction,
        )
    } else {
        let wd = weapon_info(db, lo, &crate::stats::active_key(lo, "weapon")).map(|w| w.base).unwrap_or(0.0);
        (
            wd * c.weapon_ratio.unwrap_or(0.0) + atk.g("PhysicalWeaponDamageAdd"),
            atk.physical_power_bonus,
            atk.g("PhysicalDamageAdd"),
            atk.g("PhysicalDamageTrue"),
            atk.armor_penetration,
            tgt.physical_reduction,
        )
    };
    let powered = if c.is_true {
        c.base
    } else {
        (c.base + s * weapon_dmg) * (1.0 + s * power) + s * add
    };
    let drm = if c.is_true { 1.0 } else { dr_mult(dr, pen) };
    let proj = if c.projectile { 1.0 - tgt.projectile_reduction } else { 1.0 };
    let mut r = ComponentResult {
        id: c.id.clone(),
        kind: c.kind.clone(),
        magical,
        powered,
        per_location: None,
        flat: None,
        per_tick: None,
        ticks: None,
        tick_interval: None,
        dot_total: None,
        dr_mult: drm,
    };
    match c.kind.as_str() {
        "dot" => {
            let interval = c.tick_interval.unwrap_or(1.0).max(0.05);
            let dur = c.duration.unwrap_or(interval);
            let base_ticks = (dur / interval).round().max(1.0);
            let ticks = ((dur * (1.0 + tgt.debuff_duration)) / interval + 1e-9).floor().max(1.0);
            let per_tick = powered / base_ticks * drm;
            r.per_tick = Some(per_tick);
            r.ticks = Some(ticks as u32);
            r.tick_interval = Some(interval);
            r.dot_total = Some(per_tick * ticks);
        }
        "aoe" => r.flat = Some((powered * drm * proj).max(0.0)),
        _ => {
            let mut a = [0.0; 6];
            for (i, v) in a.iter_mut().enumerate() {
                *v = (powered * location_mult(db, i, atk, tgt) * drm * proj + tru).max(0.0);
            }
            r.per_location = Some(a);
        }
    }
    r
}

pub fn ability_damage(db: &GameDb, lo: &Loadout, atk: &Stats, tgt: &Stats, ability: &str) -> Vec<ComponentResult> {
    let Some(a) = db.damage_abilities.get(ability) else { return vec![] };
    active_components(&a.components, &lo.perks)
        .into_iter()
        .map(|c| component_damage(db, lo, atk, tgt, c))
        .collect()
}

// ---------------------------------------------------------------- time to kill

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub t: f64,
    pub label: String,
    pub damage: f64,
    pub hp_after: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Ttk {
    pub hits: u32,
    /// seconds; None if the target survives the simulated window
    pub seconds: Option<f64>,
    pub events: Vec<Event>,
    /// true when the weapon has no animation data and a 1 s swing was assumed
    pub assumed_timing: bool,
}

const MAX_SIM_SECONDS: f64 = 180.0;

/// Swing the primary combo on repeat until the target dies.
pub fn weapon_ttk(db: &GameDb, w: &WeaponInfo, atk: &Stats, tgt: &Stats, zone: usize, loc: usize) -> Ttk {
    let combo: Vec<&Attack> = {
        let p: Vec<&Attack> = w.attacks.iter().filter(|a| a.group == "Primary").collect();
        if p.is_empty() { w.attacks.iter().take(1).collect() } else { p }
    };
    let speed = (1.0 + atk.action_speed).max(0.05);
    let mut hp = tgt.max_health;
    let mut t = 0.0;
    let mut events = vec![];
    let mut assumed = false;
    let mut i = 0usize;
    while hp > 0.0 && t < MAX_SIM_SECONDS && events.len() < 500 {
        let a = combo[i % combo.len()];
        let (windup, cycle) = match (a.windup, a.cycle) {
            (Some(wu), Some(c)) => (wu, c),
            (None, Some(c)) => (c * 0.5, c),
            _ => {
                assumed = true;
                (0.5, 1.0)
            }
        };
        let dmg = weapon_hit(db, w, atk, tgt, a, zone, loc).total;
        let hit_t = t + windup / speed;
        hp -= dmg;
        events.push(Event { t: hit_t, label: a.name.clone(), damage: dmg, hp_after: hp.max(0.0) });
        if dmg <= 0.0 {
            break;
        }
        t += cycle / speed;
        i += 1;
    }
    let dead = hp <= 0.0;
    Ttk { hits: events.len() as u32, seconds: dead.then(|| events.last().map(|e| e.t).unwrap_or(0.0)), events, assumed_timing: assumed }
}

/// Cast a spell every `interval` seconds; damage over time refreshes (does not stack) on recast.
pub fn spell_ttk(results: &[ComponentResult], tgt_hp: f64, loc: usize, interval: f64, include_splash: bool) -> Ttk {
    let interval = interval.max(0.1);
    let mut events: Vec<Event> = vec![];
    let mut hp = tgt_hp;
    let mut casts = 0u32;
    // pending dot ticks: (time, damage, label)
    let mut dots: Vec<(f64, f64, String)> = vec![];
    let mut next_cast = 0.0;
    let direct: f64 = results
        .iter()
        .map(|r| {
            r.per_location.map(|a| a[loc]).unwrap_or(0.0) + if include_splash { r.flat.unwrap_or(0.0) } else { 0.0 }
        })
        .sum();
    let has_dot = results.iter().any(|r| r.per_tick.is_some());
    if direct <= 0.0 && !has_dot {
        return Ttk { hits: 0, seconds: None, events, assumed_timing: false };
    }
    while hp > 0.0 && next_cast < MAX_SIM_SECONDS {
        // resolve dot ticks that happen before the next cast
        dots.sort_by(|a, b| a.0.total_cmp(&b.0));
        while let Some(pos) = dots.iter().position(|d| d.0 < next_cast) {
            let (t, d, l) = dots.remove(pos);
            hp -= d;
            events.push(Event { t, label: l, damage: d, hp_after: hp.max(0.0) });
            if hp <= 0.0 {
                break;
            }
        }
        if hp <= 0.0 {
            break;
        }
        let t = next_cast;
        casts += 1;
        if direct > 0.0 {
            hp -= direct;
            events.push(Event { t, label: format!("Cast {casts}"), damage: direct, hp_after: hp.max(0.0) });
        }
        for r in results {
            if let (Some(pt), Some(n), Some(iv)) = (r.per_tick, r.ticks, r.tick_interval) {
                dots.retain(|d| d.2 != r.id); // refresh
                for k in 1..=n {
                    dots.push((t + iv * k as f64, pt, r.id.clone()));
                }
            }
        }
        next_cast += interval;
    }
    // finish dots if still alive after the window
    let dead = hp <= 0.0;
    Ttk { hits: casts, seconds: dead.then(|| events.last().map(|e| e.t).unwrap_or(0.0)), events, assumed_timing: false }
}
