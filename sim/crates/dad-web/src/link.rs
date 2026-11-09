//! Connection to DarkerLink (tools/DarkerLink): the desktop companion sends OCR'd item tooltips
//! from the game; we parse them and equip the item on the Attacker or Target.

use crate::items::slot_key_for;
use crate::{db, gear_fits};
use dad_core::stats::{all_slots, Equipped, Loadout};
use dad_core::tooltip::{parse_tooltip, OcrLine};
use leptos::prelude::*;
use serde::Deserialize;
use wasm_bindgen::prelude::*;

pub const LINK_URL: &str = "http://localhost:47321";

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    side: String,
    #[serde(default)]
    lines: Vec<OcrLine>,
}

/// Last item added through DarkerLink: (side, slot) — used to flash that equipment cell.
#[derive(Clone, Copy)]
pub struct LinkState {
    pub connected: RwSignal<bool>,
    pub last: RwSignal<Option<(&'static str, &'static str, u64)>>,
}

pub fn start(
    atk: RwSignal<Loadout>,
    tgt: RwSignal<Loadout>,
    sel_atk: RwSignal<Option<&'static str>>,
    sel_tgt: RwSignal<Option<&'static str>>,
) -> LinkState {
    let st = LinkState { connected: RwSignal::new(false), last: RwSignal::new(None) };
    provide_context(st);
    let Ok(es) = web_sys::EventSource::new(&format!("{LINK_URL}/events")) else { return st };

    let on_open = Closure::<dyn FnMut()>::new(move || st.connected.set(true));
    es.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    on_open.forget();
    // EventSource reconnects by itself; we only track the state
    let on_err = Closure::<dyn FnMut()>::new(move || st.connected.set(false));
    es.set_onerror(Some(on_err.as_ref().unchecked_ref()));
    on_err.forget();

    let on_msg = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
        st.connected.set(true);
        let Some(text) = ev.data().as_string() else { return };
        let Ok(p) = serde_json::from_str::<Payload>(&text) else { return };
        let (lo, sel, side_name, side): (RwSignal<Loadout>, RwSignal<Option<&'static str>>, &str, &'static str) =
            if p.side == "tgt" { (tgt, sel_tgt, "Target", "tgt") } else { (atk, sel_atk, "Attacker", "atk") };
        let (ok, msg) = match parse_tooltip(db(), &p.lines) {
            Err(e) => (false, format!("Couldn't read the item: {e}")),
            Ok(item) => {
                let fam = &db().items[&item.item];
                let rarity_label = if item.rarity == "Legend" { "Legendary" } else { item.rarity.as_str() };
                // the slot clicked in the equipment view wins, if the item goes there
                let selected = sel.get_untracked();
                let fits_selected = selected.filter(|k| all_slots().any(|(kk, _, s)| kk == k && Some(*s) == fam.slot.as_deref()));
                let mut slot_note = None;
                if let (Some(k), None) = (selected, fits_selected) {
                    let lbl = all_slots().find(|s| s.0 == k).map(|s| s.1).unwrap_or(k);
                    slot_note = Some(format!("{} doesn't go in the selected {lbl} slot", fam.name));
                }
                match fits_selected.or_else(|| lo.with_untracked(|l| slot_key_for(fam, l))) {
                    None => (false, format!("{} can't be equipped (no gear slot)", fam.name)),
                    Some(slot) => {
                        let n_ench = item.enchants.len();
                        lo.update(|l| {
                            l.gear.insert(slot.to_string(), Equipped { item: item.item.clone(), rarity: item.rarity.clone(), roll: item.roll, enchants: item.enchants.clone() });
                        });
                        let stamp = js_sys_now();
                        st.last.set(Some((side, slot, stamp)));
                        set_timeout(move || if st.last.get_untracked().map(|l| l.2) == Some(stamp) { st.last.set(None) }, std::time::Duration::from_millis(2600));
                        let class = lo.with_untracked(|l| l.class.clone());
                        let slot_label = all_slots().find(|s| s.0 == slot).map(|s| s.1).unwrap_or(slot);
                        let mut m = format!("Added {rarity_label} {} to {side_name} · {slot_label} ({n_ench} enchant{})", fam.name, if n_ench == 1 { "" } else { "s" });
                        if let Some(n) = &slot_note {
                            m.push_str(&format!("\n· {n}"));
                        }
                        if !gear_fits(fam, &class) {
                            m.push_str(&format!("\n⚠ This item does not fit {class} — equipped anyway"));
                        }
                        for n in &item.notes {
                            m.push_str(&format!("\n· {n}"));
                        }
                        (true, m)
                    }
                }
            }
        };
        report(ok, msg);
    });
    es.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
    on_msg.forget();
    std::mem::forget(es);
    st
}

fn js_sys_now() -> u64 {
    web_sys::window().and_then(|w| w.performance()).map(|p| p.now() as u64).unwrap_or(0)
}

/// Sends the outcome back so DarkerLink can show it as an in-game toast.
fn report(ok: bool, text: String) {
    leptos::task::spawn_local(async move {
        let body = serde_json::json!({ "ok": ok, "text": text }).to_string();
        // text/plain keeps this a "simple" request (no CORS preflight)
        if let Ok(req) = gloo_net::http::Request::post(&format!("{LINK_URL}/result")).header("Content-Type", "text/plain").body(body) {
            let _ = req.send().await;
        }
    });
}

/// Header badge: DarkerLink connected / not running.
#[component]
pub fn LinkBadge() -> impl IntoView {
    let st = use_context::<LinkState>();
    let on = move || st.map(|s| s.connected.get()).unwrap_or(false);
    view! {
        <span class=move || if on() { "link-badge on" } else { "link-badge" }
            title=move || if on() {
                "DarkerLink is running: hover an item in game and press Ctrl+Alt+A (Attacker) or Ctrl+Alt+T (Target)".to_string()
            } else {
                "DarkerLink is not running. Click to see what DarkerLink is".to_string()
            }>
            <span class="dot"></span>{move || if on() { "DarkerLink" } else { "DarkerLink offline" }}
        </span>
    }
}
