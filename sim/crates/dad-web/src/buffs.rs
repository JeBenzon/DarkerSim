//! Buff & debuff panel: active buffs as game icons, hover card with what they do,
//! and a picker popup whose top row is the current class's own buffs.

use crate::{db, stat_value};
use dad_core::data::Buff;
use dad_core::stats::Loadout;
use leptos::prelude::*;

#[derive(Clone, Copy)]
struct BuffTip {
    id: RwSignal<Option<String>>,
    pos: RwSignal<(f64, f64)>,
    /// the hovered buff is applied but its condition is not met (Brewmaster without Drunk)
    inactive: RwSignal<bool>,
}

pub fn provide_tip() {
    provide_context(BuffTip { id: RwSignal::new(None), pos: RwSignal::new((0.0, 0.0)), inactive: RwSignal::new(false) });
}

fn icon_view(b: Option<&Buff>, name: &str) -> AnyView {
    match b.and_then(|b| b.icon.clone()) {
        Some(i) => view! { <img src=format!("icons/{i}.webp") alt="" loading="lazy"/> }.into_any(),
        None => {
            let initials: String = name.split_whitespace().filter_map(|w| w.chars().next()).take(2).collect();
            view! { <span class="bf-noicon">{initials}</span> }.into_any()
        }
    }
}

/// One mod as the game words it: "+15% Action Speed", "-30% Move Speed Bonus".
fn mod_line(stat: &str, v: f64) -> String {
    let val = stat_value(stat, v);
    let val = if val.starts_with('-') || val.starts_with('+') { val } else { format!("+{val}") };
    format!("{val} {}", db().stat_name(stat))
}

/// Icon tile that shows the floating buff card on hover.
#[component]
fn BuffIcon(id: String, #[prop(optional)] big: bool, #[prop(optional)] inactive: bool) -> impl IntoView {
    let tip = use_context::<BuffTip>().expect("buff tip");
    let b = db().buffs.get(&id);
    let debuff = b.map(|b| b.debuff).unwrap_or(false);
    let name = b.map(|b| b.name.clone()).unwrap_or_else(|| id.clone());
    let (id_enter, id_leave) = (id.clone(), id.clone());
    view! {
        <span class=format!("bf-icon {} {} {}", if debuff { "debuff" } else { "buff" }, if big { "big" } else { "" }, if inactive { "inactive" } else { "" })
            on:mouseenter=move |ev| { tip.inactive.set(inactive); tip.id.set(Some(id_enter.clone())); tip.pos.set((ev.client_x() as f64, ev.client_y() as f64)); }
            on:mousemove=move |ev| tip.pos.set((ev.client_x() as f64, ev.client_y() as f64))
            on:mouseleave=move |_| if tip.id.get_untracked().as_deref() == Some(id_leave.as_str()) { tip.id.set(None) }>
            {icon_view(b, &name)}
        </span>
    }
}

/// The hover card (rendered once, follows the mouse).
#[component]
pub fn BuffFloatTip() -> impl IntoView {
    let tip = use_context::<BuffTip>().expect("buff tip");
    move || {
        let id = tip.id.get()?;
        let b = db().buffs.get(&id)?;
        let (x, y) = tip.pos.get();
        let vw = web_sys::window().and_then(|w| w.inner_width().ok()).and_then(|v| v.as_f64()).unwrap_or(1600.0);
        let left = if x + 330.0 > vw { x - 320.0 } else { x + 18.0 };
        let kind = if b.debuff { "Debuff" } else { "Buff" };
        let dur = b.duration.filter(|d| *d > 0.0).map(|d| format!(" · {}s", (d / 100.0).round() / 10.0)).unwrap_or_else(|| " · while active".into());
        let srcs: Vec<String> = b.sources.iter().map(|s| format!("{}: {}", s.kind, dad_core::humanize(&s.id))).take(3).collect();
        let classes = if b.classes.is_empty() { String::new() } else { b.classes.join(", ") };
        Some(view! {
            <div class=format!("bf-tip {}", kind.to_lowercase()) style=format!("left:{left:.0}px; top:{:.0}px", y + 16.0)>
                <div class="bf-tip-head">
                    {icon_view(Some(b), &b.name)}
                    <div>
                        <div class="bf-tip-name">{b.name.clone()}</div>
                        <div class="bf-tip-kind">{format!("{kind}{dur}")}</div>
                    </div>
                </div>
                <div class="bf-tip-mods">
                    {if b.mods.is_empty() {
                        view! { <div class="muted">"No stat change in the data (effect is in game code)"</div> }.into_any()
                    } else {
                        b.mods.iter().map(|m| {
                            let neg = m.value < 0.0;
                            view! { <div class=if neg { "neg" } else { "pos" }>{mod_line(&m.stat, m.value)}</div> }
                        }).collect_view().into_any()
                    }}
                </div>
                {tip.inactive.get().then(|| view! { <div class="bf-tip-off">"Inactive — its condition is not met, so it adds nothing right now"</div> })}
                {b.requires.as_ref().map(|r| view! { <div class="bf-tip-req">{format!("Needs: {}", dad_core::humanize(r))}</div> })}
                {b.note.clone().map(|n| view! { <div class="bf-tip-note2">{n}</div> })}
                <div class="bf-tip-src">{srcs.join(" · ")}</div>
                {(!classes.is_empty()).then(|| view! { <div class="bf-tip-src">{format!("Class: {classes}")}</div> })}
                {(b.confidence != "verified").then(|| view! { <div class="bf-tip-note">"Values inferred from the files — not confirmed in game"</div> })}
            </div>
        })
    }
}

