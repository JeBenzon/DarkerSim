//! Item display: in-game style tooltip card, a hover tooltip used everywhere, and the Items browser.

use crate::{db, item_name, stat_value};
use dad_core::data::ItemFamily;
use dad_core::gen::enchant_count;
use dad_core::stats::{all_slots, prop_value, slot_active, Equipped, Loadout, WEAPON_DMG};
use leptos::prelude::*;

pub fn icon_url(icon: &str) -> String {
    format!("icons/{icon}.webp")
}

/// Icon of an item at a rarity (falls back to any rarity that has one).
pub fn icon_for(fam: &ItemFamily, rarity: &str) -> Option<String> {
    fam.variants
        .get(rarity)
        .and_then(|v| v.icon.clone())
        .or_else(|| db().variants_ordered(fam).into_iter().find_map(|(_, v)| v.icon.clone()))
}

fn rarity_label(r: &str) -> &str {
    if r == "Legend" { "Legendary" } else { r }
}

fn slot_label(fam: &ItemFamily) -> String {
    let hand = match fam.hand.as_deref() {
        Some("TwoHanded") => "Two Handed",
        Some("OneHanded") => "One Handed",
        Some(h) => h,
        None => "",
    };
    let kind = if !fam.weapon_types.is_empty() {
        fam.weapon_types.iter().map(|t| dad_core::humanize(t)).collect::<Vec<_>>().join(" / ")
    } else if let Some(a) = &fam.armor_type {
        a.clone()
    } else {
        fam.item_type.clone().unwrap_or_default()
    };
    let slot = match fam.slot.as_deref() {
        Some("Primary") => "Primary Weapon",
        Some("Secondary") => "Secondary Weapon",
        Some("Foot") => "Feet",
        Some("Back") => "Cloak",
        Some(s) => s,
        None => "",
    };
    [hand.to_string(), kind, slot.to_string()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
}

fn fmt_range(stat: &str, min: f64, max: f64) -> String {
    let r = db().ratio(stat);
    if (min - max).abs() < f64::EPSILON {
        stat_value(stat, min * r)
    } else if db().is_pct(stat) {
        format!("+{:.1}~{:.1}%", min * r * 100.0, max * r * 100.0)
    } else {
        format!("{min}~{max}")
    }
}

/// The tooltip card. `eq` = a concrete equipped roll (shows exact values + enchants).
#[component]
pub fn ItemCard(item: String, rarity: String, eq: Option<Equipped>) -> impl IntoView {
    let Some(fam) = db().items.get(&item) else { return view! { <div class="itemcard">{item}</div> }.into_any() };
    let var = fam.variants.get(&rarity);
    let ridx = db().rarity_index(&rarity) + 1;
    let wiki = fam.wiki.as_ref();
    let icon = icon_for(fam, &rarity);
    let props = var
        .map(|v| {
            v.props
                .iter()
                .map(|p| {
                    let weapon = WEAPON_DMG.contains(&p.stat.as_str());
                    let value = match &eq {
                        Some(e) => {
                            let val = prop_value(db(), p, e.roll);
                            if db().is_pct(&p.stat) { stat_value(&p.stat, val) } else { format!("{}", (val * 10.0).round() / 10.0) }
                        }
                        None => fmt_range(&p.stat, p.min, p.max),
                    };
                    let cls = if weapon { "prop weapon" } else if p.min < 0.0 { "prop neg" } else { "prop" };
                    view! { <div class=cls><span>{db().stat_name(&p.stat)}</span><b>{value}</b></div> }
                })
                .collect_view()
        });
    let enchants = match &eq {
        Some(e) if !e.enchants.is_empty() => Some(
            e.enchants
                .iter()
                .map(|en| view! { <div class="prop ench"><span>{db().stat_name(&en.stat)}</span><b>{stat_value(&en.stat, en.raw * db().ratio(&en.stat))}</b></div> })
                .collect_view()
                .into_any(),
        ),
        _ => {
            let n = enchant_count(db(), fam, &rarity);
            (n > 0).then(|| view! { <div class="prop ench muted">{format!("+ {n} random enchantment{}", if n == 1 { "" } else { "s" })}</div> }.into_any())
        }
    };
    let classes = var.map(|v| if v.classes.is_empty() { fam.classes.clone() } else { v.classes.clone() }).unwrap_or_default();
    let flavor = wiki.and_then(|w| w.flavortext.clone()).filter(|s| !s.is_empty());
    let price = wiki.and_then(|w| w.sellprices.get(&ridx.to_string()).copied().flatten());
    let gs = wiki.and_then(|w| w.gearscore.get(&ridx.to_string()).copied().flatten());
    let size = wiki.and_then(|w| Some(format!("{}×{}", w.invwidth?, w.invheight?)));
    view! {
        <div class=format!("itemcard rc-{rarity}")>
            <div class="ic-head">
                {icon.map(|i| view! { <img class="ic-icon" src=icon_url(&i) alt=""/> })}
                <div class="ic-title">
                    <div class=format!("ic-name r-{rarity}")>{fam.name.clone()}</div>
                    <div class=format!("ic-rarity r-{rarity}")>{rarity_label(&rarity).to_string()}</div>
                    <div class="ic-type">{slot_label(fam)}</div>
                </div>
            </div>
            <div class="ic-props">{props}</div>
            {enchants.map(|e| view! { <div class="ic-sep"></div><div class="ic-props">{e}</div> })}
            {(!classes.is_empty()).then(|| view! { <div class="ic-req">"Classes: " {classes.join(", ")}</div> })}
            {flavor.map(|f| view! { <div class="ic-flavor">{f}</div> })}
            <div class="ic-foot">
                <span>{size.map(|s| format!("Size {s}"))}</span>
                <span>{gs.map(|g| format!("Gear score {g}"))}</span>
                <span class="gold">{price.map(|p| format!("{p} gold"))}</span>
            </div>
        </div>
    }
    .into_any()
}

// ------------------------------------------------------------------ hover tooltip

#[derive(Clone, PartialEq)]
pub struct TipItem {
    pub item: String,
    pub rarity: String,
    pub eq: Option<Equipped>,
}

#[derive(Clone, Copy)]
pub struct Tip {
    pub item: RwSignal<Option<TipItem>>,
    pub pos: RwSignal<(f64, f64)>,
}

pub fn provide_tip() -> Tip {
    let t = Tip { item: RwSignal::new(None), pos: RwSignal::new((0.0, 0.0)) };
    provide_context(t);
    t
}

/// Wraps content; hovering it shows the item tooltip next to the cursor.
#[component]
pub fn WithTip(item: String, rarity: String, #[prop(optional)] eq: Option<Equipped>, children: Children) -> impl IntoView {
    let tip = expect_context::<Tip>();
    let me = TipItem { item, rarity, eq };
    view! {
        <span class="tipwrap"
            on:mousemove=move |ev| {
                tip.pos.set((ev.client_x() as f64, ev.client_y() as f64));
                if tip.item.with_untracked(|t| t.as_ref() != Some(&me)) {
                    tip.item.set(Some(me.clone()));
                }
            }
            on:mouseleave=move |_| tip.item.set(None)>
            {children()}
        </span>
    }
}

/// The single floating tooltip (mounted once at the app root).
#[component]
pub fn FloatingTip() -> impl IntoView {
    let tip = expect_context::<Tip>();
    let style = move || {
        let (x, y) = tip.pos.get();
        let (w, h) = web_sys::window()
            .map(|w| {
                (
                    w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(1600.0),
                    w.inner_height().ok().and_then(|v| v.as_f64()).unwrap_or(900.0),
                )
            })
            .unwrap_or((1600.0, 900.0));
        let left = if x + 360.0 > w { (x - 352.0).max(4.0) } else { x + 18.0 };
        let top = if y + 420.0 > h { (h - 420.0).max(4.0) } else { y + 14.0 };
        format!("left:{left}px;top:{top}px")
    };
    move || {
        tip.item.get().map(|t| {
            view! { <div class="floattip" style=style><ItemCard item=t.item rarity=t.rarity eq=t.eq/></div> }
        })
    }
}

// ------------------------------------------------------------------ items browser

/// Gear slot key an item goes into (second ring goes to ring2 when ring1 is used).
pub fn slot_key_for(fam: &ItemFamily, lo: &Loadout) -> Option<&'static str> {
    let slot = fam.slot.as_deref()?;
    // only the weapon set that's in hand; rings fill the free ring slot first
    let keys: Vec<&'static str> = all_slots().filter(|(k, _, s)| *s == slot && slot_active(lo, k)).map(|(k, _, _)| *k).collect();
    keys.iter().find(|k| !lo.gear.contains_key(**k)).or(keys.first()).copied()
}

