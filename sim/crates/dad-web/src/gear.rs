//! Equipment view: an in-game style paper doll plus an interactive item card editor.
//!
//! Rarity and enchant count are tied together like in the game (Uncommon 1, Rare 2, Epic 3, Legendary 4,
//! Unique 5): adding an enchant past the limit bumps the item a tier, removing one drops it back down.

use crate::items::{icon_for, icon_url, WithTip};
use crate::{db, item_name, items_by_slot, js_sys_now, next_id, stat_value};
use dad_core::data::{ItemFamily, PoolEntry};
use dad_core::gen::{enchant_count, Rng};
use dad_core::stats::{all_slots, Enchant, Equipped, Loadout, WEAPON_DMG};
use crate::controls::Stepper;
use leptos::prelude::*;

/// (display scale, decimals, suffix) for a stat: percent stats are stored per mille.
fn fmt_of(stat: &str) -> (f64, usize, &'static str) {
    if db().is_pct(stat) { (db().ratio(stat) * 100.0, 1, "%") } else { (db().ratio(stat), 0, "") }
}

// ------------------------------------------------------------------ rules

fn fam(id: &str) -> Option<&'static ItemFamily> {
    db().items.get(id)
}
fn rarities_of(f: &ItemFamily) -> Vec<String> {
    db().variants_ordered(f).into_iter().filter(|(_, v)| !v.props.is_empty()).map(|(r, _)| r.to_string()).collect()
}
fn cap(f: &ItemFamily, r: &str) -> usize {
    enchant_count(db(), f, r) as usize
}
fn pool(f: &ItemFamily) -> &'static [PoolEntry] {
    f.secondary_pool.as_ref().and_then(|k| db().secondary_pools.get(k)).map(|v| v.as_slice()).unwrap_or(&[])
}
fn base_stats(f: &ItemFamily, r: &str) -> Vec<String> {
    f.variants.get(r).map(|v| v.props.iter().map(|p| p.stat.clone()).collect()).unwrap_or_default()
}
/// Enchant stats that can still be added to this item.
fn addable(f: &ItemFamily, e: &Equipped) -> Vec<&'static PoolEntry> {
    let base = base_stats(f, &e.rarity);
    pool(f).iter().filter(|p| !base.contains(&p.stat) && !e.enchants.iter().any(|x| x.stat == p.stat)).collect()
}
/// First higher rarity that allows at least `n` enchants.
fn rarity_for_more(f: &ItemFamily, cur: &str, n: usize) -> Option<String> {
    let rs = rarities_of(f);
    let i = rs.iter().position(|r| r == cur)?;
    rs.into_iter().skip(i + 1).find(|r| cap(f, r) >= n)
}
/// Rarity matching exactly `n` enchants, closest to the current one (keeps current if it fits).
fn rarity_for_count(f: &ItemFamily, cur: &str, n: usize) -> String {
    if cap(f, cur) == n {
        return cur.to_string();
    }
    let ci = db().rarity_index(cur) as i64;
    rarities_of(f)
        .into_iter()
        .filter(|r| cap(f, r) == n)
        .min_by_key(|r| (db().rarity_index(r) as i64 - ci).abs())
        .unwrap_or_else(|| cur.to_string())
}
fn rng() -> Rng {
    Rng::new(next_id() ^ (js_sys_now() as u64))
}

fn edit(lo: RwSignal<Loadout>, key: &'static str, f: impl FnOnce(&mut Equipped, &'static ItemFamily)) {
    lo.update(|l| {
        if let Some(e) = l.gear.get_mut(key) {
            if let Some(fm) = fam(&e.item) {
                f(e, fm)
            }
        }
    })
}

fn add_enchant(lo: RwSignal<Loadout>, key: &'static str, stat: String) {
    edit(lo, key, |e, f| {
        let Some(p) = pool(f).iter().find(|p| p.stat == stat) else { return };
        let n = e.enchants.len() + 1;
        if n > cap(f, &e.rarity) {
            match rarity_for_more(f, &e.rarity, n) {
                Some(r) => e.rarity = r,
                None => return,
            }
        }
        e.enchants.push(Enchant { stat, raw: p.max });
    })
}

