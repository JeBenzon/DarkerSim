//! Darker Sim: browser UI over dad-core (Leptos, client-side rendered, WebAssembly).

use dad_core::damage::{ability_damage, location_mult, spell_ttk, weapon_hit, weapon_info, weapon_ttk, HIT_LOCS};
use dad_core::data::{Archetype, Attack, GameDb};
use dad_core::gen::{archetypes_from_json, enchant_count, generate, TIERS};
use dad_core::sim::{tier_ttk, AttackChoice, TierResult};
use dad_core::stats::{all_slots, compute_stats, prop_value, Enchant, Equipped, Loadout, Stats};
use leptos::prelude::*;

mod companion;
mod controls;
mod gear;
mod buffs;
mod items;
mod link;
use items::{FloatingTip, GearIcon, ItemBrowser};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

// ------------------------------------------------------------------ globals (loaded once)

static DB: OnceLock<GameDb> = OnceLock::new();
static ARCHS: OnceLock<Vec<Archetype>> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn db() -> &'static GameDb {
    DB.get().expect("db loaded")
}
fn archs() -> &'static [Archetype] {
    ARCHS.get().map(|v| v.as_slice()).unwrap_or(&[])
}
fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// source type -> source id -> buff ids
fn buff_tree() -> &'static BTreeMap<String, BTreeMap<String, Vec<String>>> {
    static T: OnceLock<BTreeMap<String, BTreeMap<String, Vec<String>>>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t: BTreeMap<String, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        for (id, b) in &db().buffs {
            if b.mods.is_empty() {
                continue;
            }
            let srcs = if b.sources.is_empty() {
                vec![("Other".to_string(), "(unknown)".to_string())]
            } else {
                b.sources.iter().map(|s| (s.kind.clone(), s.id.clone())).collect()
            };
            for (k, s) in srcs {
                t.entry(k).or_default().entry(s).or_default().push(id.clone());
            }
        }
        for m in t.values_mut() {
            for v in m.values_mut() {
                v.sort();
            }
        }
        t
    })
}

/// item slot -> [(display name, family id)]
fn items_by_slot() -> &'static BTreeMap<String, Vec<(String, String)>> {
    static T: OnceLock<BTreeMap<String, Vec<(String, String)>>> = OnceLock::new();
    T.get_or_init(|| {
        let mut t: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for f in db().items.values() {
            let Some(slot) = &f.slot else { continue };
            if f.variants.values().all(|v| v.props.is_empty()) && slot != "Sash" {
                continue;
            }
            t.entry(slot.clone()).or_default().push((f.name.clone(), f.id.clone()));
        }
        for v in t.values_mut() {
            v.sort();
        }
        t
    })
}

// ------------------------------------------------------------------ storage

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}
fn load<T: serde::de::DeserializeOwned>(key: &str) -> Option<T> {
    serde_json::from_str(&storage()?.get_item(key).ok()??).ok()
}
fn save<T: serde::Serialize>(key: &str, v: &T) {
    if let (Some(s), Ok(j)) = (storage(), serde_json::to_string(v)) {
        let _ = s.set_item(key, &j);
    }
}

// ------------------------------------------------------------------ formatting

fn pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}
fn spct(v: f64) -> String {
    format!("{}{:.1}%", if v > 0.0 { "+" } else { "" }, v * 100.0)
}
fn n1(v: f64) -> String {
    format!("{v:.1}")
}
fn stat_value(stat: &str, internal: f64) -> String {
    if db().is_pct(stat) {
        spct(internal)
    } else {
        format!("{}{}", if internal > 0.0 { "+" } else { "" }, (internal * 100.0).round() / 100.0)
    }
}
fn item_name(id: &str) -> String {
    db().items.get(id).map(|f| f.name.clone()).unwrap_or_else(|| id.to_string())
}
fn quality_label(q: f64) -> &'static str {
    match q {
        q if q <= 0.0 => "Timmy (naked)",
        q if q < 0.25 => "Normal",
        q if q < 0.45 => "Decent",
        q if q < 0.75 => "Good",
        q if q < 1.0 => "Great",
        q if q <= 1.0 => "BIS",
        _ => "BISBIS (theoretical max)",
    }
}
fn ttk_text(secs: Option<f64>, hits: u32, unit: &str) -> String {
    match secs {
        Some(s) => format!("{s:.2}s · {hits} {unit}"),
        None => format!("survives ({hits} {unit} simulated)"),
    }
}

// ------------------------------------------------------------------ app

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let r = gloo_net::http::Request::get(url).send().await.map_err(|e| e.to_string())?;
    if !r.ok() {
        return Err(format!("{url}: HTTP {}", r.status()));
    }
    r.text().await.map_err(|e| e.to_string())
}

#[component]
fn App() -> impl IntoView {
    let state = RwSignal::new(None::<Result<(), String>>);
    leptos::task::spawn_local(async move {
        let res = async {
            let g = fetch_text("game.json").await?;
            let db = GameDb::from_json(&g).map_err(|e| format!("game.json: {e}"))?;
            let a = fetch_text("archetypes.json").await.unwrap_or_else(|_| "[]".into());
            let archs = archetypes_from_json(&a).map_err(|e| format!("archetypes.json: {e}"))?;
            let _ = DB.set(db);
            let _ = ARCHS.set(archs);
            Ok(())
        }
        .await;
        state.set(Some(res));
    });
    move || match state.get() {
        None => view! { <div class="loading">"Loading game data…"</div> }.into_any(),
        Some(Err(e)) => view! { <div class="loading bad">"Could not load data: " {e}</div> }.into_any(),
        Some(Ok(())) => view! { <Sim/> }.into_any(),
    }
}

/// Whether a class can normally use this item (DarkerLink still equips misfits, flagged in red).
pub fn gear_fits(fam: &dad_core::data::ItemFamily, class: &str) -> bool {
    fam.classes.is_empty() || fam.classes.iter().any(|c| c == class)
}

fn default_loadout(arch: &str, q: f64, seed: u64) -> Loadout {
    archs()
        .iter()
        .find(|a| a.id == arch)
        .map(|a| generate(db(), a, q, seed))
        .unwrap_or_else(|| Loadout { class: "Fighter".into(), ..Default::default() })
}

#[derive(Clone, PartialEq)]
struct Float {
    id: u64,
    text: String,
    kind: &'static str,
}

