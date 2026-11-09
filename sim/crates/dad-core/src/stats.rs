//! Loadout -> character sheet. Port of `computeStats` in app/app.js, plus buffs.

use crate::data::{GameDb, ItemFamily, Prop};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Equipment slots: (key, label, item slot in the data).
pub const SLOTS: &[(&str, &str, &str)] = &[
    ("weapon", "Weapon", "Primary"),
    ("offhand", "Off-hand", "Secondary"),
    ("head", "Head", "Head"),
    ("chest", "Chest", "Chest"),
    ("legs", "Legs", "Legs"),
    ("hands", "Hands", "Hands"),
    ("foot", "Feet", "Foot"),
    ("back", "Cloak", "Back"),
    ("sash", "Sash", "Sash"),
    ("necklace", "Necklace", "Necklace"),
    ("ring1", "Ring", "Ring"),
    ("ring2", "Ring", "Ring"),
];

/// Second weapon set (swap with the weapon-swap key in game). Only the active set counts.
pub const SET2_SLOTS: &[(&str, &str, &str)] = &[("weapon2", "Weapon (set 2)", "Primary"), ("offhand2", "Off-hand (set 2)", "Secondary")];

/// SLOTS + SET2_SLOTS: every equipment key.
pub fn all_slots() -> impl Iterator<Item = &'static (&'static str, &'static str, &'static str)> {
    SLOTS.iter().chain(SET2_SLOTS.iter())
}

/// Whether gear in this slot counts (weapons of the inactive set don't).
pub fn slot_active(lo: &Loadout, key: &str) -> bool {
    match key {
        "weapon" | "offhand" => lo.weapon_set == 0,
        "weapon2" | "offhand2" => lo.weapon_set == 1,
        _ => true,
    }
}

/// "weapon" / "offhand" of the active weapon set ("weapon2"/"offhand2" when set 2 is out).
pub fn active_key(lo: &Loadout, base: &str) -> String {
    if lo.weapon_set == 1 && matches!(base, "weapon" | "offhand") { format!("{base}2") } else { base.to_string() }
}

