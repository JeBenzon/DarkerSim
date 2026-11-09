//! The game database, as written by `scripts/build_db.py` (`app/data/game.json`).
//!
//! Every field is `#[serde(default)]` so a patch that adds or drops a field never breaks loading.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GameDb {
    pub meta: Meta,
    pub rarities: Vec<String>,
    pub attributes: Vec<String>,
    pub property_types: HashMap<String, PropType>,
    pub items: HashMap<String, ItemFamily>,
    pub secondary_pools: HashMap<String, Vec<PoolEntry>>,
    pub curves: HashMap<String, Vec<[f64; 2]>>,
    pub classes: HashMap<String, Class>,
    pub perks: HashMap<String, Named>,
    pub skills: HashMap<String, Named>,
    pub spells: HashMap<String, Spell>,
    pub buffs: HashMap<String, Buff>,
    /// character skins that change stats (one per character)
    pub skins: HashMap<String, Skin>,
    pub damage_abilities: HashMap<String, DamageAbility>,
    pub constants: HashMap<String, f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Meta {
    pub build: String,
    pub generated_at: String,
    pub notes: MetaNotes,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MetaNotes {
    pub class_base_health_add: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PropType {
    pub name: String,
    pub pct: bool,
    pub ratio: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ItemFamily {
    pub id: String,
    pub name: String,
    pub item_type: Option<String>,
    pub slot: Option<String>,
    pub hand: Option<String>,
    pub armor_type: Option<String>,
    pub weapon_types: Vec<String>,
    pub classes: Vec<String>,
    pub variants: HashMap<String, Variant>,
    pub secondary_pool: Option<String>,
    pub wiki: Option<WikiItem>,
    pub attacks: Vec<Attack>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WikiItem {
    /// keyed by rarity index + 1 ("3" = Uncommon)
    pub numenchants: HashMap<String, u32>,
    pub flavortext: Option<String>,
    pub invwidth: Option<f64>,
    pub invheight: Option<f64>,
    /// keyed by rarity index + 1
    pub sellprices: HashMap<String, Option<f64>>,
    /// keyed by rarity index + 1 (gear score is in the wiki as 0-7)
    pub gearscore: HashMap<String, Option<f64>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Variant {
    pub props: Vec<Prop>,
    pub classes: Vec<String>,
    /// icon texture name; the image is icons/<icon>.webp
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Prop {
    pub stat: String,
    pub min: f64,
    pub max: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PoolEntry {
    pub stat: String,
    pub min: f64,
    pub max: f64,
    pub weight: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Attack {
    pub name: String,
    pub group: String,
    pub damage_type: Option<String>,
    pub combo: f64,
    pub zones: Vec<f64>,
    /// seconds from swing start to the hit
    pub windup: Option<f64>,
    /// seconds until the next attack can start (windup + hit + recover)
    pub cycle: Option<f64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Class {
    pub name: String,
    pub attributes: HashMap<String, f64>,
    pub move_speed: f64,
    pub perks: Vec<String>,
    pub skills: Vec<String>,
    pub spells: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Named {
    pub name: String,
    pub classes: Vec<String>,
    /// file stem under icons/ability/
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Spell {
    pub name: String,
    pub classes: Vec<String>,
    pub cooldown: Option<f64>,
    pub charges: Option<f64>,
    pub source: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Buff {
    pub name: String,
    pub mods: Vec<Mod>,
    pub confidence: String,
    pub sources: Vec<Source>,
    pub classes: Vec<String>,
    /// milliseconds
    pub duration: Option<f64>,
    pub debuff: bool,
    /// path under icons/ without extension, e.g. "buff/Icon_Buff_Rage"
    pub icon: Option<String>,
    /// conditions this buff provides, e.g. ["drunk"] for the Ale's drunk effect
    pub tags: Vec<String>,
    /// only counts while another active buff has this tag (Brewmaster needs "drunk")
    pub requires: Option<String>,
    /// extra explanation shown in the app
    pub note: Option<String>,
}

/// A character skin with stat changes (Felidian Panther: +2 Agility, -1 Strength, -1 Vigor).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Skin {
    pub name: String,
    pub classes: Vec<String>,
    pub mods: Vec<Mod>,
    pub confidence: String,
    /// skin ids in the game (Man / Woman / colour versions share one effect)
    pub variants: Vec<String>,
    /// portrait under icons/ without extension, e.g. "skin/Skin_Character_FelidianMan_S"
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Mod {
    pub stat: String,
    /// internal units (fractions for percent stats)
    pub value: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Source {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DamageAbility {
    pub kind: String,
    pub name: String,
    pub classes: Vec<String>,
    pub components: Vec<Component>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Component {
    pub id: String,
    /// "hit" | "aoe" | "dot"
    pub kind: String,
    /// "magical" | "physical"
    pub damage_type: String,
    #[serde(rename = "true")]
    pub is_true: bool,
    pub base: f64,
    pub weapon_ratio: Option<f64>,
    /// attribute bonus ratio (1.0 = 100%)
    pub scaling: f64,
    /// seconds
    pub duration: Option<f64>,
    pub tick_interval: Option<f64>,
    pub element: Option<String>,
    pub projectile: bool,
    pub requires_perk: Option<String>,
}

/// A build template the gear generator follows (data/archetypes.json).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Archetype {
    pub id: String,
    pub cls: String,
    pub name: String,
    pub desc: String,
    pub weapons: Vec<String>,
    pub offhand: Vec<String>,
    pub armor: Vec<String>,
    pub ignore_class_restriction: Vec<String>,
    pub priority: HashMap<String, f64>,
    pub buffs: Vec<String>,
}

impl GameDb {
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    pub fn c(&self, key: &str, default: f64) -> f64 {
        self.constants.get(key).copied().unwrap_or(default)
    }

    /// Piecewise-linear curve lookup, clamped at both ends.
    pub fn curve(&self, key: &str, x: f64) -> f64 {
        let Some(k) = self.curves.get(key) else { return 0.0 };
        if k.is_empty() {
            return 0.0;
        }
        if x <= k[0][0] {
            return k[0][1];
        }
        for w in k.windows(2) {
            let ([x0, y0], [x1, y1]) = (w[0], w[1]);
            if x <= x1 {
                return if x1 == x0 { y1 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) };
            }
        }
        k[k.len() - 1][1]
    }

    pub fn is_pct(&self, stat: &str) -> bool {
        self.property_types.get(stat).map(|p| p.pct).unwrap_or(false)
    }

    /// Multiplier from the stored integer value to the internal value (0.001 for percent stats).
    pub fn ratio(&self, stat: &str) -> f64 {
        match self.property_types.get(stat) {
            Some(p) if p.ratio != 0.0 => p.ratio,
            _ => 1.0,
        }
    }

    pub fn stat_name(&self, stat: &str) -> String {
        if let Some(p) = self.property_types.get(stat) {
            if !p.name.is_empty() {
                return p.name.clone();
            }
        }
        crate::humanize(stat)
    }

    pub fn rarity_index(&self, r: &str) -> usize {
        self.rarities.iter().position(|x| x == r).unwrap_or(0)
    }

    /// Variants of an item, in rarity order.
    pub fn variants_ordered<'a>(&'a self, fam: &'a ItemFamily) -> Vec<(&'a str, &'a Variant)> {
        self.rarities
            .iter()
            .filter_map(|r| fam.variants.get(r).map(|v| (r.as_str(), v)))
            .collect()
    }

    pub fn is_weapon(fam: &ItemFamily) -> bool {
        fam.variants
            .values()
            .any(|v| v.props.iter().any(|p| p.stat == "PhysicalWeaponDamage" || p.stat == "MagicalWeaponDamage"))
    }

    pub fn class_can_use(&self, fam: &ItemFamily, rarity: &str, cls: &str) -> bool {
        let classes = fam.variants.get(rarity).map(|v| &v.classes).filter(|c| !c.is_empty()).unwrap_or(&fam.classes);
        classes.is_empty() || classes.iter().any(|c| c == cls)
    }
}