fn remove_enchant(lo: RwSignal<Loadout>, key: &'static str, i: usize) {
    edit(lo, key, |e, f| {
        if i < e.enchants.len() {
            e.enchants.remove(i);
            e.rarity = rarity_for_count(f, &e.rarity, e.enchants.len());
        }
    })
}

fn set_rarity(lo: RwSignal<Loadout>, key: &'static str, r: String) {
    edit(lo, key, |e, f| {
        e.enchants.truncate(cap(f, &r));
        e.rarity = r;
    })
}

fn reroll_values(lo: RwSignal<Loadout>, key: &'static str) {
    let mut g = rng();
    edit(lo, key, |e, f| {
        e.roll = (g.f() * 20.0).round() / 20.0;
        for en in &mut e.enchants {
            if let Some(p) = pool(f).iter().find(|p| p.stat == en.stat) {
                en.raw = (p.min + (p.max - p.min) * g.f()).round();
            }
        }
    })
}

fn reroll_all(lo: RwSignal<Loadout>, key: &'static str) {
    let mut g = rng();
    edit(lo, key, |e, f| {
        let n = cap(f, &e.rarity);
        let base = base_stats(f, &e.rarity);
        let mut left: Vec<&PoolEntry> = pool(f).iter().filter(|p| !base.contains(&p.stat)).collect();
        e.enchants.clear();
        e.roll = (g.f() * 20.0).round() / 20.0;
        for _ in 0..n {
            let w: Vec<f64> = left.iter().map(|p| p.weight.max(1.0)).collect();
            let Some(i) = g.pick_weighted(&w) else { break };
            let p = left.remove(i);
            e.enchants.push(Enchant { stat: p.stat.clone(), raw: (p.min + (p.max - p.min) * g.f()).round() });
        }
    })
}

fn max_out(lo: RwSignal<Loadout>, key: &'static str) {
    edit(lo, key, |e, f| {
        e.roll = 1.0;
        for en in &mut e.enchants {
            if let Some(p) = pool(f).iter().find(|p| p.stat == en.stat) {
                en.raw = p.max;
            }
        }
    })
}

/// Put another item in the slot, keeping rarity, roll and any enchants the new item can also have.
fn swap_item(lo: RwSignal<Loadout>, key: &'static str, item: &str) {
    let Some(nf) = fam(item) else { return };
    lo.update(|l| {
        let old = l.gear.get(key).cloned();
        let rs = rarities_of(nf);
        let want = old.as_ref().map(|o| o.rarity.clone()).unwrap_or_else(|| "Epic".into());
        let wi = db().rarity_index(&want) as i64;
        let rarity = if rs.contains(&want) {
            want
        } else {
            rs.iter().min_by_key(|r| (db().rarity_index(r) as i64 - wi).abs()).cloned().unwrap_or_default()
        };
        let base = base_stats(nf, &rarity);
        let mut enchants: Vec<Enchant> = old
            .as_ref()
            .map(|o| o.enchants.iter().filter(|e| pool(nf).iter().any(|p| p.stat == e.stat) && !base.contains(&e.stat)).cloned().collect())
            .unwrap_or_default();
        enchants.truncate(cap(nf, &rarity));
        let roll = old.map(|o| o.roll).unwrap_or(1.0);
        l.gear.insert(key.to_string(), Equipped { item: item.to_string(), rarity, roll, enchants });
    });
}

// ------------------------------------------------------------------ paper doll

/// Slot boxes in the in-game equipment layout, in px of a 534×460 frame (measured from the game UI).
/// (key, x, y, w, h)
const LAYOUT: [(&str, f64, f64, f64, f64); 14] = [
    ("weapon", 37.0, 62.0, 58.0, 148.0),
    ("offhand", 96.0, 62.0, 58.0, 148.0),
    ("weapon2", 391.0, 62.0, 58.0, 148.0),
    ("offhand2", 450.0, 62.0, 58.0, 148.0),
    ("head", 233.0, 72.0, 76.0, 76.0),
    ("necklace", 320.0, 107.0, 42.0, 42.0),
    ("sash", 133.0, 222.0, 76.0, 76.0),
    ("chest", 220.0, 160.0, 103.0, 140.0),
    ("back", 335.0, 222.0, 76.0, 76.0),
    ("ring1", 167.0, 312.0, 42.0, 42.0),
    ("ring2", 335.0, 312.0, 42.0, 42.0),
    ("hands", 133.0, 365.0, 76.0, 76.0),
    ("legs", 220.0, 312.0, 103.0, 140.0),
    ("foot", 335.0, 365.0, 76.0, 76.0),
];
const FW: f64 = 534.0;
const FH: f64 = 460.0;

