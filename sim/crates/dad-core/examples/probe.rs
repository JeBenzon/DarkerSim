use dad_core::{damage::*, gen::*, sim::*, stats::*, GameDb};
fn main() {
    let db = GameDb::from_json(&std::fs::read_to_string("../app/data/game.json").unwrap()).unwrap();
    let archs = archetypes_from_json(&std::fs::read_to_string("../data/archetypes.json").unwrap()).unwrap();
    let naked = |c: &str| Loadout { class: c.into(), ..Default::default() };
    let wiz = naked("Wizard");
    let (a, t) = (compute_stats(&db, &wiz), compute_stats(&db, &naked("Fighter")));
    println!("wiz MPB {:.3} fighter MR {:.1} MDR {:.3} hp {}", a.magical_power_bonus, t.magic_resistance, t.magical_reduction, t.max_health);
    for c in ability_damage(&db, &wiz, &a, &t, "Fireball") { println!("{:?}", c); }
    for arch in &archs { for q in [0.1, 0.5, 1.0, 1.1] {
        let lo = generate(&db, arch, q, 1); let s = compute_stats(&db, &lo);
        println!("{:16} q{q}: hp {:5.0} AR {:5.1} PDR {:5.3} PPB {:5.3} MPB {:5.3} AS {:5.3} MS {:5.1} | {}", arch.id, s.max_health, s.armor_rating, s.physical_reduction, s.physical_power_bonus, s.magical_power_bonus, s.action_speed, s.move_speed,
          lo.gear.iter().map(|(k,e)| format!("{k}={}:{}({})", e.item, &e.rarity[..2], e.enchants.len())).collect::<Vec<_>>().join(" "));
    }}
    let atk = generate(&db, &archs[1], 1.0, 1);
    let ch = AttackChoice::Weapon { slot: "weapon".into(), zone: 0 };
    for (n, q) in TIERS { println!("{:?}", tier_ttk(&db, &archs, &atk, &ch, 1, n, q, 55, 1)); }
}