#[component]
fn Sim() -> impl IntoView {
    let atk = RwSignal::new(load::<Loadout>("dadsim.atk").unwrap_or_else(|| default_loadout("cleric_battle", 0.5, 1)));
    let tgt = RwSignal::new(load::<Loadout>("dadsim.tgt").unwrap_or_else(|| default_loadout("fighter_pdr", 0.5, 2)));
    let builds = RwSignal::new(load::<BTreeMap<String, Loadout>>("dadsim.builds").unwrap_or_default());
    Effect::new(move |_| { save("dadsim.atk", &atk.get()) });
    Effect::new(move |_| save("dadsim.tgt", &tgt.get()));
    Effect::new(move |_| save("dadsim.builds", &builds.get()));

    let atk_stats = Memo::new(move |_| { compute_stats(db(), &atk.get()) });
    let tgt_stats = Memo::new(move |_| compute_stats(db(), &tgt.get()));

    let hp = RwSignal::new(tgt_stats.get_untracked().max_health);
    Effect::new(move |_| { hp.set(tgt_stats.get().max_health); });

    items::provide_tip();
    buffs::provide_tip();
    let sel_atk = RwSignal::new(None::<&'static str>);
    let sel_tgt = RwSignal::new(None::<&'static str>);
    link::start(atk, tgt, sel_atk, sel_tgt);
    let tab = RwSignal::new(load::<String>("dadsim.tab").unwrap_or_else(|| "sim".into()));
    Effect::new(move |_| save("dadsim.tab", &tab.get()));
    let m = &db().meta;
    let info = format!("Game {} · data extracted {}", m.build, m.generated_at.get(..10).unwrap_or(""));
    view! {
        <header>
            <h1>"Darker Sim"</h1>
            <nav class="tabs">
                <button class=move || if tab.get() == "sim" { "on" } else { "" } on:click=move |_| tab.set("sim".into())>"Simulator"</button>
                <button class=move || if tab.get() == "items" { "on" } else { "" } on:click=move |_| tab.set("items".into())>"Items"</button>
                <button class=move || if tab.get() == "link" { "on" } else { "" } on:click=move |_| tab.set("link".into())>"DarkerLink"</button>
            </nav>
            <span class="muted">{info}</span>
            <span class="link-badge-wrap" on:click=move |_| tab.set("link".into())><link::LinkBadge/></span>
        </header>
        <FloatingTip/>
        <buffs::BuffFloatTip/>
        <div style:display=move || if tab.get() == "items" { "block" } else { "none" }>
            <ItemBrowser atk=atk tgt=tgt/>
        </div>
        <main class="grid" style:display=move || if tab.get() == "sim" { "grid" } else { "none" }>
            <Card title="Attacker" side="atk" lo=atk stats=atk_stats builds=builds hp=None selected=sel_atk/>
            <Combat atk=atk tgt=tgt atk_stats=atk_stats tgt_stats=tgt_stats hp=hp/>
            <Card title="Target" side="tgt" lo=tgt stats=tgt_stats builds=builds hp=Some(hp) selected=sel_tgt/>
        </main>
        <div style:display=move || if tab.get() == "link" { "block" } else { "none" }>
            <companion::CompanionPage/>
        </div>
        <SiteFooter/>
    }
}

// ------------------------------------------------------------------ footer

/// Fan-project notice, shown under every tab.
#[component]
fn SiteFooter() -> impl IntoView {
    view! {
        <footer class="site-footer">
            <div class="sf-rule"></div>
            <p class="sf-brand">"Darker Sim"</p>
            <p>
                "Darker Sim is an unofficial fan project, made for fun by a player for other players. It is not affiliated with, "
                "endorsed by or connected to IRONMACE, the developer of Dark and Darker."
            </p>
            <p>
                "Dark and Darker and everything that belongs to it — names, item and ability icons, game data and other game "
                "assets — is the property of IRONMACE. We claim no ownership of any of it, and all credit for the game goes to "
                "IRONMACE. Numbers shown here are calculated from game files and may be incomplete or wrong; for anything "
                "official, trust the game itself."
            </p>
            <p class="sf-tm">
                "IRONMACE™ and Dark and Darker™ are trademarks of IRONMACE in the U.S. and/or other countries. "
                "Certain assets and content used on this site are © IRONMACE. All rights reserved."
            </p>
        </footer>
    }
}

// ------------------------------------------------------------------ combat panel

#[derive(Clone, PartialEq)]
struct AtkOpt {
    value: String,
    name: String,
    group: &'static str,
    sub: String,
    icon: Option<String>,
    rarity: String,
}

/// Icon grid of everything the attacker can hit with, grouped like the in-game skill/spell bars.
#[component]
fn AttackPicker(options: Memo<Vec<AtkOpt>>, choice: RwSignal<String>, on_pick: Callback<String>) -> impl IntoView {
    let filter = RwSignal::new(String::new());
    let current = move || {
        let c = choice.get();
        options.get().into_iter().find(|o| o.value == c)
    };
    let tile = move |o: AtkOpt, big: bool| {
        let initials: String = o.name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
        let img = match o.icon.clone() {
            Some(u) => view! { <img src=u alt="" loading="lazy"/> }.into_any(),
            None => view! { <span class="ap-noicon">{initials}</span> }.into_any(),
        };
        let cls = format!("ap-icon g-{}{}{}", o.group.to_lowercase(), if o.rarity.is_empty() { String::new() } else { format!(" rc-{}", o.rarity) }, if big { " big" } else { "" });
        view! { <span class=cls>{img}</span> }
    };
    let open = RwSignal::new(false);
    let search_ref = NodeRef::<leptos::html::Input>::new();
    let close = move || { open.set(false); filter.set(String::new()); };
    // focus the search box as soon as the popover opens
    Effect::new(move |_| {
        if open.get() {
            if let Some(el) = search_ref.get() { let _ = el.focus(); }
        }
    });
    view! {
        <div class="atk-picker">
            <button class=move || if open.get() { "ap-current open" } else { "ap-current" } title="Change attack"
                on:click=move |_| if open.get_untracked() { close() } else { open.set(true) }>
                {move || current().map(|o| view! {
                    {tile(o.clone(), true)}
                    <div class="ap-cur-text">
                        <div class="ap-cur-name">{o.name.clone()}</div>
                        <div class="muted small">{format!("{} · {}", o.group, o.sub)}</div>
                    </div>
                })}
                <span class="ap-caret">{move || if open.get() { "▴" } else { "▾" }}</span>
            </button>
            <Show when=move || open.get()>
                <div class="ap-backdrop" on:click=move |_| close()></div>
                <div class="ap-pop" on:keydown=move |ev| if ev.key() == "Escape" { close() }>
                    <input class="ap-search" placeholder="search attacks…" node_ref=search_ref prop:value=move || filter.get()
                        on:input=move |ev| filter.set(event_target_value(&ev))/>
                    {move || {
                        let f = filter.get().to_lowercase();
                        let c = choice.get();
                        let all = options.get();
                        [("Weapon", "Weapons"), ("Skill", "Class skills"), ("Spell", "Class spells"), ("Other", "Item / unassigned abilities")].into_iter().filter_map(|(g, label)| {
                            let list: Vec<AtkOpt> = all.iter().filter(|o| o.group == g && (f.is_empty() || o.name.to_lowercase().contains(&f))).cloned().collect();
                            (!list.is_empty()).then(|| view! {
                                <div class="ap-group">
                                    <div class="ap-label">{label}</div>
                                    <div class="ap-grid">
                                        {list.into_iter().map(|o| {
                                            let sel = o.value == c;
                                            let v = o.value.clone();
                                            let title = format!("{} — {} · {}", o.name, o.group, o.sub);
                                            view! {
                                                <button class=if sel { "ap-tile sel" } else { "ap-tile" } title=title
                                                    on:click=move |_| { on_pick.run(v.clone()); close(); }>
                                                    {tile(o.clone(), false)}
                                                    <span class="ap-name">{o.name.clone()}</span>
                                                </button>
                                            }
                                        }).collect_view()}
                                    </div>
                                </div>
                            })
                        }).collect_view()
                    }}
                </div>
            </Show>
        </div>
    }
}

fn class_icon(id: &str) -> String {
    format!("icons/class/ClassIcon_L_{id}.webp")
}

/// Current class as a big emblem; click to open the class grid.
#[component]
fn ClassPicker(lo: RwSignal<Loadout>, class_ids: Vec<String>, on_pick: Callback<String>) -> impl IntoView {
    let open = RwSignal::new(false);
    let name = |c: &str| db().classes.get(c).map(|x| x.name.clone()).unwrap_or_else(|| c.to_string());
    view! {
        <div class="class-picker">
            <button class=move || if open.get() { "cp-current open" } else { "cp-current" } title="Change class" on:click=move |_| open.update(|o| *o = !*o)>
                {move || { let c = lo.get().class; view! {
                    <span class="cp-emblem"><img src=class_icon(&c) alt=""/></span>
                    <span class="cp-name">{name(&c)}</span>
                } }}
                <span class="ap-caret">{move || if open.get() { "▴" } else { "▾" }}</span>
            </button>
            <Show when=move || open.get()>
                <div class="ap-backdrop" on:click=move |_| open.set(false)></div>
                <div class="ap-pop cp-pop" on:keydown=move |ev| if ev.key() == "Escape" { open.set(false) }>
                    {
                        let ids = class_ids.clone();
                        move || {
                            let cur = lo.get().class;
                            ids.iter().map(|c| {
                                let c2 = c.clone();
                                view! {
                                    <button class=if *c == cur { "cp-tile sel" } else { "cp-tile" }
                                        on:click=move |_| { let c = c2.clone(); if lo.with_untracked(|l| l.class != c) { on_pick.run(c); } open.set(false); }>
                                        <span class="cp-emblem"><img src=class_icon(c) alt=""/></span>
                                        <span class="cp-tname">{name(c)}</span>
                                    </button>
                                }
                            }).collect_view()
                        }
                    }
                </div>
            </Show>
        </div>
    }
}

/// "+2 Agility · -1 Strength" for a skin's stat changes.
fn skin_mods(mods: &[dad_core::data::Mod]) -> Vec<(String, bool)> {
    mods.iter().map(|m| {
        let v = stat_value(&m.stat, m.value);
        let v = if v.starts_with('-') || v.starts_with('+') { v } else { format!("+{v}") };
        (format!("{v} {}", db().stat_name(&m.stat)), m.value < 0.0)
    }).collect()
}

/// Character skin: some skins change attributes (Felidian Panther: +2 Agility, -1 Strength, -1 Vigor).
#[component]
fn SkinPicker(lo: RwSignal<Loadout>) -> impl IntoView {
    let open = RwSignal::new(false);
    let pick = move |k: Option<String>| { lo.update(|l| l.skin = k); open.set(false); };
    view! {
        <div class="skin-picker">
            <button class=move || if open.get() { "sk-current open" } else { "sk-current" } title="Character skin — some skins change your stats"
                on:click=move |_| open.update(|o| *o = !*o)>
                <span class="sk-label">"Skin"</span>
                {move || match lo.with(|l| l.skin.clone()).and_then(|k| db().skins.get(&k).map(|s| (s.name.clone(), s.mods.clone(), s.icon.clone()))) {
                    Some((name, mods, icon)) => view! {
                        <span class="sk-face">{icon.map(|i| view! { <img src=format!("icons/{i}.webp") alt=""/> })}</span>
                        <span class="sk-name">{name}</span>
                        <span class="sk-mods">{skin_mods(&mods).into_iter().map(|(t, neg)| view! { <span class=if neg { "neg" } else { "pos" }>{t}</span> }).collect_view()}</span>
                    }.into_any(),
                    None => view! { <span class="sk-face empty">"—"</span><span class="sk-name muted">"Default (no stat changes)"</span> }.into_any(),
                }}
                <span class="ap-caret">{move || if open.get() { "▴" } else { "▾" }}</span>
            </button>
            <Show when=move || open.get()>
                <div class="ap-backdrop" on:click=move |_| open.set(false)></div>
                <div class="ap-pop sk-pop" on:keydown=move |ev| if ev.key() == "Escape" { open.set(false) }>
                    <div class="ap-label">"Character skin"</div>
                    {move || {
                        let cur = lo.with(|l| l.skin.clone());
                        let class = lo.with(|l| l.class.clone());
                        let mut list: Vec<(String, String, Vec<dad_core::data::Mod>, bool, Option<String>)> = db().skins.iter()
                            .filter(|(_, s)| s.classes.is_empty() || s.classes.iter().any(|c| *c == class))
                            .map(|(k, s)| (k.clone(), s.name.clone(), s.mods.clone(), s.confidence == "verified", s.icon.clone()))
                            .collect();
                        list.sort_by(|a, b| a.1.cmp(&b.1));
                        view! {
                            <button class=if cur.is_none() { "sk-tile sel" } else { "sk-tile" } on:click=move |_| pick(None)>
                                <span class="sk-face empty">"—"</span>
                                <span class="sk-text"><span class="sk-tname">"Default"</span><span class="sk-mods"><span class="muted">"no stat changes"</span></span></span>
                            </button>
                            {list.into_iter().map(|(k, name, mods, verified, icon)| {
                                let sel = cur.as_deref() == Some(k.as_str());
                                view! {
                                    <button class=if sel { "sk-tile sel" } else { "sk-tile" } on:click=move |_| pick(Some(k.clone()))
                                        title=if verified { "" } else { "Read from the game files, not yet checked in game" }>
                                        <span class="sk-face">{icon.map(|i| view! { <img src=format!("icons/{i}.webp") alt="" loading="lazy"/> })}</span>
                                        <span class="sk-text">
                                            <span class="sk-tname">{name}</span>
                                            <span class="sk-mods">{skin_mods(&mods).into_iter().map(|(t, neg)| view! { <span class=if neg { "neg" } else { "pos" }>{t}</span> }).collect_view()}</span>
                                        </span>
                                    </button>
                                }
                            }).collect_view()}
                        }
                    }}
                </div>
            </Show>
        </div>
    }
}

fn parse_choice(s: &str) -> Option<AttackChoice> {
    if let Some(slot) = s.strip_prefix("w:") {
        return Some(AttackChoice::Weapon { slot: slot.into(), zone: 0 });
    }
    s.strip_prefix("a:").map(|id| AttackChoice::Ability { id: id.into(), interval: 2.0, splash: true })
}

fn primary_combo(attacks: &[Attack]) -> Vec<Attack> {
    let p: Vec<Attack> = attacks.iter().filter(|a| a.group == "Primary").cloned().collect();
    if p.is_empty() { attacks.iter().take(1).cloned().collect() } else { p }
}

#[component]
fn Combat(
    atk: RwSignal<Loadout>,
    tgt: RwSignal<Loadout>,
    atk_stats: Memo<Stats>,
    tgt_stats: Memo<Stats>,
    hp: RwSignal<f64>,
) -> impl IntoView {
    let _ = tgt;
    let choice = RwSignal::new("w:weapon".to_string());
    let loc = RwSignal::new(1usize);
    let zone = RwSignal::new(0usize);
    let interval = RwSignal::new(2.0f64);
    let splash = RwSignal::new(true);
    let combo_i = RwSignal::new(0usize);
    let log = RwSignal::new(Vec::<(u64, String)>::new());
    let floats = RwSignal::new(Vec::<Float>::new());
    let tiers = RwSignal::new(Vec::<TierResult>::new());

    // available attacks for the attacker (weapons first, then skills, then spells)
    let options = Memo::new(move |_| {
        let lo = atk.get();
        let mut o: Vec<AtkOpt> = vec![];
        for slot in ["weapon", "offhand", "weapon2", "offhand2"] {
            if let Some(w) = weapon_info(db(), &lo, slot) {
                let icon = lo.gear.get(slot).and_then(|eq| db().items.get(&eq.item).and_then(|f| items::icon_for(f, &eq.rarity))).map(|i| items::icon_url(&i));
                let rarity = lo.gear.get(slot).map(|e| e.rarity.clone()).unwrap_or_default();
                o.push(AtkOpt {
                    value: format!("w:{slot}"),
                    name: w.name.clone(),
                    group: "Weapon",
                    sub: format!("Set {} · {} · {} attacks", if slot.ends_with('2') { 2 } else { 1 }, if slot.starts_with("weapon") { "main hand" } else { "off hand" }, w.attacks.len()),
                    icon,
                    rarity,
                });
            }
        }
        let mut ab: Vec<AtkOpt> = db()
            .damage_abilities
            .iter()
            .filter(|(_, a)| a.classes.is_empty() || a.classes.contains(&lo.class))
            .map(|(id, a)| {
                let mut types: Vec<String> = a.components.iter().map(|c| c.element.clone().unwrap_or_else(|| c.damage_type.clone())).collect();
                types.sort();
                types.dedup();
                let cd = db().spells.get(id).and_then(|s| s.cooldown);
                AtkOpt {
                    value: format!("a:{id}"),
                    name: a.name.clone(),
                    // abilities with no class in the data come from items (scrolls, staffs, granted effects) or are unassigned
                    group: if a.classes.is_empty() { "Other" } else if a.kind == "skill" { "Skill" } else { "Spell" },
                    sub: format!("{}{}", types.join(", "), cd.map(|c| format!(" · {c}s cooldown")).unwrap_or_default()),
                    icon: a.icon.as_ref().map(|i| format!("icons/ability/{i}.webp")),
                    rarity: String::new(),
                }
            })
            .collect();
        let rank = |g: &str| match g { "Skill" => 0, "Spell" => 1, _ => 2 };
        ab.sort_by(|a, b| (rank(a.group), &a.name).cmp(&(rank(b.group), &b.name)));
        o.extend(ab);
        o
    });
    // keep the choice valid when the attacker changes
    Effect::new(move |_| {
        let o = options.get();
        let c = choice.get_untracked();
        if !o.iter().any(|x| x.value == c) {
            if let Some(first) = o.first() {
                choice.set(first.value.clone());
            }
        }
    });

    let apply = move |dmg: f64, label: String, kind: &'static str| {
        let before = hp.get_untracked();
        let after = (before - dmg).max(0.0);
        hp.set(after);
        let id = next_id();
        let killed = before > 0.0 && after <= 0.0;
        log.update(|l| {
            l.insert(0, (id, format!("{label}: {:.1}{}", dmg, if killed { "  ☠ KILL" } else { "" })));
            l.truncate(80);
        });
        floats.update(|f| f.push(Float { id, text: format!("{dmg:.0}"), kind }));
        set_timeout(move || floats.update(|f| f.retain(|x| x.id != id)), Duration::from_millis(1100));
    };

    let attack_once = move || {
        let (a_lo, a, t) = (atk.get_untracked(), atk_stats.get_untracked(), tgt_stats.get_untracked());
        let l = loc.get_untracked();
        let lname = HIT_LOCS[l].0;
        match parse_choice(&choice.get_untracked()) {
            Some(AttackChoice::Weapon { slot, .. }) => {
                let Some(w) = weapon_info(db(), &a_lo, &slot) else { return };
                let combo = primary_combo(&w.attacks);
                let i = combo_i.get_untracked() % combo.len();
                let b = weapon_hit(db(), &w, &a, &t, &combo[i], zone.get_untracked(), l);
                apply(b.total, format!("{} · {} → {lname}", w.name, combo[i].name), if l == 0 { "crit" } else { "hit" });
                combo_i.set(i + 1);
            }
            Some(AttackChoice::Ability { id, .. }) => {
                for r in ability_damage(db(), &a_lo, &a, &t, &id) {
                    if let Some(p) = r.per_location {
                        apply(p[l], format!("{} → {lname}", r.id), if l == 0 { "crit" } else { "hit" });
                    }
                    if let Some(f) = r.flat {
                        if splash.get_untracked() {
                            apply(f, format!("{} (splash)", r.id), "hit");
                        }
                    }
                    if let (Some(pt), Some(n), Some(iv)) = (r.per_tick, r.ticks, r.tick_interval) {
                        for k in 1..=n {
                            let label = format!("{} tick {k}/{n}", r.id);
                            set_timeout(move || apply(pt, label, "dot"), Duration::from_secs_f64(iv * k as f64));
                        }
                    }
                }
            }
            None => {}
        }
    };

    let current_choice = move || {
        let mut c = parse_choice(&choice.get())?;
        match &mut c {
            AttackChoice::Weapon { zone: z, .. } => *z = zone.get(),
            AttackChoice::Ability { interval: iv, splash: sp, .. } => {
                *iv = interval.get();
                *sp = splash.get();
            }
        }
        Some(c)
    };

    // per-location preview for the selected attack
    let preview = Memo::new(move |_| {
        let (a_lo, a, t) = (atk.get(), atk_stats.get(), tgt_stats.get());
        let mut out = [0.0f64; 6];
        match current_choice() {
            Some(AttackChoice::Weapon { slot, zone: z }) => {
                if let Some(w) = weapon_info(db(), &a_lo, &slot) {
                    let combo = primary_combo(&w.attacks);
                    let atk_ = &combo[combo_i.get() % combo.len()];
                    for (i, o) in out.iter_mut().enumerate() {
                        *o = weapon_hit(db(), &w, &a, &t, atk_, z, i).total;
                    }
                }
            }
            Some(AttackChoice::Ability { id, splash: sp, .. }) => {
                for r in ability_damage(db(), &a_lo, &a, &t, &id) {
                    for (i, o) in out.iter_mut().enumerate() {
                        *o += r.per_location.map(|p| p[i]).unwrap_or(0.0) + if sp { r.flat.unwrap_or(0.0) } else { 0.0 };
                    }
                }
            }
            None => {}
        }
        out
    });

    let ttk_now = Memo::new(move |_| {
        let (a_lo, a, t) = (atk.get(), atk_stats.get(), tgt_stats.get());
        let l = loc.get();
        match current_choice()? {
            AttackChoice::Weapon { slot, zone: z } => {
                let w = weapon_info(db(), &a_lo, &slot)?;
                let r = weapon_ttk(db(), &w, &a, &t, z, l);
                Some((ttk_text(r.seconds, r.hits, "hits"), r.assumed_timing))
            }
            AttackChoice::Ability { id, interval: iv, splash: sp } => {
                let rs = ability_damage(db(), &a_lo, &a, &t, &id);
                let r = spell_ttk(&rs, t.max_health, l, iv, sp);
                Some((ttk_text(r.seconds, r.hits, "casts"), false))
            }
        }
    });

    let simulate = move || {
        let Some(c) = current_choice() else { return };
        let (a_lo, a, t) = (atk.get_untracked(), atk_stats.get_untracked(), tgt_stats.get_untracked());
        let l = loc.get_untracked();
        let r = match c {
            AttackChoice::Weapon { slot, zone: z } => {
                weapon_info(db(), &a_lo, &slot).map(|w| weapon_ttk(db(), &w, &a, &t, z, l))
            }
            AttackChoice::Ability { id, interval: iv, splash: sp } => {
                Some(spell_ttk(&ability_damage(db(), &a_lo, &a, &t, &id), t.max_health, l, iv, sp))
            }
        };
        let Some(r) = r else { return };
        hp.set(t.max_health);
        log.update(|v| v.insert(0, (next_id(), format!("— simulate to death ({}) —", HIT_LOCS[l].0))));
        // replay at 4× speed
        for e in r.events {
            let label = format!("[{:.2}s] {}", e.t, e.label);
            set_timeout(move || apply(e.damage, label, "hit"), Duration::from_secs_f64(e.t / 4.0));
        }
    };

    let run_tiers = move || {
        let Some(c) = current_choice() else { return };
        let a_lo = atk.get_untracked();
        let l = loc.get_untracked();
        let res: Vec<TierResult> = TIERS
            .iter()
            .enumerate()
            .map(|(i, (name, q))| tier_ttk(db(), archs(), &a_lo, &c, l, name, *q, 66, 1000 + i as u64))
            .collect();
        tiers.set(res);
    };

    let detail = move || {
        let (a_lo, a, t) = (atk.get(), atk_stats.get(), tgt_stats.get());
        let hp_max = t.max_health;
        match current_choice() {
            Some(AttackChoice::Weapon { slot, zone: z }) => {
                let Some(w) = weapon_info(db(), &a_lo, &slot) else { return view! { <p class="muted">"No weapon."</p> }.into_any() };
                let rows = w
                    .attacks
                    .iter()
                    .map(|at| {
                        let cells = (0..6)
                            .map(|i| {
                                let d = weapon_hit(db(), &w, &a, &t, at, z, i).total;
                                let htk = if d > 0.0 { (hp_max / d).ceil() as u32 } else { 0 };
                                view! { <td class="num">{n1(d)}<span class="htk">{format!("{htk}×")}</span></td> }
                            })
                            .collect_view();
                        let sub = format!(
                            "{} {}",
                            at.damage_type.clone().unwrap_or_default(),
                            if at.combo != 1.0 { format!("· combo {:.0}%", at.combo * 100.0) } else { String::new() }
                        );
                        view! { <tr><td>{at.name.clone()}<span class="muted small">" " {sub}</span></td>{cells}</tr> }
                    })
                    .collect_view();
                let b = weapon_hit(db(), &w, &a, &t, &w.attacks[0], z, 1);
                let explain = format!(
                    "base {} · power bonus {} · +{} weapon dmg · +{} additional · target DR {} (pen {}) → ×{:.3}{} · +{} true",
                    n1(b.base), spct(b.power_bonus), n1(b.gear_weapon), n1(b.additional), pct(b.dr), pct(b.pen), b.dr_mult,
                    if w.projectile { format!(" · projectile ×{:.2}", b.projectile_mult) } else { String::new() },
                    n1(b.true_dmg)
                );
                view! {
                    <table class="dmg">
                        <tr><th>"Attack"</th>{HIT_LOCS.iter().enumerate().map(|(i, (n, _))| view! { <th class="num">{*n}<br/><span class="muted small">{format!("×{:.2}", location_mult(db(), i, &a, &t))}</span></th> }).collect_view()}</tr>
                        {rows}
                    </table>
                    <p class="muted small">"Cell = damage · hits to kill. " {explain}</p>
                }
                .into_any()
            }
            Some(AttackChoice::Ability { id, interval: iv, splash: sp }) => {
                let rs = ability_damage(db(), &a_lo, &a, &t, &id);
                let rows = rs
                    .iter()
                    .map(|r| {
                        let body = if let Some(p) = r.per_location {
                            p.iter().map(|d| view! { <td class="num">{n1(*d)}</td> }).collect_view().into_any()
                        } else if let Some(f) = r.flat {
                            view! { <td class="num" colspan="6">{format!("{} splash (no hit location)", n1(f))}</td> }.into_any()
                        } else {
                            let txt = format!(
                                "{} per tick × {} ticks (every {}s) = {} total",
                                n1(r.per_tick.unwrap_or(0.0)), r.ticks.unwrap_or(0), r.tick_interval.unwrap_or(1.0), n1(r.dot_total.unwrap_or(0.0))
                            );
                            view! { <td class="num" colspan="6">{txt}</td> }.into_any()
                        };
                        view! { <tr><td>{r.id.clone()}<span class="muted small">" " {r.kind.clone()}</span></td>{body}</tr> }
                    })
                    .collect_view();
                let per_cast: f64 = rs.iter().map(|r| r.per_location.map(|p| p[1]).unwrap_or(0.0) + if sp { r.flat.unwrap_or(0.0) } else { 0.0 } + r.dot_total.unwrap_or(0.0)).sum();
                let gated: Vec<String> = db().damage_abilities.get(&id).map(|ab| ab.components.iter().filter_map(|c| c.requires_perk.clone()).collect()).unwrap_or_default();
                view! {
                    <table class="dmg">
                        <tr><th>"Component"</th>{HIT_LOCS.iter().map(|(n, _)| view! { <th class="num">{*n}</th> }).collect_view()}</tr>
                        {rows}
                    </table>
                    <p class="muted small">{format!("Body-hit cast incl. full DoT: {} · target {} HP · recast every {iv}s", n1(per_cast), hp_max)}
                        {(!gated.is_empty()).then(|| format!(" · perk-gated variants: {} (toggle the perk on the attacker)", gated.join(", ")))}</p>
                }
                .into_any()
            }
            None => view! { <p class="muted">"Pick an attack."</p> }.into_any(),
        }
    };

    let is_ability = move || choice.get().starts_with("a:");
    let max_zone = move || {
        weapon_info(db(), &atk.get(), choice.get().strip_prefix("w:").unwrap_or("weapon"))
            .map(|w| w.attacks.iter().map(|a| a.zones.len()).max().unwrap_or(1))
            .unwrap_or(1)
    };

    view! {
        <section class="card combat">
            <h2>"Combat"</h2>
            <AttackPicker options choice on_pick=Callback::new(move |v: String| {
                if let Some(id) = v.strip_prefix("a:") {
                    let cd = db().spells.get(id).and_then(|s| s.cooldown).unwrap_or(1.0);
                    interval.set(cd.max(0.5));
                }
                // a weapon of the other set: swap sets, like pressing weapon swap in game
                if let Some(slot) = v.strip_prefix("w:") {
                    let set = u8::from(slot.ends_with('2'));
                    if atk.with_untracked(|l| l.weapon_set) != set {
                        atk.update(|l| l.weapon_set = set);
                    }
                }
                combo_i.set(0);
                choice.set(v);
            })/>
            <div class="row">
                {move || (!is_ability() && max_zone() > 1).then(|| view! {
                    <select on:change=move |ev| zone.set(event_target_value(&ev).parse().unwrap_or(0))>
                        {(0..max_zone()).map(|i| view! { <option value=i.to_string() selected=zone.get_untracked() == i>{if i == 0 { "Sweet spot".to_string() } else { format!("Zone {}", i + 1) }}</option> }).collect_view()}
                    </select>
                })}
                {move || is_ability().then(|| view! {
                    <label class="small">"recast every "
                        <input type="number" step="0.1" min="0.1" class="w4" prop:value=move || interval.get()
                            on:change=move |ev| interval.set(event_target_value(&ev).parse().unwrap_or(2.0))/>"s"</label>
                    <label class="small"><input type="checkbox" prop:checked=move || splash.get() on:change=move |ev| splash.set(event_target_checked(&ev))/>" splash hits too"</label>
                })}
            </div>

            <div class="arena">
                <Body loc=loc preview=preview/>
                <div class="locs">
                    {move || {
                        let p = preview.get();
                        let l = loc.get();
                        HIT_LOCS.iter().enumerate().map(|(i, (n, _))| view! {
                            <button class=if i == l { "loc sel" } else { "loc" } on:click=move |_| loc.set(i)>
                                <span>{*n}</span><b>{n1(p[i])}</b>
                            </button>
                        }).collect_view()
                    }}
                </div>
            </div>

            <div class="hpwrap">
                <div class="hpbar"><div class="hpfill" style:width=move || format!("{:.1}%", (hp.get() / tgt_stats.get().max_health.max(1.0) * 100.0).clamp(0.0, 100.0))></div>
                </div>
                <span class="hptext">{move || format!("{:.1} / {} HP", hp.get(), tgt_stats.get().max_health)}</span>
                <div class="floats">
                    <For each=move || floats.get() key=|f| f.id children=move |f| view! { <span class=format!("float {}", f.kind)>{f.text}</span> }/>
                </div>
            </div>

            <div class="row">
                <button class="big" on:click=move |_| attack_once()>"⚔ Attack"</button>
                <button on:click=move |_| simulate()>"Simulate to death"</button>
                <button class="ghost" on:click=move |_| { hp.set(tgt_stats.get_untracked().max_health); combo_i.set(0); log.set(vec![]); }>"Reset"</button>
                <span class="ttk">"TTK " {move || ttk_now.get().map(|(t, assumed)| format!("{t}{}", if assumed { " (no swing timing data: 1s assumed)" } else { "" })).unwrap_or_default()}</span>
            </div>

            <div class="detail">{detail}</div>

            <h3>"Median TTK vs random targets"</h3>
            <p class="muted small">"66 generated targets per tier across all archetypes, hitting the selected location."</p>
            <button on:click=move |_| run_tiers()>"Run tiers"</button>
            {move || {
                let t = tiers.get();
                (!t.is_empty()).then(|| view! {
                    <table class="dmg">
                        <tr><th>"Tier"</th><th class="num">"median HP"</th><th class="num">"median hits"</th><th class="num">"median TTK"</th><th class="num">"p10–p90"</th><th class="num">"survive"</th></tr>
                        {t.into_iter().map(|r| view! {
                            <tr><td>{r.tier.clone()}</td><td class="num">{format!("{:.0}", r.median_hp)}</td><td class="num">{format!("{:.0}", r.median_hits)}</td>
                            <td class="num">{r.median_seconds.map(|s| format!("{s:.2}s")).unwrap_or("—".into())}</td>
                            <td class="num">{match (r.p10_seconds, r.p90_seconds) { (Some(a), Some(b)) => format!("{a:.2}–{b:.2}s"), _ => "—".into() }}</td>
                            <td class="num">{pct(r.survived)}</td></tr>
                        }).collect_view()}
                    </table>
                })
            }}

            <h3>"Combat log"</h3>
            <div class="log">
                <For each=move || log.get() key=|e| e.0 children=move |e| view! { <div>{e.1}</div> }/>
            </div>
        </section>
    }
}

/// Hit-location regions of a traced hooded adventurer (front view, viewBox 200×400).
/// Right-side parts are the left-side paths mirrored. (index into HIT_LOCS, path, mirrored).
/// Drawn in this order, so later regions sit on top (the tabard covers the tops of the legs).
const BODY_ZONES: &[(usize, &str, bool)] = &[
    // leg: cuisse, poleyn, greave to the ankle — wide stance
    (4, "M70,194 L98,198 L96,250 C97,258 96,266 92,274 L84,344 L64,344 L66,274 C62,266 62,256 66,248 Z", false),
    (4, "M70,194 L98,198 L96,250 C97,258 96,266 92,274 L84,344 L64,344 L66,274 C62,266 62,256 66,248 Z", true),
    // foot: boot
    (5, "M63,343 L85,343 L86,358 C92,362 94,370 92,378 L54,378 C53,370 57,362 62,358 Z", false),
    (5, "M63,343 L85,343 L86,358 C92,362 94,370 92,378 L54,378 C53,370 57,362 62,358 Z", true),
    // torso: gambeson / breastplate, belt and tabard skirt
    (1, "M84,80 L116,80 L128,86 L136,92 L138,128 L132,156 L136,160 L140,206 L118,214 L100,204 L82,214 L60,206 L64,160 L68,156 L62,128 L64,92 L72,86 Z", false),
    // arm: pauldron, upper arm, couter, bracer to the wrist
    (2, "M66,86 C52,82 40,88 36,100 C33,108 36,116 42,118 L38,142 C34,150 33,156 34,162 L26,194 L42,199 L50,162 C55,154 57,148 56,142 L62,118 Z", false),
    (2, "M66,86 C52,82 40,88 36,100 C33,108 36,116 42,118 L38,142 C34,150 33,156 34,162 L26,194 L42,199 L50,162 C55,154 57,148 56,142 L62,118 Z", true),
    // hand: glove
    (3, "M25,193 L43,198 C46,204 46,212 44,218 C42,226 36,230 30,228 C23,226 20,218 21,210 C22,204 23,198 25,193 Z", false),
    (3, "M25,193 L43,198 C46,204 46,212 44,218 C42,226 36,230 30,228 C23,226 20,218 21,210 C22,204 23,198 25,193 Z", true),
    // head: peaked hood draping onto the shoulders
    (0, "M100,8 C88,12 78,24 76,40 C74,54 76,66 70,78 L86,82 L100,84 L114,82 L130,78 C124,66 126,54 124,40 C122,24 112,12 100,8 Z", false),
];

/// Cloak hanging behind the figure (decoration, not a hit region).
const BODY_CLOAK: &str = "M68,88 C56,120 50,200 42,302 C60,314 80,306 100,312 C120,306 140,314 158,302 C150,200 144,120 132,88 Z";
/// The shadowed face inside the hood.
const BODY_FACE: &str = "M100,30 C90,30 85,40 85,52 C85,62 92,70 100,72 C108,70 115,62 115,52 C115,40 110,30 100,30 Z";

/// Seams and plate edges drawn over the regions (no hit area); each is also drawn mirrored.
const BODY_DETAIL: &[&str] = &[
    "M100,8 L100,30", "M80,40 C84,30 90,24 100,22",
    "M84,82 L100,98", "M100,98 L100,148", "M70,110 C86,120 100,120 100,120", "M64,96 C70,94 74,90 76,86",
    "M66,150 L100,150", "M67,160 L100,160", "M95,148 L95,162 L100,162", "M100,162 L100,204", "M80,166 L75,208", "M68,170 L64,204",
    "M42,118 C47,122 56,122 62,118", "M37,104 C45,98 56,96 65,98", "M36,152 C42,148 50,148 56,150", "M28,184 L45,189",
    "M23,210 L45,212",
    "M64,252 C72,246 88,246 96,252", "M65,268 C74,274 86,274 93,268", "M80,276 L75,342",
    "M62,360 L86,360", "M56,370 L90,370",
];

#[component]
fn Body(loc: RwSignal<usize>, preview: Memo<[f64; 6]>) -> impl IntoView {
    let cls = move |i: usize| if loc.get() == i { "zone sel" } else { "zone" };
    let tip = move |i: usize| format!("{}: {:.1}", HIT_LOCS[i].0, preview.get()[i]);
    let mirror = |m: bool| if m { "translate(200,0) scale(-1,1)" } else { "" };
    view! {
        <svg class="body" viewBox="0 0 200 400">
            <defs>
                <radialGradient id="body-shade" cx="50%" cy="35%" r="70%">
                    <stop offset="0%" stop-color="#ffffff" stop-opacity="0.10"/>
                    <stop offset="100%" stop-color="#000000" stop-opacity="0.35"/>
                </radialGradient>
                <filter id="body-glow" x="-30%" y="-30%" width="160%" height="160%">
                    <feGaussianBlur stdDeviation="3" result="b"/>
                    <feMerge><feMergeNode in="b"/><feMergeNode in="SourceGraphic"/></feMerge>
                </filter>
            </defs>
            <ellipse cx="100" cy="382" rx="66" ry="7" class="body-ground"/>
            <path class="body-cloak" d=BODY_CLOAK/>
            {BODY_ZONES.iter().map(|&(i, d, m)| view! {
                <path d=d transform=mirror(m) class=move || cls(i) on:click=move |_| loc.set(i)><title>{move || tip(i)}</title></path>
            }).collect_view()}
            <path class="body-face" d=BODY_FACE/>
            <g class="body-detail">
                {BODY_DETAIL.iter().map(|d| view! { <path d=*d/><path d=*d transform="translate(200,0) scale(-1,1)"/> }).collect_view()}
            </g>
            <g class="body-shade">
                {BODY_ZONES.iter().map(|&(_, d, m)| view! { <path d=d transform=mirror(m) fill="url(#body-shade)"/> }).collect_view()}
            </g>
        </svg>
    }
}

// ------------------------------------------------------------------ character card

#[component]
fn Card(
    title: &'static str,
    side: &'static str,
    lo: RwSignal<Loadout>,
    stats: Memo<Stats>,
    builds: RwSignal<BTreeMap<String, Loadout>>,
    hp: Option<RwSignal<f64>>,
    /// equipment slot clicked in the doll (DarkerLink puts captured items there)
    selected: RwSignal<Option<&'static str>>,
) -> impl IntoView {
    let _ = hp;
    let mut class_ids: Vec<String> = db().classes.keys().cloned().collect();
    class_ids.sort();

    // generator state
    let arch_id = RwSignal::new(
        archs().iter().find(|a| a.cls == lo.get_untracked().class).map(|a| a.id.clone()).unwrap_or_default(),
    );
    Effect::new(move |_| {
        let cls = lo.with(|l| l.class.clone());
        let cur = arch_id.get_untracked();
        if !archs().iter().any(|a| a.id == cur && a.cls == cls) {
            arch_id.set(archs().iter().find(|a| a.cls == cls).map(|a| a.id.clone()).unwrap_or_default());
        }
    });
    let quality = RwSignal::new(0.5f64);
    let seed = RwSignal::new(1u64);
    let gen = move |new_seed: bool| {
        if new_seed {
            seed.set(next_id().wrapping_mul(2654435761) ^ (js_sys_now() as u64));
        }
        let id = arch_id.get_untracked();
        let cls = lo.with_untracked(|l| l.class.clone());
        if let Some(a) = archs().iter().find(|a| a.id == id && a.cls == cls) {
            let keep_perks = lo.get_untracked();
            let mut new = generate(db(), a, quality.get_untracked(), seed.get_untracked());
            if keep_perks.class == new.class {
                new.perks = keep_perks.perks;
            }
            new.skin = keep_perks.skin; // the skin is the character's look, not part of the gear roll
            lo.set(new);
        }
    };

    let build_name = RwSignal::new(String::new());
    let all_items = RwSignal::new(false);
    let vkey = format!("dadsim.view.{side}");
    let view_mode = RwSignal::new(load::<String>(&vkey).unwrap_or_else(|| "doll".into()));
    Effect::new(move |_| save(&vkey, &view_mode.get()));

    view! {
        <section class=format!("card {side}")>
            <h2>{title}</h2>
            <div class="row">
                <ClassPicker lo=lo class_ids=class_ids.clone() on_pick=Callback::new(move |c: String| {
                    // new class: switch, then roll a fresh loadout from that class's first build
                    lo.update(|l| { l.class = c.clone(); l.perks.clear(); });
                    if let Some(a) = archs().iter().find(|a| a.cls == c) {
                        arch_id.set(a.id.clone());
                        gen(true);
                    }
                })/>
                <SkinPicker lo=lo/>
                <label class="small"><input type="checkbox" prop:checked=move || all_items.get() on:change=move |ev| all_items.set(event_target_checked(&ev))/>" all items"</label>
            </div>

            <details class="box" open=true>
                <summary>"Gear generator"</summary>
                <div class="row">
                    {move || {
                        let cls = lo.get().class;
                        let cur = arch_id.get();
                        let list: Vec<&Archetype> = archs().iter().filter(|a| a.cls == cls).collect();
                        if list.is_empty() {
                            return view! { <span class="muted small">{format!("No {cls} archetype yet (add one to data/archetypes.json).")}</span> }.into_any();
                        }
                        list.into_iter().map(|a| {
                            let id = a.id.clone();
                            view! {
                                <button class=if a.id == cur { "arch on" } else { "arch" } title=a.desc.clone() on:click=move |_| arch_id.set(id.clone())>
                                    {a.name.clone()}
                                </button>
                            }
                        }).collect_view().into_any()
                    }}
                </div>
                <div class="row">
                    <input type="range" min="0" max="1.1" step="0.05" class="grow" prop:value=move || quality.get()
                        on:input=move |ev| quality.set(event_target_value(&ev).parse().unwrap_or(0.5))/>
                    <span class="qlabel">{move || format!("{:.2} · {}", quality.get(), quality_label(quality.get()))}</span>
                </div>
                <div class="row">
                    <button on:click=move |_| gen(false)>"Generate"</button>
                    <button on:click=move |_| gen(true)>"🎲 Randomize"</button>
                    <button class="ghost" on:click=move |_| lo.update(|l| { l.gear.clear(); l.buffs.clear(); })>"Strip"</button>
                </div>
            </details>

            <details class="box">
                <summary>"Saved builds"</summary>
                <div class="row">
                    <input placeholder="name, e.g. BIS PDR Fighter" class="grow" prop:value=move || build_name.get() on:input=move |ev| build_name.set(event_target_value(&ev))/>
                    <button on:click=move |_| { let n = build_name.get_untracked(); if !n.trim().is_empty() { builds.update(|b| { b.insert(n.trim().to_string(), lo.get_untracked()); }); } }>"Save"</button>
                </div>
                <div class="builds">
                    {move || builds.get().into_iter().map(|(name, l)| {
                        let n2 = name.clone();
                        view! { <span class="pill">
                            <a on:click=move |_| lo.set(l.clone())>{format!("{name} ({})", builds.with_untracked(|b| b.get(&name).map(|x| x.class.clone()).unwrap_or_default()))}</a>
                            <button class="x" on:click=move |_| builds.update(|b| { b.remove(&n2); })>"✕"</button>
                        </span> }
                    }).collect_view()}
                </div>
            </details>

            <div class="viewtoggle">
                <button class=move || if view_mode.get() == "doll" { "on" } else { "" } on:click=move |_| view_mode.set("doll".into())>"Equipment"</button>
                <button class=move || if view_mode.get() == "list" { "on" } else { "" } on:click=move |_| view_mode.set("list".into())>"List"</button>
            </div>
            <div style:display=move || if view_mode.get() == "doll" { "block" } else { "none" }>
                <gear::PaperDoll lo=lo selected=selected side=side all_items=all_items/>
            </div>
            <div class="gear" style:display=move || if view_mode.get() == "list" { "flex" } else { "none" }>
                {all_slots().map(|(key, label, slot)| view! { <GearRow lo=lo slot_key=key label=label item_slot=slot side=side all_items=all_items/> }).collect_view()}
            </div>

            <buffs::BuffPanel lo=lo/>
            <PerkPanel lo=lo/>
            <StatSheet stats=stats/>
        </section>
    }
}

fn js_sys_now() -> f64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now() * 1000.0).unwrap_or(0.0)
}