fn pct_box(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("left:{:.3}%;top:{:.3}%;width:{:.3}%;height:{:.3}%", x / FW * 100.0, y / FH * 100.0, w / FW * 100.0, h / FH * 100.0)
}

#[component]
pub fn PaperDoll(
    lo: RwSignal<Loadout>,
    selected: RwSignal<Option<&'static str>>,
    side: &'static str,
    all_items: RwSignal<bool>,
) -> impl IntoView {
    // right-click menu on an equipped item: (cursor x, cursor y, slot)
    let menu = RwSignal::new(None::<(f64, f64, &'static str)>);
    view! {
        <div class="paperdoll">
            // weapon sets: the one "in hand" counts for stats and combat; click a header to swap
            {[(0u8, 33.0), (1u8, 387.0)].into_iter().map(|(n, x)| {
                let active = move || lo.with(|l| l.weapon_set) == n;
                view! {
                    <div class=move || if active() { "pd-set on" } else { "pd-set off" } style=pct_box(x, 45.0, 124.0, 168.0)>
                        <button class="pd-set-head" title=move || if active() { "This weapon set is in hand".to_string() } else { "Swap to this weapon set".to_string() }
                            on:click=move |_| lo.update(|l| l.weapon_set = n)>
                            {move || if active() { format!("Set {} · in hand", n + 1) } else { format!("Set {} · swap", n + 1) }}
                        </button>
                    </div>
                }
            }).collect_view()}
            {LAYOUT.iter().map(|(key, x, y, w, h)| {
                let key: &'static str = key;
                let label = all_slots().find(|s| s.0 == key).map(|s| s.1).unwrap_or(key);
                let eq = Memo::new(move |_| lo.with(|l| l.gear.get(key).cloned()));
                let small = *w < 50.0;
                view! {
                    <div class=move || format!("pd-cell {} {} {}", if small { "small" } else { "" }, if selected.get() == Some(key) { "sel" } else { "" },
                        if use_context::<crate::link::LinkState>().and_then(|st| st.last.get()).map(|(s, k, _)| s == side && k == key).unwrap_or(false) { "linked" } else { "" })
                        style=pct_box(*x, *y, *w, *h)
                        on:click=move |_| selected.set(if selected.get_untracked() == Some(key) { None } else { Some(key) })
                        on:contextmenu=move |ev| if lo.with_untracked(|l| l.gear.contains_key(key)) {
                            ev.prevent_default();
                            menu.set(Some((ev.client_x() as f64, ev.client_y() as f64, key)));
                        }>
                        {move || match eq.get() {
                            Some(e) => {
                                let icon = fam(&e.item).and_then(|f| icon_for(f, &e.rarity));
                                let class = lo.with(|l| l.class.clone());
                                let misfit = fam(&e.item).map(|f| !crate::gear_fits(f, &class)).unwrap_or(false);
                                let inner = view! {
                                    <div class=format!("pd-item rc-{}", e.rarity)>
                                        {icon.map(|i| view! { <img src=icon_url(&i) alt=""/> })}
                                        <span class="pd-ench">{"◆".repeat(e.enchants.len())}</span>
                                        {misfit.then(|| view! {
                                            <div class="pd-misfit" title=format!("This item does not fit {class}")>
                                                <span>{if small { "✕".to_string() } else { format!("Doesn't fit {class}") }}</span>
                                            </div>
                                        })}
                                    </div>
                                };
                                // the pinned card replaces the hover tooltip for the selected slot
                                if selected.get() == Some(key) {
                                    inner.into_any()
                                } else {
                                    view! { <WithTip item=e.item.clone() rarity=e.rarity.clone() eq=e.clone()>{inner}</WithTip> }.into_any()
                                }
                            }
                            None => {
                                // our own line-art glyph for the empty slot
                                let glyph = match key {
                                    "weapon" | "weapon2" => "M19 5 L19 8 L10 17 L7 14 L16 5 Z M6 13 L11 18 M8.5 15.5 L5 19",
                                    "offhand" | "offhand2" => "M12 3 L19 6 C19 13 16 18 12 21 C8 18 5 13 5 6 Z M12 3 L12 21",
                                    "head" => "M5 16 C5 8 8 4 12 4 C16 4 19 8 19 16 L19 19 L14 19 L14 12 L10 12 L10 19 L5 19 Z",
                                    "necklace" => "M6 4 C6 10 9 13 12 14 C15 13 18 10 18 4 M12 14 L14.5 17 L12 20 L9.5 17 Z",
                                    "chest" => "M8 4 L5 6 L4 11 L7 12 L7 20 L17 20 L17 12 L20 11 L19 6 L16 4 C15 6 9 6 8 4 Z M12 7 L12 20",
                                    "back" => "M9 4 L15 4 L16 7 C18 12 19 17 20 20 L4 20 C5 17 6 12 8 7 Z M9 4 C10 6 14 6 15 4",
                                    "hands" => "M8 20 L8 11 L7 6 C7 5 8.5 5 9 6 L10 10 L10 4.5 C10 3.5 11.5 3.5 11.7 4.5 L12 10 L12.5 4.8 C12.7 3.8 14.2 4 14.2 5 L14 10.5 L15.2 6.5 C15.5 5.6 16.8 5.9 16.6 7 L16 13 L16 20 Z",
                                    "legs" => "M7 4 L17 4 L18 20 L13.5 20 L12 10 L10.5 20 L6 20 Z M7 7 L17 7",
                                    "foot" => "M8 4 L13 4 L13 14 L19 16 C20 16.5 20 20 19 20 L7 20 L8 14 Z M8 9 L13 9",
                                    "sash" => "M3 10 L21 10 L21 14 L3 14 Z M10 9 L14 9 L14 15 L10 15 Z",
                                    _ => "M12 8.5 A5.5 5.5 0 1 1 11.99 8.5 Z M10 6 L12 3.5 L14 6 L12 8.5 Z",
                                };
                                view! { <div class="pd-empty" title=label><svg class="pd-sil" viewBox="0 0 24 24"><path d=glyph/></svg></div> }.into_any()
                            }
                        }}
                    </div>
                }
            }).collect_view()}
            {move || menu.get().map(|(x, y, k)| {
                let name = lo.with(|l| l.gear.get(k).map(|e| item_name(&e.item))).unwrap_or_default();
                let close = move || menu.set(None);
                view! {
                    <div class="ap-backdrop ctx-backdrop" on:click=move |_| close()
                        on:contextmenu=move |ev| { ev.prevent_default(); close() }></div>
                    <div class="ctx-menu" style=format!("left:{x:.0}px; top:{y:.0}px")>
                        <div class="ctx-title">{name}</div>
                        <button class="ctx-item" autofocus=true
                            on:keydown=move |ev| if ev.key() == "Escape" { close() }
                            on:click=move |_| {
                                lo.update(|l| { l.gear.remove(k); });
                                if selected.get_untracked() == Some(k) { selected.set(None); }
                                close();
                            }>"Unequip"</button>
                    </div>
                }
            })}
            // pinned, editable card next to the equipment panel (like the in-game item card)
            {move || selected.get().map(|k| {
                let top = LAYOUT.iter().find(|l| l.0 == k).map(|l| (l.2 / FH * 100.0).min(45.0)).unwrap_or(0.0);
                view! {
                    <div class=format!("pd-pin {side}") style=format!("top:{top:.2}%")>
                        <button class="pin-close" title="close" on:click=move |_| selected.set(None)>"✕"</button>
                        <ItemEditor lo=lo slot_key=k all_items=all_items/>
                    </div>
                }
            })}
        </div>
    }
}
// ------------------------------------------------------------------ interactive card

#[component]
pub fn ItemEditor(lo: RwSignal<Loadout>, slot_key: &'static str, all_items: RwSignal<bool>) -> impl IntoView {
    let key = slot_key;
    let eq = Memo::new(move |_| lo.with(|l| l.gear.get(key).cloned()));
    // the card shell only rebuilds when the item itself changes, so sliders keep working while dragging
    let cur_item = Memo::new(move |_| eq.get().map(|e| e.item));
    let item_slot = all_slots().find(|s| s.0 == key).map(|s| s.2).unwrap_or("");
    let label = all_slots().find(|s| s.0 == key).map(|s| s.1).unwrap_or(key);
    let swapping = RwSignal::new(false);
    let adding = RwSignal::new(false);
    let q = RwSignal::new(String::new());
    let eq_q = RwSignal::new(String::new());
    let n_ench = Memo::new(move |_| eq.get().map(|e| e.enchants.len()).unwrap_or(0));
    let _ = n_ench;
    let idxs = move || -> Vec<(usize, String)> {
        eq.get().map(|e| e.enchants.iter().enumerate().map(|(i, x)| (i, x.stat.clone())).collect()).unwrap_or_default()
    };
    let prop_keys = move || -> Vec<(String, usize)> {
        eq.get()
            .and_then(|e| fam(&e.item).and_then(|f| f.variants.get(&e.rarity)).map(|v| (0..v.props.len()).map(|i| (format!("{}:{}", e.item, e.rarity), i)).collect()))
            .unwrap_or_default()
    };

    // ---- item picker: every item for this slot — the ones the class can use first, the rest in red (still equippable)
    let _ = all_items;
    let tile = move |n: String, id: String, bad: bool| {
        let f = fam(&id).unwrap();
        let r = eq.get_untracked().map(|e| e.rarity).filter(|r| f.variants.contains_key(r)).unwrap_or_else(|| rarities_of(f).last().cloned().unwrap_or_default());
        let icon = icon_for(f, &r);
        let id2 = id.clone();
        view! {
            <WithTip item=id.clone() rarity=r.clone()>
                <button class=format!("pick rc-{r}{}", if bad { " misfit" } else { "" }) on:click=move |_| { swap_item(lo, key, &id2); swapping.set(false); q.set(String::new()); }>
                    {icon.map(|i| view! { <img src=icon_url(&i) alt="" loading="lazy"/> })}
                    <span>{n}</span>
                </button>
            </WithTip>
        }
    };
    let candidates = move || {
        let query = q.get().to_lowercase();
        let cls = lo.with(|l| l.class.clone());
        let cname = db().classes.get(&cls).map(|c| c.name.clone()).unwrap_or(cls.clone());
        let list: Vec<(String, String)> = items_by_slot()
            .get(item_slot)
            .map(|v| v.iter().filter(|(n, _)| query.is_empty() || n.to_lowercase().contains(&query)).cloned().collect())
            .unwrap_or_default();
        let (fit, misfit): (Vec<_>, Vec<_>) = list.into_iter().partition(|(_, id)| fam(id).is_some_and(|f| crate::gear_fits(f, &cls)));
        view! {
            <div class="pick-scroll">
                <div class="pick-sec">{format!("Fits {cname} ({})", fit.len())}</div>
                <div class="pick-grid">{fit.into_iter().map(|(n, id)| tile(n, id, false)).collect_view()}</div>
                {(!misfit.is_empty()).then(|| view! {
                    <div class="pick-sec bad">{format!("Doesn't fit {cname} — can still be equipped ({})", misfit.len())}</div>
                    <div class="pick-grid">{misfit.into_iter().map(|(n, id)| tile(n, id, true)).collect_view()}</div>
                })}
            </div>
        }
    };
    let swap_panel = move || {
        view! {
            <div class="swap">
                <div class="swap-head">
                    <input class="grow" placeholder=format!("search {} items…", label.to_lowercase()) prop:value=move || q.get()
                        on:input=move |ev| q.set(event_target_value(&ev))/>
                    {move || eq.get().is_some().then(|| view! {
                        <button class="ghost" title="close the list; the current item stays" on:click=move |_| { swapping.set(false); q.set(String::new()); }>"Cancel"</button>
                    })}
                </div>
                {candidates}
            </div>
        }
    };

    // ---- enchant picker: only stats this item can roll and doesn't have yet
    let add_panel = move || {
        let e = eq.get()?;
        let f = fam(&e.item)?;
        let query = eq_q.get().to_lowercase();
        let n = e.enchants.len() + 1;
        let bump = if n > cap(f, &e.rarity) { rarity_for_more(f, &e.rarity, n) } else { None };
        let list = addable(f, &e)
            .into_iter()
            .filter(|p| query.is_empty() || db().stat_name(&p.stat).to_lowercase().contains(&query))
            .map(|p| {
                let st = p.stat.clone();
                let range = format!("{} – {}", stat_value(&p.stat, p.min * db().ratio(&p.stat)), stat_value(&p.stat, p.max * db().ratio(&p.stat)));
                view! {
                    <button class="ench-pick" on:click=move |_| { add_enchant(lo, key, st.clone()); adding.set(false); eq_q.set(String::new()); }>
                        <span>{db().stat_name(&p.stat)}</span><small>{range}</small>
                    </button>
                }
            })
            .collect_view();
        Some(view! {
            <div class="addpanel">
                {bump.map(|r| view! { <div class=format!("bump r-{r}")>{format!("Adding this enchant upgrades the item to {}", if r == "Legend" { "Legendary" } else { r.as_str() })}</div> })}
                <input class="grow" placeholder="search enchantments…" prop:value=move || eq_q.get() on:input=move |ev| eq_q.set(event_target_value(&ev))/>
                <div class="ench-list">{list}</div>
            </div>
        })
    };

    move || match cur_item.get() {
        None => view! {
            <div class="editor empty">
                <div class="ed-title">{format!("{label}: empty")}</div>
                {swap_panel()}
            </div>
        }
        .into_any(),
        Some(_) => {
            view! {
                <div class=move || format!("itemcard editor rc-{}", eq.get().map(|e| e.rarity).unwrap_or_default())>
                    // ---- header: icon, name, rarity stepper
                    {move || eq.get().map(|e| {
                        let f = fam(&e.item).unwrap();
                        let rs = rarities_of(f);
                        let i = rs.iter().position(|r| *r == e.rarity).unwrap_or(0);
                        let prev = (i > 0).then(|| rs[i - 1].clone());
                        let next = rs.get(i + 1).cloned();
                        let icon = icon_for(f, &e.rarity);
                        let (p2, n2) = (prev.clone(), next.clone());
                        view! {
                            <div class="ic-head">
                                {icon.map(|ic| view! { <img class="ic-icon big" src=icon_url(&ic) alt=""/> })}
                                <div class="ic-title">
                                    <div class=format!("ic-name r-{}", e.rarity)>{item_name(&e.item)}</div>
                                    <div class="stepper">
                                        <button class="x" disabled=prev.is_none() on:click=move |_| if let Some(r) = p2.clone() { set_rarity(lo, key, r) }>"◀"</button>
                                        <span class=format!("ic-rarity r-{}", e.rarity)>{if e.rarity == "Legend" { "Legendary".to_string() } else { e.rarity.clone() }}</span>
                                        <button class="x" disabled=next.is_none() on:click=move |_| if let Some(r) = n2.clone() { set_rarity(lo, key, r) }>"▶"</button>
                                    </div>
                                    <div class="ic-type">{format!("{} · {}/{} enchants", label, e.enchants.len(), cap(f, &e.rarity))}</div>
                                </div>
                            </div>
                        }
                    })}

                    // ---- base properties: every stat with a range gets its own control (they share the item's roll)
                    <div class="ic-props">
                        <For each=prop_keys key=|k| k.clone() children=move |(_, i): (String, usize)| {
                            let p = move || eq.get().and_then(|e| fam(&e.item).and_then(|f| f.variants.get(&e.rarity)).and_then(|v| v.props.get(i).cloned()));
                            let Some(p0) = p() else { return view! { <div></div> }.into_any() };
                            let cls = if WEAPON_DMG.contains(&p0.stat.as_str()) { "prop weapon" } else if p0.min < 0.0 { "prop neg" } else { "prop" };
                            let (scale, dec, suf) = fmt_of(&p0.stat);
                            let name = db().stat_name(&p0.stat);
                            if p0.min == p0.max {
                                let txt = stat_value(&p0.stat, p0.min * db().ratio(&p0.stat));
                                return view! { <div class=cls><span>{name}</span><b class="fixed">{txt}</b></div> }.into_any();
                            }
                            let value = Signal::derive(move || p().map(|p| p.min + (p.max - p.min) * eq.get().map(|e| e.roll).unwrap_or(1.0)).unwrap_or(0.0));
                            let min = Signal::derive(move || p().map(|p| p.min).unwrap_or(0.0));
                            let max = Signal::derive(move || p().map(|p| p.max).unwrap_or(0.0));
                            let on_set = Callback::new(move |v: f64| edit(lo, key, |e, f| {
                                if let Some(p) = f.variants.get(&e.rarity).and_then(|vv| vv.props.get(i)) {
                                    if p.max != p.min { e.roll = ((v - p.min) / (p.max - p.min)).clamp(0.0, 1.0); }
                                }
                            }));
                            view! {
                                <div class=cls><span>{name}</span>
                                    <Stepper value=value min=min max=max scale=scale decimals=dec suffix=suf signed=db().is_pct(&p0.stat) on_set=on_set/>
                                </div>
                            }.into_any()
                        }/>
                        <div class="prop rollrow">
                            <span>"Base roll"</span>
                            <Stepper value=Signal::derive(move || (eq.get().map(|e| e.roll).unwrap_or(1.0) * 100.0).round())
                                min=Signal::derive(|| 0.0) max=Signal::derive(|| 100.0) step=5.0 suffix="%"
                                on_set=Callback::new(move |v: f64| edit(lo, key, |e, _| e.roll = v / 100.0))/>
                        </div>
                    </div>

                    // ---- enchants (rows are keyed by index so controls keep focus while you change them)
                    <div class="ic-sep"></div>
                    <div class="ic-props">
                        <For each=idxs key=|k| k.clone() children=move |(i, _): (usize, String)| {
                            let en = move || eq.get().and_then(|e| e.enchants.get(i).cloned());
                            let p = move || en().and_then(|x| eq.get().and_then(|e| fam(&e.item)).and_then(|f| pool(f).iter().find(|p| p.stat == x.stat)));
                            let stat0 = en().map(|x| x.stat).unwrap_or_default();
                            let (scale, dec, suf) = fmt_of(&stat0);
                            view! {
                                <div class="prop ench edit">
                                    <span>{move || en().map(|x| db().stat_name(&x.stat)).unwrap_or_default()}</span>
                                    <Stepper value=Signal::derive(move || en().map(|x| x.raw).unwrap_or(0.0))
                                        min=Signal::derive(move || p().map(|p| p.min).unwrap_or(0.0))
                                        max=Signal::derive(move || p().map(|p| p.max).unwrap_or(0.0))
                                        scale=scale decimals=dec suffix=suf signed=true
                                        on_set=Callback::new(move |v: f64| edit(lo, key, |e, _| if let Some(x) = e.enchants.get_mut(i) { x.raw = v }))/>
                                    <button class="x del" title="remove (drops rarity)" on:click=move |_| remove_enchant(lo, key, i)>"✕"</button>
                                </div>
                            }
                        }/>
                        {move || eq.get().map(|e| {
                            let f = fam(&e.item).unwrap();
                            let c = cap(f, &e.rarity);
                            let n = e.enchants.len();
                            let can_more = !addable(f, &e).is_empty();
                            let bump = if n >= c { rarity_for_more(f, &e.rarity, n + 1) } else { None };
                            let empty_slots = c.saturating_sub(n);
                            let label = if empty_slots > 0 {
                                format!("+ Add enchantment ({empty_slots} open)")
                            } else if let Some(r) = &bump {
                                format!("+ Add enchantment → {}", if r == "Legend" { "Legendary" } else { r })
                            } else {
                                String::new()
                            };
                            (can_more && !label.is_empty()).then(|| view! {
                                <button class="addench" on:click=move |_| adding.update(|a| *a = !*a)>{label}</button>
                            })
                        })}
                        {move || adding.get().then(add_panel)}
                    </div>

                    // ---- actions
                    <div class="ed-actions">
                        <button title="random base roll and enchant values" on:click=move |_| reroll_values(lo, key)>"🎲 Values"</button>
                        <button title="random enchantments for this rarity" on:click=move |_| reroll_all(lo, key)>"🎲 Enchants"</button>
                        <button on:click=move |_| max_out(lo, key)>"Max"</button>
                        <button class=move || if swapping.get() { "on" } else { "" } title="pick another item for this slot (Cancel keeps this one)" on:click=move |_| swapping.update(|s| *s = !*s)>"⇄ Change"</button>
                        <button class="ghost" on:click=move |_| lo.update(|l| { l.gear.remove(key); })>"Unequip"</button>
                    </div>
                    {move || swapping.get().then(swap_panel)}
                </div>
            }
            .into_any()
        }
    }
}