/// Picker groups: (title, ids). The current class's own buffs come first.
fn groups(class: &str) -> Vec<(String, Vec<String>)> {
    let mut own = vec![];
    let mut items = vec![];
    let mut shrines = vec![];
    let mut others = vec![];
    let mut debuffs = vec![];
    let mut misc = vec![];
    for (id, b) in &db().buffs {
        if b.mods.is_empty() {
            continue;
        }
        let kind = b.sources.first().map(|s| s.kind.as_str()).unwrap_or("Other");
        let class_src = matches!(kind, "Skill" | "Spell" | "Perk" | "Bard song" | "Shape Shift");
        if b.debuff {
            debuffs.push(id.clone());
        } else if class_src && b.classes.iter().any(|c| c == class) {
            own.push(id.clone());
        } else if class_src {
            others.push(id.clone());
        } else if kind == "Item" {
            items.push(id.clone());
        } else if matches!(kind, "Shrine / Religion" | "Luck") {
            shrines.push(id.clone());
        } else {
            misc.push(id.clone());
        }
    }
    // alphabetical, but "(Poor)" … "(Unique)" variants of one potion in rarity order
    const RAR: [&str; 8] = ["(Poor)", "(Common)", "(Uncommon)", "(Rare)", "(Epic)", "(Legendary)", "(Unique)", "(Artifact)"];
    let key = |id: &String| {
        let n = db().buffs[id].name.clone();
        match RAR.iter().position(|r| n.ends_with(r)) {
            Some(i) => (n[..n.len() - RAR[i].len()].to_string(), i),
            None => (n, 0),
        }
    };
    for v in [&mut own, &mut items, &mut shrines, &mut others, &mut debuffs, &mut misc] {
        v.sort_by_key(key);
    }
    let cname = db().classes.get(class).map(|c| c.name.clone()).unwrap_or_else(|| class.to_string());
    vec![
        (format!("{cname} buffs"), own),
        ("Party buffs from other classes".into(), others),
        ("Debuffs".into(), debuffs),
        ("Potions & items".into(), items),
        ("Shrines, blessings & luck".into(), shrines),
        ("Other".into(), misc),
    ]
}

#[component]
pub fn BuffPanel(lo: RwSignal<Loadout>) -> impl IntoView {
    let open = RwSignal::new(false);
    let query = RwSignal::new(String::new());
    let search_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if open.get() {
            if let Some(el) = search_ref.get() {
                let _ = el.focus();
            }
        }
    });
    let toggle = move |id: String| {
        lo.update(|l| {
            if let Some(i) = l.buffs.iter().position(|x| *x == id) {
                l.buffs.remove(i);
            } else {
                l.buffs.push(id);
            }
        })
    };
    let close = move || {
        open.set(false);
        query.set(String::new());
    };
    view! {
        <div class="buffbox">
            <div class="bb-head">
                <span class="bb-title">"Buffs & Debuffs"</span>
                <span class="muted small">{move || { let n = lo.with(|l| l.buffs.len()); if n == 0 { "none active".to_string() } else { format!("{n} active · click to remove") } }}</span>
                <button class="bb-add" on:click=move |_| if open.get_untracked() { close() } else { open.set(true) }>"+ Add"</button>
            </div>
            <div class="bb-row">
                {move || {
                    let ids = lo.get().buffs;
                    if ids.is_empty() {
                        return view! { <button class="bb-empty" on:click=move |_| open.set(true)>"No buffs — click to add potions, songs, shrines…"</button> }.into_any();
                    }
                    ids.into_iter().map(|id| {
                        let id2 = id.clone();
                        let off = !lo.with(|l| dad_core::stats::buff_active(db(), l, &id));
                        view! { <button class=if off { "bb-active off" } else { "bb-active" } on:click=move |_| toggle(id2.clone())><BuffIcon id=id big=true inactive=off/></button> }
                    }).collect_view().into_any()
                }}
            </div>
            <Show when=move || open.get()>
                <div class="ap-backdrop" on:click=move |_| close()></div>
                <div class="ap-pop bb-pop" on:keydown=move |ev| if ev.key() == "Escape" { close() }>
                    <input class="ap-search" placeholder="search buffs, potions, songs, shrines…" node_ref=search_ref
                        prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
                    {move || {
                        let q = query.get().to_lowercase();
                        let active = lo.with(|l| l.buffs.clone());
                        let class = lo.with(|l| l.class.clone());
                        groups(&class).into_iter().enumerate().filter_map(|(gi, (title, ids))| {
                            let ids: Vec<String> = ids.into_iter().filter(|id| {
                                q.is_empty() || db().buffs[id].name.to_lowercase().contains(&q)
                                    || db().buffs[id].mods.iter().any(|m| db().stat_name(&m.stat).to_lowercase().contains(&q))
                            }).collect();
                            (!ids.is_empty()).then(|| view! {
                                <div class=if gi == 0 { "ap-group own" } else { "ap-group" }>
                                    <div class="ap-label">{format!("{title} ({})", ids.len())}</div>
                                    <div class="bb-grid">
                                        {ids.into_iter().map(|id| {
                                            let on = active.contains(&id);
                                            let name = db().buffs[&id].name.clone();
                                            let req = db().buffs[&id].requires.clone();
                                            let id2 = id.clone();
                                            view! {
                                                <button class=if on { "bb-tile on" } else { "bb-tile" } on:click=move |_| toggle(id2.clone())>
                                                    <BuffIcon id=id/>
                                                    <span class="bb-name">{name}</span>
                                                    {req.map(|r| view! { <span class="bb-req">{format!("needs {}", dad_core::humanize(&r))}</span> })}
                                                    {on.then(|| view! { <span class="bb-check">"✓"</span> })}
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