pub fn equip(lo: RwSignal<Loadout>, item: &str, rarity: &str) {
    let Some(fam) = db().items.get(item) else { return };
    let rarity = if fam.variants.contains_key(rarity) {
        rarity.to_string()
    } else {
        db().variants_ordered(fam).last().map(|(r, _)| r.to_string()).unwrap_or_default()
    };
    lo.update(|l| {
        if let Some(k) = slot_key_for(fam, l) {
            l.gear.insert(k.to_string(), Equipped { item: item.to_string(), rarity, roll: 1.0, enchants: vec![] });
        }
    });
}

#[component]
pub fn ItemBrowser(atk: RwSignal<Loadout>, tgt: RwSignal<Loadout>) -> impl IntoView {
    let query = RwSignal::new(String::new());
    let kind = RwSignal::new(String::from("Gear"));
    let cls = RwSignal::new(String::new());
    let rarity = RwSignal::new(String::from("Epic"));
    let selected = RwSignal::new(None::<String>);

    let mut classes: Vec<String> = db().classes.keys().cloned().collect();
    classes.sort();
    let rarities: Vec<String> = db().rarities.clone();
    let kinds = ["Gear", "Weapon", "Armor", "Accessory", "Utility", "Misc", "All"];

    let list = Memo::new(move |_| {
        let q = query.get().to_lowercase();
        let k = kind.get();
        let c = cls.get();
        let mut v: Vec<&'static ItemFamily> = db()
            .items
            .values()
            .filter(|f| {
                let t = f.item_type.as_deref().unwrap_or("");
                let kind_ok = match k.as_str() {
                    "All" => true,
                    "Gear" => f.slot.is_some() && f.variants.values().any(|v| !v.props.is_empty()),
                    other => t == other,
                };
                kind_ok
                    && (q.is_empty() || f.name.to_lowercase().contains(&q) || f.id.to_lowercase().contains(&q))
                    && (c.is_empty() || f.classes.iter().any(|x| *x == c))
            })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v.into_iter().map(|f| f.id.clone()).collect::<Vec<_>>()
    });

    // rarity shown for a family: the chosen one if it exists, otherwise the closest
    let shown_rarity = move |fam: &ItemFamily| -> String {
        let want = rarity.get();
        if fam.variants.contains_key(&want) {
            return want;
        }
        let wi = db().rarity_index(&want) as i64;
        db().variants_ordered(fam)
            .into_iter()
            .min_by_key(|(r, _)| (db().rarity_index(r) as i64 - wi).abs())
            .map(|(r, _)| r.to_string())
            .unwrap_or_default()
    };

    view! {
        <section class="card items-page">
            <div class="row">
                <input class="grow" placeholder="search items…" prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
                <select on:change=move |ev| kind.set(event_target_value(&ev))>
                    {kinds.iter().map(|k| view! { <option value=*k selected=*k == "Gear">{*k}</option> }).collect_view()}
                </select>
                <select on:change=move |ev| cls.set(event_target_value(&ev))>
                    <option value="">"any class"</option>
                    {classes.iter().map(|c| view! { <option value=c.clone()>{c.clone()}</option> }).collect_view()}
                </select>
                <div class="rarity-pick">
                    {rarities.iter().map(|r| { let r2 = r.clone(); let r3 = r.clone(); view! {
                        <button class=move || format!("rb r-{} {}", r3, if rarity.get() == r3 { "on" } else { "" }) on:click=move |_| rarity.set(r2.clone())>{rarity_label(r).to_string()}</button>
                    } }).collect_view()}
                </div>
                <span class="muted small">{move || format!("{} items", list.get().len())}</span>
            </div>
            <div class="items-layout">
                <div class="item-grid">
                    {move || list.get().into_iter().map(|id| {
                        let fam = &db().items[&id];
                        let r = shown_rarity(fam);
                        let icon = icon_for(fam, &r);
                        let id2 = id.clone();
                        let sel = move || selected.get().as_deref() == Some(id2.as_str());
                        let id3 = id.clone();
                        view! {
                            <WithTip item=id.clone() rarity=r.clone()>
                                <div class=move || format!("item-tile rc-{} {}", r, if sel() { "sel" } else { "" }) on:click=move |_| selected.set(Some(id3.clone()))>
                                    {match icon { Some(i) => view! { <img src=icon_url(&i) loading="lazy" alt=""/> }.into_any(), None => view! { <div class="noicon">"?"</div> }.into_any() }}
                                    <span>{fam.name.clone()}</span>
                                </div>
                            </WithTip>
                        }
                    }).collect_view()}
                </div>
                <aside class="item-detail">
                    {move || match selected.get() {
                        None => view! { <p class="muted">"Click an item to pin it here. Hover any item for its tooltip."</p> }.into_any(),
                        Some(id) => {
                            let fam = &db().items[&id];
                            let r = shown_rarity(fam);
                            let all: Vec<String> = db().variants_ordered(fam).into_iter().map(|(r, _)| r.to_string()).collect();
                            let (id_a, id_t, r_a, r_t) = (id.clone(), id.clone(), r.clone(), r.clone());
                            view! {
                                <ItemCard item=id.clone() rarity=r.clone() eq=None/>
                                <div class="row">
                                    {all.into_iter().map(|x| { let x2 = x.clone(); view! { <button class=format!("rb r-{x} {}", if x == r { "on" } else { "" }) on:click=move |_| rarity.set(x2.clone())>{rarity_label(&x).to_string()}</button> } }).collect_view()}
                                </div>
                                {fam.slot.is_some().then(|| view! {
                                    <div class="row">
                                        <button on:click=move |_| equip(atk, &id_a, &r_a)>"Equip on attacker"</button>
                                        <button on:click=move |_| equip(tgt, &id_t, &r_t)>"Equip on target"</button>
                                    </div>
                                })}
                                {(!fam.attacks.is_empty()).then(|| view! {
                                    <table class="dmg">
                                        <tr><th>"Attack"</th><th>"Type"</th><th class="num">"Combo"</th><th class="num">"Swing"</th></tr>
                                        {fam.attacks.iter().map(|a| view! { <tr><td>{a.name.clone()}</td><td>{a.damage_type.clone().unwrap_or_default()}</td>
                                            <td class="num">{format!("{:.0}%", a.combo * 100.0)}</td><td class="num">{a.cycle.map(|c| format!("{c:.2}s")).unwrap_or_default()}</td></tr> }).collect_view()}
                                    </table>
                                })}
                            }.into_any()
                        }
                    }}
                </aside>
            </div>
        </section>
    }
}

/// Small icon used in gear rows; hovering shows the exact equipped roll.
#[component]
pub fn GearIcon(eq: Equipped) -> impl IntoView {
    let fam = db().items.get(&eq.item);
    let icon = fam.and_then(|f| icon_for(f, &eq.rarity));
    let (item, rarity) = (eq.item.clone(), eq.rarity.clone());
    view! {
        <WithTip item=item rarity=rarity.clone() eq=eq.clone()>
            <span class=format!("gicon rc-{rarity}")>
                {icon.map(|i| view! { <img src=icon_url(&i) alt=item_name(&eq.item)/> })}
            </span>
        </WithTip>
    }
}
