//! dad-core: the Dark and Darker simulator engine. Pure Rust, no I/O; runs natively and in WebAssembly.

pub mod damage;
pub mod data;
pub mod gen;
pub mod sim;
pub mod stats;
pub mod tooltip;

pub use data::GameDb;

/// "PhysicalDamageBonus" -> "Physical Damage Bonus"
pub fn humanize(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    let mut prev_lower = false;
    for ch in s.chars() {
        if ch == '_' {
            out.push(' ');
            prev_lower = false;
            continue;
        }
        if ch.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::damage::*;
    use super::gen::*;
    use super::sim::*;
    use super::stats::*;
    use super::*;
    use std::sync::OnceLock;

    fn db() -> &'static GameDb {
        static DB: OnceLock<GameDb> = OnceLock::new();
        DB.get_or_init(|| {
            let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../app/data/game.json");
            GameDb::from_json(&std::fs::read_to_string(p).expect("run scripts/build_db.py first")).unwrap()
        })
    }
    fn archetypes() -> Vec<data::Archetype> {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../data/archetypes.json");
        archetypes_from_json(&std::fs::read_to_string(p).unwrap()).unwrap()
    }
    fn naked(cls: &str) -> Loadout {
        Loadout { class: cls.into(), ..Default::default() }
    }

    #[test]
    fn loads_and_base_stats() {
        let s = compute_stats(db(), &naked("Fighter"));
        assert_eq!(s.max_health, 100.0, "fighter hp");
        assert_eq!(s.armor_rating, 0.0);
    }

    /// Checked against the in-game stats window (2026-11-07): naked Cleric wearing the Felidian
    /// (Panther) skin shows Str 10 / Vig 12 / Agi 14, Health 93, Move Speed 298, Action Speed -1%.
    #[test]
    fn naked_cleric_felidian_matches_game() {
        let mut lo = naked("Cleric");
        lo.skin = Some("FelidianPanther".into());
        let s = compute_stats(db(), &lo);
        assert_eq!((s.attrs["Strength"], s.attrs["Vigor"], s.attrs["Agility"]), (10.0, 12.0, 14.0));
        assert_eq!(s.max_health, 93.0);
        assert!((s.move_speed - 298.0).abs() < 1e-9, "move speed {}", s.move_speed);
        assert!((s.action_speed + 0.01).abs() < 1e-9, "action speed {}", s.action_speed);
        assert!((s.magical_reduction - 0.143).abs() < 0.0005, "magical DR {}", s.magical_reduction);
        assert_eq!(s.memory_capacity, 20.0);
    }

    #[test]
    fn longsword_epic_damage() {
        let db = db();
        let mut lo = naked("Fighter");
        lo.gear.insert("weapon".into(), Equipped { item: "Longsword".into(), rarity: "Epic".into(), roll: 1.0, enchants: vec![] });
        let w = weapon_info(db, &lo, "weapon").unwrap();
        assert_eq!(w.base, 48.0);
        let s = compute_stats(db, &lo);
        assert_eq!(s.g("MoveSpeed"), -40.0);
        let tgt = compute_stats(db, &naked("Fighter"));
        let b = weapon_hit(db, &w, &s, &tgt, &w.attacks[0], 0, loc_index("Body"));
        // 48 × combo 1 × zone 1 × (1 + PPB) with 0 AR -> curve gives a small negative/zero DR
        assert!(b.total > 40.0 && b.total < 70.0, "{b:?}");
        let head = weapon_hit(db, &w, &s, &tgt, &w.attacks[0], 0, loc_index("Head"));
        assert!((head.total / b.total - 1.5).abs() < 0.01);
    }

    #[test]
    fn fireball_components() {
        let db = db();
        let lo = naked("Wizard");
        let atk = compute_stats(db, &lo);
        let tgt = compute_stats(db, &naked("Fighter"));
        let r = ability_damage(db, &lo, &atk, &tgt, "Fireball");
        let dot = r.iter().find(|c| c.kind == "dot").unwrap();
        assert_eq!(dot.ticks, Some(3));
        assert!(r.iter().any(|c| c.per_location.is_some()));
        assert!(r.iter().any(|c| c.flat.is_some()));
        // Fire Mastery swaps the dot for the stronger version
        let mut lo2 = lo.clone();
        lo2.perks.push("FireMastery".into());
        let r2 = ability_damage(db, &lo2, &atk, &tgt, "Fireball");
        let dot2 = r2.iter().find(|c| c.kind == "dot").unwrap();
        assert_eq!(dot2.id, "FireballRemain_FireMastery");
        assert_eq!(r2.iter().filter(|c| c.kind == "dot").count(), 1);
    }

    #[test]
    fn generator_quality_levels() {
        let db = db();
        let archs = archetypes();
        let a = archs.iter().find(|a| a.id == "fighter_pdr").unwrap();
        let timmy = generate(db, a, 0.0, 1);
        assert!(timmy.gear.is_empty());
        let normal = generate(db, a, 0.1, 1);
        let bis = generate(db, a, 1.0, 1);
        let bisbis = generate(db, a, 1.1, 1);
        let ar = |lo: &Loadout| compute_stats(db, lo).armor_rating;
        let hp = |lo: &Loadout| compute_stats(db, lo).max_health;
        assert!(ar(&normal) > 0.0);
        assert!(ar(&bis) > ar(&normal), "bis {} normal {}", ar(&bis), ar(&normal));
        assert!(ar(&bisbis) >= ar(&bis));
        assert!(hp(&bisbis) >= hp(&timmy));
        assert!(bis.gear.contains_key("weapon"));
        // deterministic
        assert_eq!(generate(db, a, 0.5, 42), generate(db, a, 0.5, 42));
        for arch in &archs {
            let lo = generate(db, arch, 0.7, 3);
            assert!(lo.gear.contains_key("weapon"), "{} got no weapon", arch.id);
        }
    }

    #[test]
    fn ttk_runs() {
        let db = db();
        let archs = archetypes();
        let atk = generate(db, archs.iter().find(|a| a.id == "fighter_dmg").unwrap(), 0.5, 5);
        let choice = AttackChoice::Weapon { slot: "weapon".into(), zone: 0 };
        let r = ttk(db, &atk, &naked("Wizard"), &choice, loc_index("Body")).unwrap();
        assert!(r.seconds.is_some() && r.hits >= 1);
        let t = tier_ttk(db, &archs, &atk, &choice, 1, "Low", 0.1, 22, 9);
        assert_eq!(t.samples, 22);
        assert!(t.median_seconds.is_some());

        let wiz = generate(db, archs.iter().find(|a| a.id == "wizard_fire").unwrap(), 0.5, 5);
        let fb = AttackChoice::Ability { id: "Fireball".into(), interval: 2.0, splash: true };
        let r = ttk(db, &wiz, &naked("Rogue"), &fb, 1).unwrap();
        assert!(r.seconds.is_some(), "{r:?}");
    }
}
