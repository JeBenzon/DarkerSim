//! Inline number control that looks like the card text: ◀ [value] ▶
//! Click the arrows, type a number, use ↑/↓ (Shift = ×10) or the mouse wheel.

use leptos::prelude::*;

#[component]
pub fn Stepper(
    /// current value in raw units
    value: Signal<f64>,
    min: Signal<f64>,
    max: Signal<f64>,
    /// raw units per click
    #[prop(default = 1.0)]
    step: f64,
    /// display = raw × scale
    #[prop(default = 1.0)]
    scale: f64,
    #[prop(default = 0)]
    decimals: usize,
    #[prop(default = "")]
    suffix: &'static str,
    /// show a "+" for positive values
    #[prop(default = false)]
    signed: bool,
    on_set: Callback<f64>,
) -> impl IntoView {
    let snap = move |v: f64| {
        let (lo, hi) = (min.get_untracked(), max.get_untracked());
        let v = (v / step).round() * step;
        v.clamp(lo.min(hi), hi.max(lo))
    };
    let set = move |v: f64| {
        let v = snap(v);
        if (v - value.get_untracked()).abs() > 1e-9 {
            on_set.run(v);
        }
    };
    let bump = move |dir: f64, big: bool| set(value.get_untracked() + dir * step * if big { 10.0 } else { 1.0 });
    let text = move || {
        let d = value.get() * scale;
        let s = format!("{:.*}", decimals, d);
        format!("{}{s}{suffix}", if signed && d > 0.0 { "+" } else { "" })
    };
    let at_min = move || value.get() <= min.get().min(max.get()) + 1e-9;
    let at_max = move || value.get() >= max.get().max(min.get()) - 1e-9;
    view! {
        <span class="stepper-ctl"
            on:wheel=move |ev| { ev.prevent_default(); bump(if ev.delta_y() < 0.0 { 1.0 } else { -1.0 }, ev.shift_key()); }>
            <button class="st-btn" tabindex="-1" disabled=at_min on:click=move |ev| { ev.stop_propagation(); bump(-1.0, ev.shift_key()); }>"◀"</button>
            <input class="st-val" type="text" inputmode="decimal"
                prop:value=text
                size=move || text().chars().count().max(2)
                on:click=move |ev| ev.stop_propagation()
                on:focus=move |ev| { let _ = event_target::<web_sys::HtmlInputElement>(&ev).select(); }
                on:keydown=move |ev| match ev.key().as_str() {
                    "ArrowUp" => { ev.prevent_default(); bump(1.0, ev.shift_key()); }
                    "ArrowDown" => { ev.prevent_default(); bump(-1.0, ev.shift_key()); }
                    "Enter" => { let _ = event_target::<web_sys::HtmlInputElement>(&ev).blur(); }
                    _ => {}
                }
                on:change=move |ev| {
                    let raw: String = event_target_value(&ev).chars().filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-').collect();
                    if let Ok(d) = raw.parse::<f64>() { set(d / scale); }
                    // re-render the formatted text even if the value did not change
                    let el = event_target::<web_sys::HtmlInputElement>(&ev);
                    el.set_value(&text());
                }/>
            <button class="st-btn" tabindex="-1" disabled=at_max on:click=move |ev| { ev.stop_propagation(); bump(1.0, ev.shift_key()); }>"▶"</button>
        </span>
    }
}