pub const WEAPON_DMG: [&str; 2] = ["PhysicalWeaponDamage", "MagicalWeaponDamage"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Enchant {
    pub stat: String,
    /// value as stored in the game (e.g. 25 = 2.5% for a percent stat)
    pub raw: f64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Equipped {
    pub item: String,
    pub rarity: String,
    /// 0 = min roll, 1 = max roll of the base properties
    pub roll: f64,
    pub enchants: Vec<Enchant>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Loadout {
    pub class: String,
    pub gear: BTreeMap<String, Equipped>,
    /// buff ids from the buff catalog (treated as always on)
    pub buffs: Vec<String>,
    /// selected perks (enables perk-gated spell components such as Fire Mastery)
    pub perks: Vec<String>,
    /// 0 = weapon set 1 in hand, 1 = set 2
    pub weapon_set: u8,
    /// character skin id from `db.skins` (skins like Felidian change attributes)
    pub skin: Option<String>,
}

pub fn prop_value(db: &GameDb, p: &Prop, roll: f64) -> f64 {
    let raw = p.min + (p.max - p.min) * roll.clamp(0.0, 1.0);
    raw * db.ratio(&p.stat)
}

pub fn enchant_value(db: &GameDb, e: &Enchant) -> f64 {
    e.raw * db.ratio(&e.stat)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Stats {
    pub attrs: BTreeMap<String, f64>,
    /// summed flat totals from gear + buffs (internal units)
    pub totals: HashMap<String, f64>,
    pub physical_power: f64,
    pub physical_power_bonus: f64,
    pub magical_power: f64,
    pub magical_power_bonus: f64,
    pub health_rating: f64,
    pub base_health: f64,
    pub max_health: f64,
    pub health_recovery: f64,
    pub move_speed_raw: f64,
    pub move_speed: f64,
    pub action_rating: f64,
    pub action_speed: f64,
    pub spell_casting_speed: f64,
    pub manual_dexterity: f64,
    pub item_equip_speed: f64,
    pub regular_interaction_speed: f64,
    pub magical_interaction_speed: f64,
    pub memory_capacity: f64,
    pub cooldown_reduction: f64,
    pub buff_duration: f64,
    pub debuff_duration: f64,
    pub armor_rating: f64,
    pub physical_reduction: f64,
    pub magic_resistance: f64,
    pub magical_reduction: f64,
    pub projectile_reduction: f64,
    pub headshot_reduction: f64,
    pub headshot_bonus: f64,
    pub armor_penetration: f64,
    pub magic_penetration: f64,
    pub physical_shield: f64,
    pub magical_shield: f64,
    /// buff stats the engine does not model yet (shown in the UI so nothing is silently dropped)
    pub unmodeled: Vec<String>,
}

impl Stats {
    pub fn g(&self, k: &str) -> f64 {
        self.totals.get(k).copied().unwrap_or(0.0)
    }
}

/// Stats the engine reads from `totals`. Anything else coming from a buff is reported as unmodeled.
const MODELED: &[&str] = &[
    "Primitive", "PhysicalPower", "PhysicalDamageBonus", "MagicalPower", "MagicalDamageBonus", "MaxHealthBonus",
    "MaxHealthAdd", "MoveSpeed", "MoveSpeedAdd", "MoveSpeedBonus", "ActionSpeed", "ManualDexterity", "ItemEquipSpeed",
    "RegularInteractionSpeed", "MagicalInteractionSpeed", "SpellCastingSpeed", "MemoryCapacityBonus",
    "MemoryCapacityAdd", "CooldownReductionBonus", "BuffDurationBonus", "DebuffDurationBonus", "ArmorRating",
    "ArmorRatingAdd", "ItemArmorRatingMod", "PhysicalDamageReduction", "PhysicalReductionMod", "MagicRegistance",
    "MagicalDamageReduction", "MagicalReductionMod", "ProjectileReductionMod", "HeadshotReductionMod",
    "HeadshotDamageMod", "ArmorPenetration", "MagicPenetration", "PhysicalDamageAdd", "MagicalDamageAdd",
    "PhysicalWeaponDamageAdd", "MagicalWeaponDamageAdd", "PhysicalDamageTrue", "MagicalDamageTrue",
    "BuffWeaponDamage", "PhysicalShield", "MagicalShield", "Luck", "Persuasiveness",
];

/// Whether an applied buff counts right now: a buff with `requires` (Brewmaster -> "drunk")
/// only works while another applied buff carries that tag (an Ale's "Drunk").
pub fn buff_active(db: &GameDb, lo: &Loadout, id: &str) -> bool {
    let Some(req) = db.buffs.get(id).and_then(|b| b.requires.as_deref()) else { return true };
    lo.buffs.iter().any(|o| o != id && db.buffs.get(o).is_some_and(|b| b.tags.iter().any(|t| t == req)))
}

pub fn gear_totals(db: &GameDb, lo: &Loadout) -> HashMap<String, f64> {
    let mut g: HashMap<String, f64> = HashMap::new();
    for (key, eq) in &lo.gear {
        if !slot_active(lo, key) {
            continue;
        }
        let Some(v) = db.items.get(&eq.item).and_then(|f| f.variants.get(&eq.rarity)) else { continue };
        for p in &v.props {
            if !WEAPON_DMG.contains(&p.stat.as_str()) {
                *g.entry(p.stat.clone()).or_default() += prop_value(db, p, eq.roll);
            }
        }
        for e in &eq.enchants {
            if !e.stat.is_empty() {
                *g.entry(e.stat.clone()).or_default() += enchant_value(db, e);
            }
        }
    }
    if let Some(sk) = lo.skin.as_ref().and_then(|k| db.skins.get(k)) {
        for m in &sk.mods {
            *g.entry(m.stat.clone()).or_default() += m.value;
        }
    }
    for id in &lo.buffs {
        if !buff_active(db, lo, id) {
            continue;
        }
        if let Some(b) = db.buffs.get(id) {
            for m in &b.mods {
                *g.entry(m.stat.clone()).or_default() += m.value;
            }
        }
    }
    g
}

pub fn compute_stats(db: &GameDb, lo: &Loadout) -> Stats {
    compute_stats_delta(db, lo, &HashMap::new())
}

pub fn compute_stats_delta(db: &GameDb, lo: &Loadout, attr_delta: &HashMap<String, f64>) -> Stats {
    let empty = Default::default();
    let cls = db.classes.get(&lo.class).unwrap_or(&empty);
    let totals = gear_totals(db, lo);
    let g = |k: &str| totals.get(k).copied().unwrap_or(0.0);
    let mut s = Stats::default();

    for a in &db.attributes {
        let flat = cls.attributes.get(a).copied().unwrap_or(0.0)
            + g(a)
            + g("Primitive")
            + attr_delta.get(a).copied().unwrap_or(0.0);
        let modp = g(&format!("{a}Mod"));
        s.attrs.insert(a.clone(), flat * (1.0 + modp));
    }
    let a = |k: &str| s.attrs.get(k).copied().unwrap_or(0.0);
    let (str_, vig, agi, dex, will, kno, res) =
        (a("Strength"), a("Vigor"), a("Agility"), a("Dexterity"), a("Will"), a("Knowledge"), a("Resourcefulness"));

    s.physical_power = str_ + g("PhysicalPower");
    s.physical_power_bonus = db.curve("CT_PhysicalPower.PhysicalDamageMod", s.physical_power) + g("PhysicalDamageBonus");
    s.magical_power = db.curve("CT_Will.MagicalPower", will) + g("MagicalPower");
    s.magical_power_bonus = db.curve("CT_MagicalPower.MagicalDamageMod", s.magical_power) + g("MagicalDamageBonus");
    s.health_rating = str_ * 0.25 + vig * 0.75;
    s.base_health = db.curve("CT_MaxHealthBase.MaxHealthBase", s.health_rating);
    s.max_health =
        (s.base_health * (1.0 + g("MaxHealthBonus")) + g("MaxHealthAdd") + db.meta.notes.class_base_health_add).ceil();
    s.health_recovery = db.curve("CT_RecoveryMod.HealthRecoveryMod", vig);

    let ms_cap = db.c(&format!("{}MaxMoveSpeed", lo.class), db.c("CharacterMaxMoveSpeed", 500.0));
    let base_ms = if cls.move_speed > 0.0 { cls.move_speed } else { db.c("CharacterBaseMoveSpeed", 300.0) };
    s.move_speed_raw = (base_ms + db.curve("CT_Agility.MoveSpeedBase", agi) + g("MoveSpeed") + g("MoveSpeedAdd"))
        * (1.0 + g("MoveSpeedBonus"));
    s.move_speed = s.move_speed_raw.min(ms_cap);

    s.action_rating = agi * 0.25 + dex * 0.75;
    s.action_speed = db.curve("CT_ActionSpeed.ActionSpeed", s.action_rating) + g("ActionSpeed");
    s.manual_dexterity = db.curve("CT_Dexterity.ManualDexterity", dex) + g("ManualDexterity");
    s.item_equip_speed = db.curve("CT_Dexterity.ItemEquipSpeed", dex) + g("ItemEquipSpeed");
    s.regular_interaction_speed =
        db.curve("CT_RegularInteractionSpeedBase.RegularInteractionSpeed", dex * 0.25 + res * 0.75)
            + g("RegularInteractionSpeed");
    s.magical_interaction_speed = db.curve("CT_Will.MagicalInteractionSpeed", will) + g("MagicalInteractionSpeed");
    s.spell_casting_speed = (db.curve("CT_Knowledge.SpellCastingSpeed", kno) + g("SpellCastingSpeed"))
        .clamp(db.c("Attribute_MinSpellCastingSpeed", -0.95), db.c("Attribute_MaxSpellCastingSpeed", 2.0));
    s.memory_capacity = (db.curve("CT_Knowledge.MemoryCapacity", kno) * (1.0 + g("MemoryCapacityBonus"))).ceil()
        + g("MemoryCapacityAdd");
    s.cooldown_reduction = (db.curve("CT_Resourcefulness.CooldownReduction", res) + g("CooldownReductionBonus"))
        .min(db.c("Attribute_MaxCooldownReductionMod", 0.65));
    s.buff_duration = db.curve("CT_Will.BuffDurationMod", will) + g("BuffDurationBonus");
    s.debuff_duration = (db.curve("CT_Will.DebuffDurationMod", will) + g("DebuffDurationBonus"))
        .max(db.c("Attribute_MinDebuffDurationMod", -0.95));

    s.armor_rating = g("ArmorRating") * (1.0 + g("ItemArmorRatingMod")) + g("ArmorRatingAdd");
    s.physical_reduction = ((db.curve("CT_ArmorRating.PhysicalReduction", s.armor_rating)
        + g("PhysicalDamageReduction"))
        * (1.0 + g("PhysicalReductionMod")))
    .clamp(db.c("Attribute_MinPhysicalDamageReduction", -6.95), db.c("Attribute_MaxPhysicalDamageReduction", 0.95));
    s.magic_resistance = db.curve("CT_Will.MagicResistance", will) + g("MagicRegistance");
    s.magical_reduction = ((db.curve("CT_MagicResistance.MagicalReduction", s.magic_resistance)
        + g("MagicalDamageReduction"))
        * (1.0 + g("MagicalReductionMod")))
    .clamp(db.c("Attribute_MinMagicalDamageReduction", -6.95), db.c("Attribute_MaxMagicalDamageReduction", 0.95));
    s.projectile_reduction = g("ProjectileReductionMod").min(db.c("Attribute_MaxProjectileReductionMod", 0.95));
    s.headshot_reduction = g("HeadshotReductionMod");
    s.headshot_bonus = g("HeadshotDamageMod").min(db.c("Attribute_MaxHeadshotDamageMod", 2.0));
    s.armor_penetration = g("ArmorPenetration");
    s.magic_penetration = g("MagicPenetration");
    s.physical_shield = g("PhysicalShield");
    s.magical_shield = g("MagicalShield");

    let attr_names: Vec<String> = db.attributes.iter().flat_map(|a| [a.clone(), format!("{a}Mod")]).collect();
    let mut unmodeled: Vec<String> = lo
        .buffs
        .iter()
        .filter(|id| buff_active(db, lo, id))
        .filter_map(|id| db.buffs.get(id))
        .flat_map(|b| b.mods.iter().map(|m| m.stat.clone()))
        .filter(|st| !MODELED.contains(&st.as_str()) && !attr_names.contains(st))
        .collect();
    unmodeled.sort();
    unmodeled.dedup();
    s.unmodeled = unmodeled;
    s.totals = totals;
    s
}

/// The weapon in a slot, if that slot holds something with weapon damage.
pub fn weapon_in<'a>(db: &'a GameDb, lo: &'a Loadout, slot: &str) -> Option<(&'a ItemFamily, &'a Equipped)> {
    let eq = lo.gear.get(slot)?;
    let fam = db.items.get(&eq.item)?;
    let v = fam.variants.get(&eq.rarity)?;
    v.props.iter().any(|p| WEAPON_DMG.contains(&p.stat.as_str())).then_some((fam, eq))
}
