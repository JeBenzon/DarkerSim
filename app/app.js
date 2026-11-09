/* Darker Companion - all data comes from data/game.js, built from the installed game files. */
(() => {
"use strict";
const D = window.DAD_DATA;
if (!D) { document.body.innerHTML = "<p style='padding:20px'>No data. Run <code>pwsh scripts/update.ps1</code> first.</p>"; return; }
const PT = D.propertyTypes;
const C = D.constants;
const $ = (s, el = document) => el.querySelector(s);
const esc = s => String(s ?? "").replace(/[&<>"']/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
const store = {
  get(k, d) { try { const v = localStorage.getItem("dadc." + k); return v ? JSON.parse(v) : d; } catch { return d; } },
  set(k, v) { try { localStorage.setItem("dadc." + k, JSON.stringify(v)); } catch { /* storage unavailable */ } },
};

// ---------- formatting ----------
const isPct = stat => !!(PT[stat] && PT[stat].pct);
const ratio = stat => (PT[stat] && PT[stat].ratio) || 1;
const statName = stat => (PT[stat] && PT[stat].name) || stat.replace(/([a-z])([A-Z])/g, "$1 $2");
const num = (v, d = 2) => (Math.round(v * 10 ** d) / 10 ** d).toString();
const signed = (v, d = 2) => (v > 0 ? "+" : "") + num(v, d);
const pct = (f, d = 1) => num(f * 100, d) + "%";
const spct = (f, d = 1) => (f > 0 ? "+" : "") + pct(f, d);
function propText(p) {
  const r = ratio(p.stat);
  const f = v => isPct(p.stat) ? num(v * r * 100, 1) + "%" : num(v, 2);
  return `${statName(p.stat)} ${p.min === p.max ? f(p.min) : f(p.min) + "~" + f(p.max)}`;
}
const rarityClass = r => "r-" + r;
const itemName = id => (D.items[id] && D.items[id].name) || id;

// ---------- curves ----------
function curve(key, x) {
  const k = D.curves[key];
  if (!k || !k.length) return 0;
  if (x <= k[0][0]) return k[0][1];
  for (let i = 1; i < k.length; i++) {
    if (x <= k[i][0]) {
      const [x0, y0] = k[i - 1], [x1, y1] = k[i];
      return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
    }
  }
  return k[k.length - 1][1];
}

// ---------- tabs ----------
const tabRenderers = {};
function showTab(name) {
  document.querySelectorAll("#tabs button").forEach(b => b.classList.toggle("active", b.dataset.tab === name));
  document.querySelectorAll(".tab").forEach(t => t.classList.toggle("active", t.id === "tab-" + name));
  store.set("tab", name);
  tabRenderers[name] && tabRenderers[name]();
}
$("#tabs").addEventListener("click", e => { const b = e.target.closest("button"); if (b) showTab(b.dataset.tab); });

const m = D.meta;
$("#buildInfo").textContent = `Game ${m.build} · ${m.counts.itemFamilies} items · ${m.counts.recipes} recipes · extracted ${m.generatedAt.slice(0, 10)}`;

// =====================================================================
// LOADOUT + STAT ENGINE
// =====================================================================
const SLOTS = [
  { key: "weapon", label: "Weapon", slots: ["Primary"] },
  { key: "offhand", label: "Off-hand", slots: ["Secondary"] },
  { key: "head", label: "Head", slots: ["Head"] },
  { key: "chest", label: "Chest", slots: ["Chest"] },
  { key: "legs", label: "Legs", slots: ["Legs"] },
  { key: "hands", label: "Hands", slots: ["Hands"] },
  { key: "foot", label: "Feet", slots: ["Foot"] },
  { key: "back", label: "Cloak", slots: ["Back"] },
  { key: "necklace", label: "Necklace", slots: ["Necklace"] },
  { key: "ring1", label: "Ring", slots: ["Ring"] },
  { key: "ring2", label: "Ring", slots: ["Ring"] },
];
const WEAPON_DMG = ["PhysicalWeaponDamage", "MagicalWeaponDamage"];
const classIds = Object.keys(D.classes);
let L = store.get("loadout", null) || { cls: classIds.includes("Fighter") ? "Fighter" : classIds[0], roll: "max", classOnly: true, gear: {}, extras: [] };

const familiesBySlot = {};
for (const f of Object.values(D.items)) {
  if (!f.slot) continue;
  if (!Object.values(f.variants).some(v => v.props.length)) continue;
  (familiesBySlot[f.slot] ||= []).push(f);
}
Object.values(familiesBySlot).forEach(a => a.sort((x, y) => x.name.localeCompare(y.name)));

const roll = (p, mode) => mode === "min" ? p.min : mode === "avg" ? (p.min + p.max) / 2 : p.max;
const toInternal = (stat, displayVal) => isPct(stat) ? displayVal / 100 : displayVal; // display % -> fraction
const propValue = (p, mode) => isPct(p.stat) ? roll(p, mode) * ratio(p.stat) : roll(p, mode);

function gearTotals(lo) {
  const g = {};
  const add = (s, v) => { g[s] = (g[s] || 0) + v; };
  for (const s of SLOTS) {
    const sel = lo.gear[s.key];
    const fam = sel && D.items[sel.item];
    const v = fam && fam.variants[sel.rarity];
    if (!v) continue;
    for (const p of v.props) if (!WEAPON_DMG.includes(p.stat)) add(p.stat, propValue(p, lo.roll));
    for (const e of sel.ench || []) if (e.stat && e.val) add(e.stat, toInternal(e.stat, +e.val));
  }
  for (const e of lo.extras || []) if (e.stat && e.val) add(e.stat, toInternal(e.stat, +e.val));
  return g;
}

function computeStats(lo, attrDelta = {}) {
  const cls = D.classes[lo.cls];
  const g = gearTotals(lo);
  const G = k => g[k] || 0;
  const A = {};
  for (const a of D.attributes) A[a] = cls.attributes[a] + G(a) + G("Primitive") + (attrDelta[a] || 0);
  const s = { attrs: A, gear: g };
  s.PhysicalPower = A.Strength + G("PhysicalPower");
  s.PhysicalDamageMod = curve("CT_PhysicalPower.PhysicalDamageMod", s.PhysicalPower) + G("PhysicalDamageBonus");
  s.MagicalPower = curve("CT_Will.MagicalPower", A.Will) + G("MagicalPower");
  s.MagicalDamageMod = curve("CT_MagicalPower.MagicalDamageMod", s.MagicalPower) + G("MagicalDamageBonus");
  s.HealthRating = A.Strength * 0.25 + A.Vigor * 0.75;
  s.BaseHealth = curve("CT_MaxHealthBase.MaxHealthBase", s.HealthRating);
  s.MaxHealth = Math.ceil(s.BaseHealth * (1 + G("MaxHealthBonus")) + G("MaxHealthAdd") + (m.notes.classBaseHealthAdd || 0));
  s.HealthRecoveryMod = curve("CT_RecoveryMod.HealthRecoveryMod", A.Vigor);
  const msCap = C[lo.cls + "MaxMoveSpeed"] || C.CharacterMaxMoveSpeed || 500;
  s.MoveSpeedRaw = ((cls.moveSpeed || C.CharacterBaseMoveSpeed || 300) + curve("CT_Agility.MoveSpeedBase", A.Agility) + G("MoveSpeed") + G("MoveSpeedAdd")) * (1 + G("MoveSpeedBonus"));
  s.MoveSpeed = Math.min(s.MoveSpeedRaw, msCap);
  s.ActionRating = A.Agility * 0.25 + A.Dexterity * 0.75;
  s.ActionSpeed = curve("CT_ActionSpeed.ActionSpeed", s.ActionRating) + G("ActionSpeed");
  s.ManualDexterity = curve("CT_Dexterity.ManualDexterity", A.Dexterity) + G("ManualDexterity");
  s.ItemEquipSpeed = curve("CT_Dexterity.ItemEquipSpeed", A.Dexterity) + G("ItemEquipSpeed");
  s.InteractRating = A.Dexterity * 0.25 + A.Resourcefulness * 0.75;
  s.RegularInteractionSpeed = curve("CT_RegularInteractionSpeedBase.RegularInteractionSpeed", s.InteractRating) + G("RegularInteractionSpeed");
  s.MagicalInteractionSpeed = curve("CT_Will.MagicalInteractionSpeed", A.Will) + G("MagicalInteractionSpeed");
  s.SpellCastingSpeed = Math.max(C.Attribute_MinSpellCastingSpeed ?? -0.95, Math.min(C.Attribute_MaxSpellCastingSpeed ?? 2, curve("CT_Knowledge.SpellCastingSpeed", A.Knowledge) + G("SpellCastingSpeed")));
  s.MemoryCapacity = Math.ceil(curve("CT_Knowledge.MemoryCapacity", A.Knowledge) * (1 + G("MemoryCapacityBonus"))) + G("MemoryCapacityAdd");
  s.MemoryRecoveryMod = curve("CT_RecoveryMod.MemoryRecoveryMod", A.Knowledge);
  s.CooldownReduction = Math.min(C.Attribute_MaxCooldownReductionMod ?? 0.65, curve("CT_Resourcefulness.CooldownReduction", A.Resourcefulness) + G("CooldownReductionBonus"));
  s.Persuasiveness = curve("CT_Resourcefulness.Persuasiveness", A.Resourcefulness) + G("Persuasiveness");
  s.BuffDurationMod = curve("CT_Will.BuffDurationMod", A.Will) + G("BuffDurationBonus");
  s.DebuffDurationMod = Math.max(C.Attribute_MinDebuffDurationMod ?? -0.95, curve("CT_Will.DebuffDurationMod", A.Will) + G("DebuffDurationBonus"));
  s.ArmorRating = G("ArmorRating") + G("ArmorRatingAdd");
  s.PhysicalReduction = Math.min(C.Attribute_MaxPhysicalDamageReduction ?? 0.95, curve("CT_ArmorRating.PhysicalReduction", s.ArmorRating) + G("PhysicalDamageReduction"));
  s.MagicResistance = curve("CT_Will.MagicResistance", A.Will) + G("MagicRegistance");
  s.MagicalReduction = Math.min(C.Attribute_MaxMagicalDamageReduction ?? 0.95, curve("CT_MagicResistance.MagicalReduction", s.MagicResistance) + G("MagicalDamageReduction"));
  return s;
}

// [key, label, format, source note]
const SHEET = [
  ["Offense", [
    ["PhysicalPower", "Physical Power", "n"], ["PhysicalDamageMod", "Physical Power Bonus", "p"],
    ["MagicalPower", "Magical Power", "n"], ["MagicalDamageMod", "Magical Power Bonus", "p"],
    ["g:PhysicalWeaponDamageAdd", "Additional Weapon Damage", "n"], ["g:PhysicalDamageAdd", "Additional Physical Damage", "n"],
    ["g:MagicalDamageAdd", "Additional Magical Damage", "n"], ["g:PhysicalDamageTrue", "True Physical Damage", "n"],
    ["g:MagicalDamageTrue", "True Magical Damage", "n"], ["g:ArmorPenetration", "Armor Penetration", "p"],
    ["g:MagicPenetration", "Magic Penetration", "p"], ["g:HeadshotDamageMod", "Headshot Damage Bonus", "p"],
  ]],
  ["Defense", [
    ["MaxHealth", "Max Health", "n0"], ["ArmorRating", "Armor Rating", "n"], ["PhysicalReduction", "Physical Damage Reduction", "p"],
    ["MagicResistance", "Magic Resistance", "n"], ["MagicalReduction", "Magical Damage Reduction", "p"],
    ["g:ProjectileReductionMod", "Projectile Damage Reduction", "p"], ["g:HeadshotReductionMod", "Headshot Damage Reduction", "p"],
    ["HealthRecoveryMod", "Health Recovery Bonus", "p"],
  ]],
  ["Speed & utility", [
    ["MoveSpeed", "Move Speed", "n1"], ["ActionSpeed", "Action Speed", "p"], ["SpellCastingSpeed", "Spell Casting Speed", "p"],
    ["RegularInteractionSpeed", "Regular Interaction Speed", "p"], ["MagicalInteractionSpeed", "Magical Interaction Speed", "p"],
    ["ManualDexterity", "Manual Dexterity", "p"], ["ItemEquipSpeed", "Item Equip Speed", "p"],
    ["CooldownReduction", "Cooldown Reduction", "p"], ["BuffDurationMod", "Buff Duration", "p"], ["DebuffDurationMod", "Debuff Duration", "p"],
    ["MemoryCapacity", "Memory Capacity", "n0"], ["MemoryRecoveryMod", "Memory Recovery", "p"], ["Persuasiveness", "Persuasiveness", "n"],
    ["g:Luck", "Luck", "n"],
  ]],
];
const getStat = (s, key) => key.startsWith("g:") ? (s.gear[key.slice(2)] || 0) : s[key];
const fmtVal = (v, f) => f === "p" ? pct(v) : f === "n0" ? num(v, 0) : f === "n1" ? num(v, 1) : num(v, 2);
const fmtDelta = (v, f) => f === "p" ? spct(v, 2) : signed(v, f === "n0" ? 0 : 2);

function slotOptions(slot) {
  const fams = slot.slots.flatMap(sl => familiesBySlot[sl] || [])
    .filter(f => !L.classOnly || !f.classes.length || f.classes.includes(L.cls));
  return `<option value="">— empty —</option>` + fams.map(f => `<option value="${esc(f.id)}">${esc(f.name)}</option>`).join("");
}

function renderSlots() {
  $("#classSel").innerHTML = classIds.map(c => `<option value="${c}">${esc(D.classes[c].name)}</option>`).join("");
  $("#classSel").value = L.cls;
  $("#rollSel").value = L.roll;
  $("#classOnly").checked = L.classOnly;
  $("#slots").innerHTML = SLOTS.map(s => `<div class="slot" data-slot="${s.key}"><div class="slot-name">${s.label}</div><div class="slot-body"></div></div>`).join("");
  SLOTS.forEach(renderSlot);
  renderExtras();
}

function renderSlot(s) {
  const body = $(`.slot[data-slot="${s.key}"] .slot-body`);
  const sel = L.gear[s.key] || {};
  const fam = D.items[sel.item];
  const rarities = fam ? Object.keys(fam.variants) : [];
  const v = fam && fam.variants[sel.rarity];
  const pool = fam && fam.secondaryPool ? D.secondaryPools[fam.secondaryPool] || [] : [];
  const poolOpts = `<option value="">— enchant —</option>` + pool.map(p => `<option value="${p.stat}">${esc(statName(p.stat))}</option>`).join("");
  body.innerHTML = `
    <div class="row">
      <select class="item">${slotOptions(s)}</select>
      ${fam ? `<select class="rarity">${rarities.map(r => `<option class="${rarityClass(r)}" value="${r}">${r}</option>`).join("")}</select>` : ""}
    </div>
    ${v ? `<div class="props">${v.props.map(propText).map(esc).join(" · ") || "no base stats"}</div>` : ""}
    ${fam ? (sel.ench || []).map((e, i) => `
      <div class="ench" data-i="${i}">
        <select class="ench-stat">${poolOpts}</select>
        <input type="number" class="ench-val" step="0.1" value="${esc(e.val)}">
        <span class="muted small">${e.stat ? (isPct(e.stat) ? "%" : "") + " " + esc(poolHint(pool, e.stat)) : ""}</span>
        <button class="x ench-del" title="Remove">✕</button>
      </div>`).join("") + (pool.length ? `<button class="ghost ench-add small">+ enchant</button>` : "") : ""}`;
  body.querySelector(".item").value = sel.item || "";
  if (fam) body.querySelector(".rarity").value = sel.rarity;
  (sel.ench || []).forEach((e, i) => { body.querySelector(`.ench[data-i="${i}"] .ench-stat`).value = e.stat || ""; });
}

function poolHint(pool, stat) {
  const p = pool.find(x => x.stat === stat);
  if (!p) return "";
  const r = isPct(stat) ? ratio(stat) * 100 : 1;
  return `pool ${num(p.min * r, 1)}~${num(p.max * r, 1)}`;
}

function poolMax(fam, stat) {
  const p = (D.secondaryPools[fam.secondaryPool] || []).find(x => x.stat === stat);
  if (!p) return 0;
  return isPct(stat) ? num(p.max * ratio(stat) * 100, 1) : p.max;
}

$("#slots").addEventListener("change", e => {
  const slotEl = e.target.closest(".slot"); if (!slotEl) return;
  const key = slotEl.dataset.slot, s = SLOTS.find(x => x.key === key);
  const sel = L.gear[key] ||= {};
  if (e.target.classList.contains("item")) {
    sel.item = e.target.value; sel.ench = [];
    const fam = D.items[sel.item];
    if (!fam) { delete L.gear[key]; } else {
      const rs = Object.keys(fam.variants);
      sel.rarity = rs.includes(sel.rarity) ? sel.rarity : (rs.includes("Epic") ? "Epic" : rs[rs.length - 1]);
    }
  } else if (e.target.classList.contains("rarity")) sel.rarity = e.target.value;
  else if (e.target.classList.contains("ench-stat")) {
    const i = +e.target.closest(".ench").dataset.i;
    sel.ench[i].stat = e.target.value;
    sel.ench[i].val = poolMax(D.items[sel.item], e.target.value);
  } else if (e.target.classList.contains("ench-val")) sel.ench[+e.target.closest(".ench").dataset.i].val = e.target.value;
  renderSlot(s); saveAndCompute();
});
$("#slots").addEventListener("click", e => {
  const slotEl = e.target.closest(".slot"); if (!slotEl) return;
  const key = slotEl.dataset.slot, sel = L.gear[key];
  if (e.target.classList.contains("ench-add")) (sel.ench ||= []).push({ stat: "", val: "" });
  else if (e.target.classList.contains("ench-del")) sel.ench.splice(+e.target.closest(".ench").dataset.i, 1);
  else return;
  renderSlot(SLOTS.find(x => x.key === key)); saveAndCompute();
});
$("#classSel").addEventListener("change", e => { L.cls = e.target.value; renderSlots(); saveAndCompute(); });
$("#rollSel").addEventListener("change", e => { L.roll = e.target.value; SLOTS.forEach(renderSlot); saveAndCompute(); });
$("#classOnly").addEventListener("change", e => { L.classOnly = e.target.checked; SLOTS.forEach(renderSlot); saveAndCompute(); });
$("#resetLoadout").addEventListener("click", () => { L.gear = {}; L.extras = []; renderSlots(); saveAndCompute(); });

const EXTRA_STATS = [...D.attributes, ...Object.keys(PT).filter(k => !D.attributes.includes(k) && !/^(Perk|Skill|Spell|Unidentified)/.test(k))];
function renderExtras() {
  $("#extraRows").innerHTML = (L.extras || []).map((e, i) => `
    <div class="ench" data-i="${i}">
      <select class="ex-stat">${EXTRA_STATS.map(s => `<option value="${s}">${esc(statName(s))}${isPct(s) ? " (%)" : ""}</option>`).join("")}</select>
      <input type="number" class="ex-val" step="0.1" value="${esc(e.val)}">
      <button class="x ex-del">✕</button>
    </div>`).join("");
  (L.extras || []).forEach((e, i) => { $(`#extraRows .ench[data-i="${i}"] .ex-stat`).value = e.stat; });
}
$("#addExtra").addEventListener("click", () => { (L.extras ||= []).push({ stat: "Strength", val: 0 }); renderExtras(); saveAndCompute(); });
$("#extraRows").addEventListener("input", e => {
  const i = +e.target.closest(".ench").dataset.i;
  if (e.target.classList.contains("ex-stat")) L.extras[i].stat = e.target.value;
  if (e.target.classList.contains("ex-val")) L.extras[i].val = e.target.value;
  saveAndCompute();
});
$("#extraRows").addEventListener("click", e => {
  if (!e.target.classList.contains("ex-del")) return;
  L.extras.splice(+e.target.closest(".ench").dataset.i, 1); renderExtras(); saveAndCompute();
});

function saveAndCompute() { store.set("loadout", L); renderSheet(); }

function renderSheet() {
  const s = computeStats(L);
  const base = D.classes[L.cls].attributes;
  $("#attrSheet").innerHTML = D.attributes.map(a => {
    const d = s.attrs[a] - base[a];
    return `<div class="attr"><span>${a}</span><b>${num(s.attrs[a], 1)}</b>${d ? `<i>${signed(d, 1)}</i>` : ""}</div>`;
  }).join("");
  $("#statSheet").innerHTML = SHEET.map(([title, rows]) => `
    <h3>${title}</h3><table>${rows.map(([k, label, f]) => {
      const v = getStat(s, k);
      if (k.startsWith("g:") && !v) return "";
      let extra = "";
      if (k === "MoveSpeed" && s.MoveSpeedRaw > s.MoveSpeed) extra = ` <span class="bad small">(capped from ${num(s.MoveSpeedRaw, 1)})</span>`;
      return `<tr><td>${label}${extra}</td><td class="num">${fmtVal(v, f)}</td></tr>`;
    }).join("")}</table>`).join("");

  // marginal value of +1 of each attribute
  const rows = SHEET.flatMap(([, r]) => r).filter(([k]) => !k.startsWith("g:"));
  $("#marginal").innerHTML = `<div class="table-wrap"><table><tr><th>+1</th><th>gives</th></tr>${D.attributes.map(a => {
    const s2 = computeStats(L, { [a]: 1 });
    const gains = rows.map(([k, label, f]) => [label, s2[k] - s[k], f]).filter(([, d]) => Math.abs(d) > 1e-6);
    return `<tr><td>${a}</td><td>${gains.map(([label, d, f]) => `<span class="pill">${esc(label)} <b class="${d > 0 ? "good" : "bad"}">${fmtDelta(d, f)}</b></span>`).join("") || `<span class="muted">nothing</span>`}</td></tr>`;
  }).join("")}</table></div>`;
}
tabRenderers.loadout = renderSheet;

// =====================================================================
// DAMAGE BENCH
// =====================================================================
const HIT = [["Head", "Hitbox_Head"], ["Body", "Hitbox_Body"], ["Arm", "Hitbox_Arm"], ["Hand", "Hitbox_Hand"], ["Leg", "Hitbox_Leg"], ["Foot", "Hitbox_Foot"]];
const PRESETS = [
  ["Custom", null, null], ["Naked (0 AR)", 0, 0], ["Cloth (~40 AR)", 40, 60], ["Leather (~100 AR)", 100, 40],
  ["Plate (~200 AR)", 200, 30], ["Heavy plate (~280 AR)", 280, 20], ["Mirror: my own loadout", "self", "self"],
];
$("#tgtPreset").innerHTML = PRESETS.map((p, i) => `<option value="${i}">${p[0]}</option>`).join("");
$("#tgtPreset").addEventListener("change", e => {
  const p = PRESETS[+e.target.value];
  if (p[1] === "self") { const s = computeStats(L); $("#tgtAR").value = num(s.ArmorRating, 1); $("#tgtMR").value = num(s.MagicResistance, 1); }
  else if (p[1] !== null) { $("#tgtAR").value = p[1]; $("#tgtMR").value = p[2]; }
  renderDamage();
});
["#dmgWeapon", "#dmgZone", "#dmgBuff", "#dmgPen", "#tgtAR", "#tgtMR", "#tgtPDR", "#tgtProj", "#tgtHS"].forEach(id => $(id).addEventListener("input", e => {
  if (id === "#dmgWeapon") fillZones();
  if (id === "#tgtAR" || id === "#tgtMR") $("#tgtPreset").value = 0;
  renderDamage();
}));

function equippedWeapons() {
  return ["weapon", "offhand"].map(k => [k, L.gear[k]]).filter(([, g]) => g && D.items[g.item])
    .filter(([, g]) => D.items[g.item].variants[g.rarity].props.some(p => WEAPON_DMG.includes(p.stat)));
}
function fillWeapons() {
  const ws = equippedWeapons();
  $("#dmgWeapon").innerHTML = ws.length ? ws.map(([k, g]) => `<option value="${k}">${esc(itemName(g.item))} (${g.rarity})</option>`).join("")
    : `<option value="">Equip a weapon in Loadout first</option>`;
  fillZones();
}
function fillZones() {
  const g = L.gear[$("#dmgWeapon").value];
  const fam = g && D.items[g.item];
  const n = fam && fam.attacks ? Math.max(...fam.attacks.map(a => a.zones.length)) : 1;
  const prev = $("#dmgZone").value;
  $("#dmgZone").innerHTML = Array.from({ length: n }, (_, i) => `<option value="${i}">${i === 0 ? "Sweet spot" : "Zone " + (i + 1)}</option>`).join("");
  if (prev && +prev < n) $("#dmgZone").value = prev;
}

function renderDamage() {
  const key = $("#dmgWeapon").value, g = L.gear[key];
  if (!g) { $("#dmgTable").innerHTML = `<p class="muted">Equip a weapon on the Loadout tab.</p>`; $("#dmgNotes").textContent = ""; return; }
  const fam = D.items[g.item], v = fam.variants[g.rarity];
  const s = computeStats(L);
  const G = k => s.gear[k] || 0;
  const physP = v.props.find(p => p.stat === "PhysicalWeaponDamage");
  const magP = v.props.find(p => p.stat === "MagicalWeaponDamage");
  const magical = !physP && !!magP;
  const base = roll(physP || magP, L.roll);
  const buff = +$("#dmgBuff").value || 0;
  const zoneI = +$("#dmgZone").value || 0;
  const gearWD = magical ? 0 : G("PhysicalWeaponDamageAdd");
  const power = magical ? s.MagicalDamageMod : s.PhysicalDamageMod;
  const add = magical ? G("MagicalDamageAdd") : G("PhysicalDamageAdd");
  const tru = magical ? G("MagicalDamageTrue") : G("PhysicalDamageTrue");
  const pen = (magical ? G("MagicPenetration") : G("ArmorPenetration")) + (+$("#dmgPen").value || 0) / 100;
  const tAR = +$("#tgtAR").value || 0, tMR = +$("#tgtMR").value || 0;
  const tDR = magical ? curve("CT_MagicResistance.MagicalReduction", tMR) : curve("CT_ArmorRating.PhysicalReduction", tAR) + (+$("#tgtPDR").value || 0) / 100;
  const drMult = Math.max(1 - tDR * (1 - Math.min(pen, 1)), 1 - tDR);
  const proj = 1 - (+$("#tgtProj").value || 0) / 100;
  const hsBonus = G("HeadshotDamageMod") - (+$("#tgtHS").value || 0) / 100;
  const hitMult = (h) => (C[h[1]] ?? 100) / 100 + (h[0] === "Head" ? hsBonus : 0);
  const attacks = fam.attacks && fam.attacks.length ? fam.attacks : [{ name: "Hit", combo: 1, zones: [1] }];
  const dmg = (a, h) => {
    const zone = a.zones[Math.min(zoneI, a.zones.length - 1)] ?? 1;
    return (((base + buff) * a.combo * zone + gearWD) * (1 + power) + add) * hitMult(h) * drMult * proj + tru;
  };
  $("#dmgTable").innerHTML = `<div class="table-wrap"><table>
    <tr><th>Attack</th>${HIT.map(h => `<th class="num">${h[0]}<br><span class="muted small">×${num(hitMult(h), 2)}</span></th>`).join("")}</tr>
    ${attacks.map(a => `<tr><td>${esc(a.name)} <span class="muted small">${a.damageType || ""} ${a.combo !== 1 ? "· combo " + pct(a.combo, 0) : ""}</span></td>
      ${HIT.map(h => `<td class="num">${num(dmg(a, h), 1)}</td>`).join("")}</tr>`).join("")}
  </table></div>`;
  $("#dmgNotes").innerHTML = `
    <p><b>${esc(fam.name)}</b> (${g.rarity}) base ${magical ? "magical" : "physical"} weapon damage <b>${num(base, 1)}</b> (${L.roll} roll),
    power bonus <b>${spct(power)}</b>, additional ${num(add, 1)}, gear weapon damage ${num(gearWD, 1)}, true ${num(tru, 1)}.</p>
    <p>Target ${magical ? `MR ${num(tMR, 1)}` : `AR ${num(tAR, 1)}`} → damage reduction <b>${pct(tDR)}</b>; your penetration ${pct(pen)} → you deal <b>${pct(drMult)}</b> through armor.</p>
    ${fam.attacks ? "<p>Combo and impact-zone values come from the wiki's datamined weapon data.</p>" : "<p>No combo data for this weapon; assuming a 100% single hit.</p>"}`;
}
tabRenderers.damage = () => { fillWeapons(); renderDamage(); };

// =====================================================================
// SCALING EXPLORER
// =====================================================================
const derivedList = D.derived;
const rawCurveKeys = Object.keys(D.curves).filter(k => !derivedList.some(d => d.curve === k));
let curSel = store.get("curve", derivedList[0] && derivedList[0].key);
function renderCurveList() {
  $("#curveList").innerHTML = `<h3 style="margin-top:0">Derived stats</h3>` +
    derivedList.map(d => `<div class="list-item ${d.key === curSel ? "sel" : ""}" data-k="${d.key}"><span>${esc(d.label)}</span><small>${Object.keys(d.inputs).map(i => i.replace("@", "")).join(" + ")}</small></div>`).join("") +
    `<h3>Other raw curves</h3>` + rawCurveKeys.map(k => `<div class="list-item ${k === curSel ? "sel" : ""}" data-k="${k}"><span>${esc(k.replace("CT_", ""))}</span></div>`).join("");
}
$("#curveList").addEventListener("click", e => {
  const it = e.target.closest(".list-item"); if (!it) return;
  curSel = it.dataset.k; store.set("curve", curSel); renderScaling();
});

function currentInput(d) {
  const s = computeStats(L);
  const ins = Object.entries(d.inputs);
  if (ins[0][0].startsWith("@")) return { x: s[ins[0][0].slice(1)], label: ins[0][0].slice(1) };
  return { x: ins.reduce((t, [a, w]) => t + s.attrs[a] * w, 0), label: ins.map(([a, w]) => w === 1 ? a : `${a}×${w}`).join(" + ") };
}

function renderScaling() {
  renderCurveList();
  const d = derivedList.find(x => x.key === curSel);
  const ckey = d ? d.curve : curSel;
  const keys = D.curves[ckey] || [];
  const isP = d ? d.unit === "pct" : false;
  const cur = d ? currentInput(d) : null;
  $("#curveTitle").textContent = d ? d.label : ckey;
  $("#curveDesc").innerHTML = d
    ? `Input: <b>${esc(cur.label)}</b> ${d.weightsSource === "wiki" ? `<span class="tag-src" title="Attribute weights are applied in game code, documented by the community wiki">weights: wiki</span>` : ""}
       · curve <code>${esc(ckey)}</code> · your loadout: input <b>${num(cur.x, 2)}</b> → <b>${isP ? pct(curve(ckey, cur.x)) : num(curve(ckey, cur.x), 2)}</b> (before gear bonuses)`
    : `Raw curve <code>${esc(ckey)}</code>`;
  // chart
  const W = 760, H = 320, P = { l: 56, r: 16, t: 14, b: 34 };
  let xs = keys.map(k => k[0]), ys = keys.map(k => k[1]);
  let x0 = Math.min(...xs), x1 = Math.max(...xs);
  if (x0 < -100 && x1 > 0) x0 = Math.max(x0, -100);           // armor/MR curves: focus the useful range
  if (x1 > 500) x1 = 500;
  if (x0 === x1) x1 = x0 + 1;
  const vis = keys.filter(k => k[0] >= x0 && k[0] <= x1);
  ys = vis.map(k => k[1]).concat([curve(ckey, x0), curve(ckey, x1)]);
  let y0 = Math.min(...ys), y1 = Math.max(...ys); if (y0 === y1) { y0 -= 1; y1 += 1; }
  const sx = x => P.l + (x - x0) / (x1 - x0) * (W - P.l - P.r);
  const sy = y => H - P.b - (y - y0) / (y1 - y0) * (H - P.t - P.b);
  const fy = y => isP ? pct(y, 0) : num(y, 1);
  const ticks = (a, b, n) => Array.from({ length: n + 1 }, (_, i) => a + (b - a) * i / n);
  const pts = [[x0, curve(ckey, x0)], ...vis, [x1, curve(ckey, x1)]].map(([x, y]) => `${sx(x)},${sy(y)}`).join(" ");
  const mx = cur && cur.x >= x0 && cur.x <= x1 ? cur.x : null;
  $("#curveChart").innerHTML = `<div class="chart"><svg viewBox="0 0 ${W} ${H}" role="img" aria-label="curve chart">
    <g class="grid">${ticks(y0, y1, 5).map(y => `<line x1="${P.l}" x2="${W - P.r}" y1="${sy(y)}" y2="${sy(y)}"/><text x="${P.l - 6}" y="${sy(y) + 4}" text-anchor="end">${fy(y)}</text>`).join("")}
    ${ticks(x0, x1, 10).map(x => `<text x="${sx(x)}" y="${H - P.b + 16}" text-anchor="middle">${num(x, 0)}</text>`).join("")}</g>
    <polyline class="curve" points="${pts}"/>
    ${mx !== null ? `<line class="marker-line" x1="${sx(mx)}" x2="${sx(mx)}" y1="${P.t}" y2="${H - P.b}"/><circle class="marker" cx="${sx(mx)}" cy="${sy(curve(ckey, mx))}" r="5"/>` : ""}
    <rect id="hover" x="${P.l}" y="${P.t}" width="${W - P.l - P.r}" height="${H - P.t - P.b}" fill="transparent"/>
  </svg><div class="tooltip" hidden></div></div>`;
  const svg = $("#curveChart svg"), tip = $("#curveChart .tooltip");
  svg.addEventListener("mousemove", e => {
    const r = svg.getBoundingClientRect();
    const x = x0 + ((e.clientX - r.left) * W / r.width - P.l) / (W - P.l - P.r) * (x1 - x0);
    if (x < x0 || x > x1) { tip.hidden = true; return; }
    tip.hidden = false; tip.style.left = (e.clientX - r.left + 14) + "px"; tip.style.top = (e.clientY - r.top) + "px";
    tip.textContent = `${num(x, 1)} → ${isP ? pct(curve(ckey, x), 2) : num(curve(ckey, x), 2)}`;
  });
  svg.addEventListener("mouseleave", () => { tip.hidden = true; });

  // merged linear segments = the breakpoints
  const segs = [];
  for (let i = 1; i < keys.length; i++) {
    const [a, ya] = keys[i - 1], [b, yb] = keys[i];
    const slope = (yb - ya) / (b - a);
    const last = segs[segs.length - 1];
    if (last && Math.abs(last.slope - slope) < 1e-6) { last.to = b; last.yTo = yb; }
    else segs.push({ from: a, to: b, yFrom: ya, yTo: yb, slope });
  }
  const sf = v => isP ? spct(v, 2) : signed(v, 3);
  $("#curveSegments").innerHTML = `<div class="table-wrap"><table><tr><th class="num">From</th><th class="num">To</th><th class="num">Value</th><th class="num">Per +1 input</th></tr>
    ${segs.map(sg => `<tr ${cur && cur.x >= sg.from && cur.x < sg.to ? 'style="outline:1px solid var(--good)"' : ""}><td class="num">${num(sg.from, 2)}</td><td class="num">${num(sg.to, 2)}</td>
      <td class="num">${fy(sg.yFrom)} → ${fy(sg.yTo)}</td><td class="num">${sf(sg.slope)}</td></tr>`).join("")}
  </table></div><p class="muted small">Past the last key the value stays flat. The highlighted row is where your current loadout sits.</p>`;
}
tabRenderers.scaling = renderScaling;

// =====================================================================
// ITEMS
// =====================================================================
const recipesByResult = {}, recipesByIngredient = {};
for (const r of D.recipes) {
  (recipesByResult[r.result.item + "|" + r.result.rarity] ||= []).push(r);
  for (const i of r.ingredients) (recipesByIngredient[i.item] ||= []).push(r);
}
const families = Object.values(D.items).sort((a, b) => a.name.localeCompare(b.name));
const uniq = a => [...new Set(a.filter(Boolean))].sort();
$("#itemType").innerHTML += uniq(families.map(f => f.itemType)).map(t => `<option>${esc(t)}</option>`).join("");
$("#itemSlot").innerHTML += uniq(families.map(f => f.slot)).map(t => `<option>${esc(t)}</option>`).join("");
$("#itemClass").innerHTML += classIds.map(c => `<option>${c}</option>`).join("");
let itemSel = store.get("item", null);
["#itemSearch", "#itemType", "#itemSlot", "#itemClass"].forEach(id => $(id).addEventListener("input", renderItemList));
$("#itemList").addEventListener("click", e => { const it = e.target.closest(".list-item"); if (it) { itemSel = it.dataset.id; store.set("item", itemSel); renderItemList(); renderItem(); } });

function renderItemList() {
  const q = $("#itemSearch").value.toLowerCase(), t = $("#itemType").value, sl = $("#itemSlot").value, c = $("#itemClass").value;
  const list = families.filter(f => (!q || f.name.toLowerCase().includes(q) || f.id.toLowerCase().includes(q))
    && (!t || f.itemType === t) && (!sl || f.slot === sl) && (!c || f.classes.includes(c) || !f.classes.length));
  $("#itemCount").textContent = `${list.length} items`;
  $("#itemList").innerHTML = list.slice(0, 600).map(f => {
    const top = Object.keys(f.variants).pop();
    return `<div class="list-item ${f.id === itemSel ? "sel" : ""}" data-id="${esc(f.id)}"><span class="${rarityClass(top)}">${esc(f.name)}</span><small>${esc(f.armorType || f.weaponTypes.join("/") || f.slot || f.itemType)}</small></div>`;
  }).join("");
}

function recipeLine(r) {
  return `<span class="pill">${esc(r.merchant)}</span> <span class="${rarityClass(r.result.rarity)}">${esc(itemName(r.result.item))}</span> ← ${r.ingredients.map(i => `${i.amount}× <span class="${rarityClass(i.rarity)}">${esc(itemName(i.item))}</span>`).join(", ")}`;
}

function renderItem() {
  const f = D.items[itemSel];
  if (!f) { $("#itemDetail").innerHTML = `<p class="muted">Pick an item.</p>`; return; }
  const stats = uniq(Object.values(f.variants).flatMap(v => v.props.map(p => p.stat)));
  const w = f.wiki || {};
  const made = Object.keys(f.variants).flatMap(r => recipesByResult[f.id + "|" + r] || []);
  const used = recipesByIngredient[f.id] || [];
  const pool = f.secondaryPool && D.secondaryPools[f.secondaryPool];
  const equipSlot = SLOTS.find(s => s.slots.includes(f.slot));
  $("#itemDetail").innerHTML = `
    <h2>${esc(f.name)} ${f.nameSource === "id" ? `<span class="tag-src" title="No English name found; derived from internal id">name from id</span>` : ""}</h2>
    <p class="muted small"><code>${esc(f.id)}</code></p>
    <p>${[f.itemType, f.slot, f.hand, f.armorType, ...f.weaponTypes, f.utilityType, f.miscType, f.accessoryType].filter(Boolean).map(x => `<span class="pill">${esc(x)}</span>`).join("")}</p>
    ${w.flavortext ? `<p class="muted"><i>${esc(w.flavortext)}</i></p>` : ""}
    <p>Classes: ${f.classes.length ? f.classes.map(c => `<span class="pill">${c}</span>`).join("") : "<span class='muted'>any</span>"}
       ${w.invwidth ? ` · Inventory ${w.invwidth}×${w.invheight}` : ""} ${f.maxCount > 1 ? ` · Stack ${f.maxCount}` : ""}</p>
    ${equipSlot ? `<button class="ghost" id="equipBtn">Equip in loadout (${equipSlot.label})</button>` : ""}
    ${stats.length ? `<h3>Base stats by rarity</h3><div class="table-wrap"><table><tr><th>Rarity</th>${stats.map(s => `<th class="num">${esc(statName(s))}</th>`).join("")}${w.sellprices ? "<th class='num'>Sell</th>" : ""}</tr>
      ${Object.entries(f.variants).map(([r, v]) => `<tr><td class="${rarityClass(r)}">${r}</td>${stats.map(s => {
        const p = v.props.find(x => x.stat === s);
        return `<td class="num">${p ? esc(propText(p).slice(statName(s).length + 1)) : ""}</td>`;
      }).join("")}${w.sellprices ? `<td class="num">${esc(w.sellprices[String(D.rarities.indexOf(r) + 1)] ?? "")}</td>` : ""}</tr>`).join("")}
    </table></div>` : ""}
    ${f.attacks ? `<h3>Attacks</h3><div class="table-wrap"><table><tr><th>Attack</th><th>Type</th><th class="num">Combo</th><th class="num">Impact zones</th></tr>
      ${f.attacks.map(a => `<tr><td>${esc(a.name)}</td><td>${esc(a.damageType || "")}</td><td class="num">${pct(a.combo, 0)}</td><td class="num">${a.zones.map(z => pct(z, 0)).join(" / ")}</td></tr>`).join("")}</table></div>` : ""}
    ${pool ? `<h3>Random enchant pool <span class="muted small">(${esc(f.secondaryPool)})</span></h3><p>${pool.map(p => `<span class="pill">${esc(propText(p))}</span>`).join("")}</p>
      ${w.numenchants ? `<p class="muted small">Enchant count by rarity: ${Object.entries(w.numenchants).map(([k, n]) => `${D.rarities[k - 1]} ${n}`).join(", ")}</p>` : ""}` : ""}
    ${made.length ? `<h3>Crafted from</h3>${made.map(r => `<p>${recipeLine(r)}</p>`).join("")}` : ""}
    ${used.length ? `<h3>Used in ${used.length} recipe(s)</h3>${used.slice(0, 40).map(r => `<p>${recipeLine(r)}</p>`).join("")}` : ""}`;
  const eb = $("#equipBtn");
  if (eb) eb.addEventListener("click", () => {
    const rs = Object.keys(f.variants);
    const target = equipSlot.key === "ring1" && L.gear.ring1 ? "ring2" : equipSlot.key;
    L.gear[target] = { item: f.id, rarity: rs.includes("Epic") ? "Epic" : rs[rs.length - 1], ench: [] };
    if (f.classes.length && !f.classes.includes(L.cls)) L.classOnly = false;
    store.set("loadout", L); renderSlots(); showTab("loadout");
  });
}
tabRenderers.items = () => { renderItemList(); renderItem(); };

// =====================================================================
// CRAFTING
// =====================================================================
$("#craftMerchant").innerHTML += uniq(D.recipes.map(r => r.merchant)).map(mm => `<option>${esc(mm)}</option>`).join("");
let craftSel = null;
["#craftSearch", "#craftMerchant"].forEach(id => $(id).addEventListener("input", renderCraftList));
$("#craftList").addEventListener("click", e => { const it = e.target.closest(".list-item"); if (it) { craftSel = +it.dataset.i; renderCraftList(); renderCraft(); } });

function renderCraftList() {
  const q = $("#craftSearch").value.toLowerCase(), mm = $("#craftMerchant").value;
  $("#craftList").innerHTML = D.recipes.map((r, i) => [r, i])
    .filter(([r]) => (!mm || r.merchant === mm) && (!q || itemName(r.result.item).toLowerCase().includes(q)))
    .sort((a, b) => itemName(a[0].result.item).localeCompare(itemName(b[0].result.item)))
    .map(([r, i]) => `<div class="list-item ${i === craftSel ? "sel" : ""}" data-i="${i}"><span class="${rarityClass(r.result.rarity)}">${esc(itemName(r.result.item))}</span><small>${esc(r.merchant)}</small></div>`).join("");
}

function renderCraft() {
  const r = D.recipes[craftSel]; if (!r) return;
  const totals = {};
  const tree = (item, rarity, amount, seen) => {
    const key = item + "|" + rarity;
    const sub = !seen.has(key) && (recipesByResult[key] || [])[0];
    const label = `${amount}× <span class="${rarityClass(rarity)}">${esc(itemName(item))}</span>`;
    if (!sub) { totals[key] = (totals[key] || 0) + amount; return `<li>${label}</li>`; }
    const s2 = new Set(seen); s2.add(key);
    return `<li>${label} <span class="muted">(${esc(sub.merchant)})</span><ul>${sub.ingredients.map(i => tree(i.item, i.rarity, i.amount * amount, s2)).join("")}</ul></li>`;
  };
  const html = `<ul>${r.ingredients.map(i => tree(i.item, i.rarity, i.amount, new Set([r.result.item + "|" + r.result.rarity]))).join("")}</ul>`;
  $("#craftDetail").innerHTML = `<h2 class="${rarityClass(r.result.rarity)}">${esc(itemName(r.result.item))} <span class="muted small">(${r.result.rarity})</span></h2>
    <p><span class="pill">${esc(r.merchant)}</span></p>
    <h3>Material tree</h3><div class="tree">${html}</div>
    <h3>Total raw materials</h3><table>${Object.entries(totals).sort((a, b) => b[1] - a[1]).map(([k, n]) => {
      const [it, ra] = k.split("|");
      return `<tr><td class="${rarityClass(ra)}">${esc(itemName(it))}</td><td class="num">${n}</td></tr>`;
    }).join("")}</table>
    <p class="muted small">Sub-recipes expand automatically when the game data has a recipe for that exact item and rarity. Totals assume each craft yields 1.</p>`;
}
tabRenderers.crafting = () => { renderCraftList(); renderCraft(); };

// =====================================================================
// PATCH CHANGES + DATA
// =====================================================================
tabRenderers.changes = () => {
  const cl = window.DAD_CHANGELOG;
  if (!cl) {
    $("#changes").innerHTML = `<h2>No patch diff yet</h2><p>This is the baseline build (<b>${esc(m.build)}</b>). After the next game update, run:</p>
      <pre class="formula"><code>pwsh scripts/update.ps1</code></pre><p class="muted">It re-extracts the game files, rebuilds the data and writes a changelog that shows up here. The Markdown version lands in <code>data/changelogs/</code>.</p>`;
    return;
  }
  const ch = cl.changes;
  const sec = (title, rows) => rows.length ? `<h3>${title} (${rows.length})</h3><div class="table-wrap"><table>${rows.join("")}</table></div>` : "";
  $("#changes").innerHTML = `<h2>${esc(cl.from)} → ${esc(cl.to)}</h2>` +
    sec("Items", ch.items.map(c => `<tr><td>${esc(c.name)} ${c.rarity ? `<span class="${rarityClass(c.rarity)}">[${c.rarity}]</span>` : ""}</td><td>${esc(c.kind)}</td>
      <td>${c.before ? `<span class="change-rem">${esc(c.before)}</span><br>` : ""}${c.after ? `<span class="change-add">${esc(c.after)}</span>` : ""}</td></tr>`)) +
    sec("Recipes", ch.recipes.map(c => `<tr><td>${esc(c.recipe)}</td><td>${c.kind}</td><td>${c.before ? `<span class="change-rem">${esc(c.before)}</span><br>` : ""}${c.after ? `<span class="change-add">${esc(c.after)}</span>` : ""}</td></tr>`)) +
    sec("Scaling curves", ch.curves.map(c => `<tr><td><code>${esc(c.curve)}</code></td><td class="small"><span class="change-rem">${esc(JSON.stringify(c.before))}</span><br><span class="change-add">${esc(JSON.stringify(c.after))}</span></td></tr>`)) +
    sec("Classes", ch.classes.map(c => `<tr><td>${esc(c.class)}</td><td>${esc(c.kind)}</td><td class="small">${esc(JSON.stringify(Object.fromEntries(Object.entries(c).filter(([k]) => !["class", "kind"].includes(k)))))}</td></tr>`)) +
    sec("Constants", ch.constants.map(c => `<tr><td><code>${esc(c.constant)}</code></td><td class="num change-rem">${esc(c.before)}</td><td class="num change-add">${esc(c.after)}</td></tr>`)) +
    sec("Perks / skills / spells", ch.abilities.map(c => `<tr><td>${esc(c.type)}</td><td>${esc(c.id)}</td><td>${c.before == null ? "added" : c.after == null ? "removed" : "changed"}</td></tr>`)) +
    (ch.files && ch.files.added ? `<h3>Game files</h3><p>${ch.files.added.length} added, ${ch.files.removed.length} removed (full list in <code>data/changelogs/</code>)</p>` : "");
};

tabRenderers.data = () => {
  const h = m.decodeHealth;
  $("#dataInfo").innerHTML = `<h2>Where the numbers come from</h2>
    <dl class="kv">
      <dt>Game build</dt><dd>${esc(m.build)} (from <code>UpdateVersion.dat</code>)</dd>
      <dt>Extracted</dt><dd>${esc(m.generatedAt)}</dd>
      ${Object.entries(m.counts).map(([k, v]) => `<dt>${esc(k)}</dt><dd>${v}</dd>`).join("")}
      <dt>Decode health</dt><dd>${h.assets - h.empty} / ${h.assets} assets decoded (${pct((h.assets - h.empty) / h.assets)})</dd>
    </dl>
    <h3>Sources</h3>
    <ul>
      <li><b>Game files</b> (via CUE4Parse + community mappings): item stats per rarity, class requirements, enchant pools, crafting recipes, class attributes, perks/skills/spells, scaling curves, constants (hitbox multipliers, caps).</li>
      <li><b>Community wiki</b> (spellsandguns, cached in <code>data/wiki</code>): English item names, sell prices, inventory size, enchant counts, weapon combo/impact-zone data, and the attribute weights for hybrid stats (e.g. Health = 0.25 STR + 0.75 VIG). Those weights live in game code, not data files.</li>
    </ul>
    <h3>Systems that do not decode yet (${Object.keys(h.brokenFolders).length})</h3>
    <p class="muted small">These struct layouts changed after the mappings file was made (Dec 2025). A newer <code>.usmap</code> in <code>tools/mappings</code> fixes them on the next update run.</p>
    <div class="table-wrap"><table><tr><th>Folder</th><th class="num">Empty / total</th></tr>
      ${Object.entries(h.brokenFolders).map(([k, [t, e]]) => `<tr><td><code>${esc(k)}</code></td><td class="num">${e} / ${t}</td></tr>`).join("")}
    </table></div>`;
};

// ---------- boot ----------
renderSlots();
renderSheet();
showTab(store.get("tab", "loadout"));
})();
