//! "DarkerLink" tab: what the companion app does and how capturing an item works.
//! The app itself isn't published yet, so this page only describes it.

use leptos::prelude::*;

#[component]
pub fn CompanionPage() -> impl IntoView {
    let key = |k: &'static str| view! { <kbd>{k}</kbd> };
    let combo = move |a: &'static str| view! { <span class="dl-combo">{key("Ctrl")}"+"{key("Alt")}"+"{key(a)}</span> };
    view! {
        <section class="card dl-page">
            <h2>"DarkerLink"</h2>
            <p class="dl-status"><span class="dl-pill">"Not available for download yet"</span>
                "DarkerLink is still in development. When it's ready you'll be able to download it here."</p>
            <p class="dl-lead">
                "DarkerLink is a small Windows companion app for Darker Sim. It sits in the system tray while you play. "
                "Hover an item in Dark and Darker, press a hotkey, and the item, with its exact rolls, appears in the simulator. "
                "There's nothing to type in by hand."
            </p>

            <div class="dl-flow">
                <div class="dl-step-box"><b>"In game"</b><span>"Hover an item so its tooltip shows"</span></div>
                <span class="dl-arrow">"→"</span>
                <div class="dl-step-box"><b>{combo("A")}</b><span>"DarkerLink reads the tooltip"</span></div>
                <span class="dl-arrow">"→"</span>
                <div class="dl-step-box"><b>"Darker Sim"</b><span>"The item is equipped on your Attacker"</span></div>
            </div>

            <h3>"How it works"</h3>
            <ol class="dl-steps">
                <li>"Keep Darker Sim open in your browser and play the game in " <b>"Borderless"</b> " or " <b>"Windowed"</b> " mode."</li>
                <li>"Hover the item you want, in your inventory, the stash, a merchant or the trade window, so its tooltip is on screen."</li>
                <li>"Press " {combo("A")} " to send it to the " <b>"Attacker"</b> ", or " {combo("T")} " to send it to the " <b>"Target"</b> "."</li>
                <li>"DarkerLink takes a screenshot of the area around your cursor and reads the tooltip with Windows' built-in text recognition: "
                    "the item's name, rarity and every rolled stat."</li>
                <li>"Darker Sim matches the item and puts it in the right slot with your rolls. To choose the slot yourself, "
                    "for example weapon set 2 or your second ring, select the slot in the sim first."</li>
                <li>"A short message next to your cursor confirms what was added. You can capture item after item in a row."</li>
            </ol>

            <h3>"Safe by design"</h3>
            <ul class="dl-safe">
                <li>"It only looks at your screen, the same pixels you see."</li>
                <li>"It never reads or changes the game's memory, never injects anything, and never sends keys or clicks to the game."</li>
                <li>"It talks only to the Darker Sim page in your own browser on the same PC, nothing on the internet."</li>
            </ul>

            <p class="muted dl-foot">"The DarkerLink badge in the top bar turns green when the app is running and connected."</p>
        </section>
    }
}