#[component]
fn GearRow(
    lo: RwSignal<Loadout>,
    slot_key: &'static str,
    label: &'static str,
    item_slot: &'static str,
    side: &'static str,
    all_items: RwSignal<bool>,
) -> impl IntoView {
    let (key, slot) = (slot_key, item_slot);
    let eq = Memo::new(move |_| lo.with(|l| l.gear.get(key).cloned()));
    let list_id = format!("dl-{side}-{key}");
    let options = move || {
        let cls = lo.with(|l| l.class.clone());
        let all = all_items.get();
        items_by_slot()
            .get(slot)
            .map(|v| {
                v.iter()
                    .filter(|(_, id)| all || db().items.get(id).is_some_and(|f| f.variants.keys().any(|r| db().class_can_use(f, r, &cls))))
                    .map(|(n, _)| view! { <option value=n.clone()></option> })
                    .collect_view()
            })
    };
    let set_item = move |name: String| {
        let name = name.trim().to_string();
        if name.is_empty() {
            lo.update(|l| {
                l.gear.remove(key);
            });
            return;
        }
        let found = items_by_slot().get(slot).and_then(|v| {
            v.iter().find(|(n, id)| n.eq_ignore_ascii_case(&name) || id.eq_ignore_ascii_case(&name)).map(|x| x.1.clone())
        });
        if let Some(id) = found {
            let fam = &db().items[&id];
            let rs: Vec<String> = db().variants_ordered(fam).into_iter().map(|(r, _)| r.to_string()).collect();
            let prev = lo.with_untracked(|l| l.gear.get(key).map(|e| e.rarity.clone()));
            let rarity = prev
                .filter(|p| rs.contains(p))
                .or_else(|| rs.iter().find(|r| *r == "Epic").cloned())
                .or_else(|| rs.last().cloned())
                .unwrap_or_default();
            lo.update(|l| {
                l.gear.insert(key.to_string(), Equipped { item: id, rarity, roll: 1.0, enchants: vec![] });
            });
        }
    };
    let update_eq = move |f: Box<dyn FnOnce(&mut Equipped)>| {
        lo.update(|l| {
            if let Some(e) = l.gear.get_mut(key) {
                f(e)
            }
        })
    };

    view! {
        <div class="slot">
            <div class="slot-head">
                <span class="slot-name">{label}</span>
                {move || eq.get().map(|e| view! { <GearIcon eq=e/> })}
                <input class="grow" list=list_id.clone() placeholder="— empty — (type to search)"
                    prop:value=move || eq.get().map(|e| item_name(&e.item)).unwrap_or_default()
                    on:change=move |ev| set_item(event_target_value(&ev))/>
                <datalist id=list_id>{options}</datalist>
                {move || eq.get().map(|e| {
                    let fam = &db().items[&e.item];
                    let rs: Vec<String> = db().variants_ordered(fam).into_iter().map(|(r, _)| r.to_string()).collect();
                    view! {
                        <select class=format!("rar r-{}", e.rarity) on:change=move |ev| { let v = event_target_value(&ev); update_eq(Box::new(move |e| e.rarity = v)); }>
                            {rs.into_iter().map(|r| { let s = r == e.rarity; view! { <option class=format!("r-{r}") value=r.clone() selected=s>{r.clone()}</option> } }).collect_view()}
                        </select>
                        <button class="x" title="Unequip" on:click=move |_| lo.update(|l| { l.gear.remove(key); })>"✕"</button>
                    }
                })}
            </div>
            {move || eq.get().map(|e| {
                let fam = &db().items[&e.item];
                let props = fam.variants.get(&e.rarity).map(|v| v.props.iter().map(|p| {
                    let val = prop_value(db(), p, e.roll);
                    format!("{} {}", db().stat_name(&p.stat), stat_value(&p.stat, val))
                }).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
                let pool = fam.secondary_pool.as_ref().and_then(|k| db().secondary_pools.get(k)).cloned().unwrap_or_default();
                let max_n = enchant_count(db(), fam, &e.rarity) as usize;
                let have: Vec<String> = e.enchants.iter().map(|x| x.stat.clone()).collect();
                let can_add = e.enchants.len() < max_n.max(1) && !pool.is_empty();
                let pool2 = pool.clone();
                view! {
                    <div class="props">
                        <span>{props}</span>
                        <label class="small roll" title="base stat roll">"roll "
                            <input type="range" min="0" max="1" step="0.05" prop:value=e.roll
                                on:change=move |ev| { let v: f64 = event_target_value(&ev).parse().unwrap_or(1.0); update_eq(Box::new(move |e| e.roll = v)); }/>
                        </label>
                    </div>
                    <div class="enchants">
                        {e.enchants.iter().enumerate().map(|(i, en)| {
                            let p = pool.iter().find(|p| p.stat == en.stat);
                            let range = p.map(|p| format!("{}–{}", p.min, p.max)).unwrap_or_default();
                            let txt = format!("{} {}", db().stat_name(&en.stat), stat_value(&en.stat, en.raw * db().ratio(&en.stat)));
                            view! { <span class="pill ench" title=format!("raw value, pool {range}")>
                                {txt}
                                <input type="number" class="w4" prop:value=en.raw on:change=move |ev| { let v: f64 = event_target_value(&ev).parse().unwrap_or(0.0); update_eq(Box::new(move |e| if let Some(x) = e.enchants.get_mut(i) { x.raw = v })); }/>
                                <button class="x" on:click=move |_| update_eq(Box::new(move |e| { e.enchants.remove(i); }))>"✕"</button>
                            </span> }
                        }).collect_view()}
                        {can_add.then(|| view! {
                            <select class="add-ench" on:change=move |ev| {
                                let st = event_target_value(&ev);
                                if let Some(p) = pool2.iter().find(|p| p.stat == st) {
                                    let raw = p.max;
                                    update_eq(Box::new(move |e| e.enchants.push(Enchant { stat: st, raw })));
                                }
                            }>
                                <option value="" selected=true>{format!("+ enchant ({}/{})", have.len(), max_n)}</option>
                                {pool.iter().filter(|p| !have.contains(&p.stat)).map(|p| view! { <option value=p.stat.clone()>{db().stat_name(&p.stat)}</option> }).collect_view()}
                            </select>
                        })}
                    </div>
                }
            })}
        </div>
    }
}

fn mods_text(mods: &[dad_core::data::Mod]) -> String {
    mods.iter().map(|m| format!("{} {}", db().stat_name(&m.stat), stat_value(&m.stat, m.value))).collect::<Vec<_>>().join(", ")
}

#[component]
fn PerkPanel(lo: RwSignal<Loadout>) -> impl IntoView {
    let gated: Vec<String> = db()
        .damage_abilities
        .values()
        .flat_map(|a| a.components.iter().filter_map(|c| c.requires_perk.clone()))
        .collect();
    view! {
        <details class="box">
            <summary>"Perks"</summary>
            <p class="muted small">"Toggling a perk adds its passive buff (if the data has one) and unlocks perk-gated spell versions (★)."</p>
            <div class="pills">
                {move || {
                    let l = lo.get();
                    let perks = db().classes.get(&l.class).map(|c| c.perks.clone()).unwrap_or_default();
                    perks.into_iter().map(|p| {
                        let on = l.perks.contains(&p);
                        let star = gated.contains(&p);
                        let name = db().perks.get(&p).map(|x| x.name.clone()).unwrap_or_else(|| dad_core::humanize(&p));
                        let buffs: Vec<String> = buff_tree().get("Perk").and_then(|m| m.get(&p)).cloned().unwrap_or_default();
                        let tip = buffs.iter().filter_map(|b| db().buffs.get(b)).map(|b| mods_text(&b.mods)).collect::<Vec<_>>().join(" | ");
                        let p2 = p.clone();
                        view! {
                            <button class=if on { "chip on" } else { "chip" } title=if tip.is_empty() { "no stat buff in data (behaviour may be in game code)".to_string() } else { tip }
                                on:click=move |_| lo.update(|l| {
                                    if let Some(i) = l.perks.iter().position(|x| *x == p2) {
                                        l.perks.remove(i);
                                        l.buffs.retain(|b| !buffs.contains(b));
                                    } else {
                                        l.perks.push(p2.clone());
                                        for b in &buffs { if !l.buffs.contains(b) { l.buffs.push(b.clone()); } }
                                    }
                                })>
                                {db().perks.get(&p).and_then(|x| x.icon.clone()).map(|i| view! { <img class="chip-ic" src=format!("icons/ability/{i}.webp") alt=""/> })}
                                {name}{star.then_some(" ★")}
                            </button>
                        }
                    }).collect_view()
                }}
            </div>
        </details>
    }
}

/// Gear/buff stat keys the grouped sheet already shows (anything else nonzero goes to "Other").
const SHEET_COVERED: &[&str] = &[
    "PhysicalPower", "PhysicalDamageBonus", "MagicalPower", "MagicalDamageBonus", "MaxHealthBonus", "MaxHealthAdd",
    "MoveSpeed", "MoveSpeedAdd", "MoveSpeedBonus", "ActionSpeed", "ManualDexterity", "ItemEquipSpeed",
    "RegularInteractionSpeed", "MagicalInteractionSpeed", "SpellCastingSpeed", "MemoryCapacityBonus", "MemoryCapacityAdd",
    "CooldownReductionBonus", "BuffDurationBonus", "DebuffDurationBonus", "ArmorRating", "ArmorRatingAdd",
    "ItemArmorRatingMod", "PhysicalDamageReduction", "PhysicalReductionMod", "MagicRegistance", "MagicalDamageReduction",
    "MagicalReductionMod", "ProjectileReductionMod", "HeadshotReductionMod", "HeadshotDamageMod", "ArmorPenetration",
    "MagicPenetration", "PhysicalDamageAdd", "MagicalDamageAdd", "PhysicalWeaponDamageAdd", "MagicalWeaponDamageAdd",
    "PhysicalDamageTrue", "MagicalDamageTrue", "PhysicalShield", "MagicalShield", "Luck", "Persuasiveness",
    // weapon base damage and other item-only numbers are shown on the item, not the sheet
    "PhysicalWeaponDamage", "MagicalWeaponDamage", "Primitive", "BuffWeaponDamage",
];

#[component]
fn StatSheet(stats: Memo<Stats>) -> impl IntoView {
    view! {
        <div class="sheet">
            {move || {
                let s = stats.get();
                let attrs = s.attrs.iter().map(|(k, v)| view! { <div class="attr"><span>{k.get(..3).unwrap_or(k).to_string()}</span><b>{format!("{v:.0}")}</b></div> }).collect_view();
                let n0 = |v: f64| format!("{v:.0}");
                // (label, shown value, is zero)
                let row = |k: &'static str, v: f64, f: &dyn Fn(f64) -> String| (k, f(v), v.abs() < 1e-9);
                let groups: Vec<(&str, Vec<(&str, String, bool)>)> = vec![
                    ("Offense", vec![
                        row("Physical Power", s.physical_power, &n0),
                        row("Phys Power Bonus", s.physical_power_bonus, &spct),
                        row("Weapon Dmg +", s.g("PhysicalWeaponDamageAdd"), &n1),
                        row("Add. Phys Dmg", s.g("PhysicalDamageAdd"), &n1),
                        row("True Phys Dmg", s.g("PhysicalDamageTrue"), &n1),
                        row("Armor Pen", s.armor_penetration, &pct),
                        row("Magical Power", s.magical_power, &n0),
                        row("Mag Power Bonus", s.magical_power_bonus, &spct),
                        row("Mag Weapon Dmg +", s.g("MagicalWeaponDamageAdd"), &n1),
                        row("Add. Mag Dmg", s.g("MagicalDamageAdd"), &n1),
                        row("True Mag Dmg", s.g("MagicalDamageTrue"), &n1),
                        row("Magic Pen", s.magic_penetration, &pct),
                        row("Headshot Bonus", s.headshot_bonus, &pct),
                    ]),
                    ("Defense", vec![
                        row("Max Health", s.max_health, &n0),
                        row("Health Recovery", s.health_recovery, &spct),
                        row("Armor Rating", s.armor_rating, &n1),
                        row("Physical DR", s.physical_reduction, &pct),
                        row("Magic Resist", s.magic_resistance, &n1),
                        row("Magical DR", s.magical_reduction, &pct),
                        row("Projectile DR", s.projectile_reduction, &pct),
                        row("Headshot DR", s.headshot_reduction, &pct),
                        row("Physical Shield", s.physical_shield, &n0),
                        row("Magical Shield", s.magical_shield, &n0),
                    ]),
                    ("Speed", vec![
                        row("Move Speed", s.move_speed, &n1),
                        row("Action Speed", s.action_speed, &spct),
                        row("Spell Cast Speed", s.spell_casting_speed, &spct),
                        row("Manual Dexterity", s.manual_dexterity, &spct),
                        row("Item Equip Speed", s.item_equip_speed, &spct),
                        row("Interaction Speed", s.regular_interaction_speed, &spct),
                        row("Magic Interaction", s.magical_interaction_speed, &spct),
                    ]),
                    ("Utility", vec![
                        row("Luck", s.g("Luck"), &n0),
                        row("Persuasiveness", s.g("Persuasiveness"), &n0),
                        row("Memory Capacity", s.memory_capacity, &n0),
                        row("Cooldown Reduction", s.cooldown_reduction, &spct),
                        row("Buff Duration", s.buff_duration, &spct),
                        row("Debuff Duration Reduction", -s.debuff_duration, &spct),
                    ]),
                ];
                // every other nonzero stat from gear / buffs, so nothing is hidden
                let attr_keys: Vec<String> = db().attributes.iter().flat_map(|a| [a.clone(), format!("{a}Mod")]).collect();
                let mut other: Vec<(String, String)> = s.totals.iter()
                    .filter(|(k, v)| v.abs() > 1e-9 && !SHEET_COVERED.contains(&k.as_str()) && !attr_keys.contains(k))
                    .map(|(k, v)| (db().stat_name(k), stat_value(k, *v)))
                    .collect();
                other.sort();
                view! {
                    <div class="attrs">{attrs}</div>
                    <div class="stat-groups">
                        {groups.into_iter().map(|(title, rows)| view! {
                            <div class="stat-group">
                                <div class="sg-title">{title}</div>
                                <table class="stats">{rows.into_iter().map(|(k, v, zero)| view! {
                                    <tr class=if zero { "zero" } else { "" }><td>{k}</td><td class="num">{v}</td></tr>
                                }).collect_view()}</table>
                            </div>
                        }).collect_view()}
                        {(!other.is_empty()).then(|| view! {
                            <div class="stat-group">
                                <div class="sg-title" title="These stats are shown but not used by the damage / TTK math yet">"Other · not in the math yet"</div>
                                <table class="stats">{other.into_iter().map(|(k, v)| view! { <tr><td>{k}</td><td class="num">{v}</td></tr> }).collect_view()}</table>
                            </div>
                        })}
                    </div>
                }
            }}
        </div>
    }
}

