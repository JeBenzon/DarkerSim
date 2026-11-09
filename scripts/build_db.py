"""Normalize a raw CUE4Parse export into one compact, diffable game database.

    python scripts/build_db.py build-80

Reads   data/raw/<build>/Data/Generated/V2/...   (+ Data/GameplayAbility curves)
Writes  data/builds/<build>.json                 (canonical, used for diffs)
        app/data/game.js                         (same data, loadable from file://)
"""
import json
import os
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

RARITIES = ["Poor", "Common", "Uncommon", "Rare", "Epic", "Legend", "Unique", "Artifact"]
ATTRS = ["Strength", "Vigor", "Agility", "Dexterity", "Will", "Knowledge", "Resourcefulness"]

# Stat-name prettifying for property types whose internal names are odd.
PROP_NAME_OVERRIDES = {
    "MagicRegistance": "Magic Resistance",
    "PhysicalWeaponDamageAdd": "Additional Weapon Damage",
    "PhysicalDamageAdd": "Additional Physical Damage",
    "MagicalDamageAdd": "Additional Magical Damage",
    "PhysicalDamageBonus": "Physical Power Bonus",
    "MagicalDamageBonus": "Magical Power Bonus",
    "PhysicalDamageTrue": "True Physical Damage",
    "MagicalDamageTrue": "True Magical Damage",
    "ArmorRatingAdd": "Additional Armor Rating",
    "MoveSpeedAdd": "Additional Move Speed",
    "MaxHealthAdd": "Max Health",
    "MaxHealthBonus": "Max Health Bonus",
    "MemoryCapacityAdd": "Additional Memory Capacity",
    "Primitive": "All Attributes",
    "PhysicalDamageReduction": "Physical Damage Reduction",
    "MagicalDamageReduction": "Magical Damage Reduction",
    "ProjectileReductionMod": "Projectile Damage Reduction",
    "HeadshotReductionMod": "Headshot Damage Reduction",
    "HeadshotDamageMod": "Headshot Damage Bonus",
    "UndeadDamageMod": "Undead Damage Bonus",
    "UndeadReductionMod": "Undead Damage Reduction",
    "DemonDamageMod": "Demon Damage Bonus",
    "DemonReductionMod": "Demon Damage Reduction",
}

# Which raw curve tables feed which derived stat, and with what attribute weights.
# Curves come from the game files; the attribute weights live in game code and are
# community-documented (spellsandguns wiki, "Hybrid Stats"). Marked source accordingly.
DERIVED = [
    # key,                    label,                        curve table/row,                          inputs (attr: weight),               unit
    ("PhysicalPower",         "Physical Power",            ("CT_Strength", "PhysicalPower"),          {"Strength": 1},                      "flat"),
    ("PhysicalDamageMod",     "Physical Power Bonus",      ("CT_PhysicalPower", "PhysicalDamageMod"), {"@PhysicalPower": 1},               "pct"),
    ("MagicalPower",          "Magical Power",             ("CT_Will", "MagicalPower"),               {"Will": 1},                          "flat"),
    ("MagicalDamageMod",      "Magical Power Bonus",       ("CT_MagicalPower", "MagicalDamageMod"),   {"@MagicalPower": 1},                "pct"),
    ("MaxHealthBase",         "Base Health",               ("CT_MaxHealthBase", "MaxHealthBase"),     {"Strength": 0.25, "Vigor": 0.75},    "flat"),
    ("HealthRecoveryMod",     "Health Recovery Bonus",     ("CT_RecoveryMod", "HealthRecoveryMod"),   {"Vigor": 1},                         "pct"),
    ("MoveSpeedBase",         "Move Speed (from Agility)", ("CT_Agility", "MoveSpeedBase"),           {"Agility": 1},                       "flat"),
    ("ActionSpeed",           "Action Speed",              ("CT_ActionSpeed", "ActionSpeed"),         {"Agility": 0.25, "Dexterity": 0.75}, "pct"),
    ("ManualDexterity",       "Manual Dexterity",          ("CT_Dexterity", "ManualDexterity"),       {"Dexterity": 1},                     "pct"),
    ("ItemEquipSpeed",        "Item Equip Speed",          ("CT_Dexterity", "ItemEquipSpeed"),        {"Dexterity": 1},                     "pct"),
    ("RegularInteractionSpeed","Regular Interaction Speed",("CT_RegularInteractionSpeedBase", "RegularInteractionSpeed"), {"Dexterity": 0.25, "Resourcefulness": 0.75}, "pct"),
    ("MagicResistance",       "Magic Resistance",          ("CT_Will", "MagicResistance"),            {"Will": 1},                          "flat"),
    ("MagicalReduction",      "Magical Damage Reduction",  ("CT_MagicResistance", "MagicalReduction"),{"@MagicResistance": 1},             "pct"),
    ("PhysicalReduction",     "Physical Damage Reduction", ("CT_ArmorRating", "PhysicalReduction"),   {"@ArmorRating": 1},                 "pct"),
    ("BuffDurationMod",       "Buff Duration Bonus",       ("CT_Will", "BuffDurationMod"),            {"Will": 1},                          "pct"),
    ("DebuffDurationMod",     "Debuff Duration Bonus",     ("CT_Will", "DebuffDurationMod"),          {"Will": 1},                          "pct"),
    ("MagicalInteractionSpeed","Magical Interaction Speed",("CT_Will", "MagicalInteractionSpeed"),    {"Will": 1},                          "pct"),
    ("SpellCastingSpeed",     "Spell Casting Speed",       ("CT_Knowledge", "SpellCastingSpeed"),     {"Knowledge": 1},                     "pct"),
    ("MemoryCapacity",        "Memory Capacity",           ("CT_Knowledge", "MemoryCapacity"),        {"Knowledge": 1},                     "flat"),
    ("MemoryRecoveryMod",     "Memory Recovery",           ("CT_RecoveryMod", "MemoryRecoveryMod"),   {"Knowledge": 1},                     "pct"),
    ("CooldownReduction",     "Cooldown Reduction",        ("CT_Resourcefulness", "CooldownReduction"),{"Resourcefulness": 1},              "pct"),
    ("Persuasiveness",        "Persuasiveness",            ("CT_Resourcefulness", "Persuasiveness"),  {"Resourcefulness": 1},               "flat"),
]


# Status-effect (buff/debuff) fields -> engine stat, scale, confidence.
# The internal GameplayEffect field names do NOT match their meaning; this table was
# calibrated against wiki-tested ability values (e.g. BluntWeaponMastery
# PhysicalDamageAdd=150 is "+15% Physical Power Bonus"). "verified" = checked against
# at least one wiki-documented ability, "inferred" = same convention, not yet checked.
STATUS_FIELDS = {
    "StrengthBase": ("Strength", 1, "verified"), "VigorBase": ("Vigor", 1, "verified"),
    "AgilityBase": ("Agility", 1, "verified"), "DexterityBase": ("Dexterity", 1, "verified"),
    "WillBase": ("Will", 1, "verified"), "KnowledgeBase": ("Knowledge", 1, "verified"),
    "ResourcefulnessBase": ("Resourcefulness", 1, "verified"),
    "StrengthMod": ("StrengthMod", 0.001, "inferred"), "VigorMod": ("VigorMod", 0.001, "inferred"),
    "AgilityMod": ("AgilityMod", 0.001, "inferred"), "DexterityMod": ("DexterityMod", 0.001, "inferred"),
    "WillMod": ("WillMod", 0.001, "inferred"), "KnowledgeMod": ("KnowledgeMod", 0.001, "inferred"),
    "ResourcefulnessMod": ("ResourcefulnessMod", 0.001, "inferred"),
    "PhysicalDamageAdd": ("PhysicalDamageBonus", 0.001, "verified"),     # = Physical Power Bonus %
    "PhysicalDamageMod": ("PhysicalPower", 1, "verified"),                # = flat Physical Power
    "PhysicalDamageWeapon": ("BuffWeaponDamage", 1, "verified"),
    "MagicalDamageMod": ("MagicalDamageBonus", 0.001, "verified"),
    "MagicalPower": ("MagicalPower", 1, "verified"),
    "MaxHealthAdd": ("MaxHealthBonus", 0.001, "verified"),               # = Max Health Bonus %
    "ArmorRating": ("ArmorRatingAdd", 1, "verified"),
    "MagicResistance": ("MagicRegistance", 1, "verified"),
    "ItemArmorRatingMod": ("ItemArmorRatingMod", 0.001, "verified"),
    "PhysicalReduction": ("PhysicalDamageReduction", 0.001, "verified"),
    "MagicalReduction": ("MagicalDamageReduction", 0.001, "inferred"),
    "PhysicalReductionMod": ("PhysicalReductionMod", 0.001, "verified"),
    "MagicalReductionMod": ("MagicalReductionMod", 0.001, "inferred"),
    "HeadshotDamageMod": ("HeadshotDamageMod", 0.001, "verified"),
    "HeadshotReductionMod": ("HeadshotReductionMod", 0.001, "inferred"),
    "ProjectileReductionMod": ("ProjectileReductionMod", 0.001, "inferred"),
    "ArmorPenetration": ("ArmorPenetration", 0.001, "inferred"),
    "MagicPenetration": ("MagicPenetration", 0.001, "inferred"),
    "MoveSpeedAdd": ("MoveSpeedBonus", 0.001, "verified"),               # = Move Speed Bonus %
    "SpellCastingSpeed": ("ActionSpeed", 0.001, "verified"),             # wiki lists these as Action Speed
    "ItemEquipSpeed": ("ItemEquipSpeed", 0.001, "inferred"),
    "ManualDexterity": ("ManualDexterity", 0.001, "inferred"),
    "MagicalInteractionSpeed": ("MagicalInteractionSpeed", 0.001, "inferred"),
    "CooldownReductionMod": ("CooldownReductionBonus", 0.001, "inferred"),
    "MemoryCapacityAdd": ("MemoryCapacityAdd", 1, "inferred"),
    "MaxPhysicalShield": ("PhysicalShield", 1, "inferred"),
    "MaxMagicalShield": ("MagicalShield", 1, "inferred"),
}
BUFF_OWNER_FOLDERS = {
    "Perk/Perk": "Perk", "Perk/PerkAbility": "Perk", "Perk/PerkEffect": "Perk",
    "Skill/Skill": "Skill", "Skill/SkillAbility": "Skill", "Skill/SkillEffect": "Skill",
    "Spell/Spell": "Spell", "Spell/SpellAbility": "Spell", "Spell/SpellEffect": "Spell",
    "Item/Item": "Item", "Item/ItemAbility": "Item", "Item/ItemEffect": "Item", "Item/ItemConsume": "Item",
    "Religion/ReligionBlessing": "Shrine / Religion", "FaustianBargain/FaustianBargainEffect": "Faustian Bargain",
    "FaustianBargain/FaustianBargainAbility": "Faustian Bargain",
}


# Buffs that only work while another effect is up: buff key -> (required tag, note shown in the app).
# Brewmaster (Cleric): "When you drink alcohol, you no longer exhibit the detrimental drunk effects and
# gain 10 Strength and increase the duration of drunken effects to 1.5x."
BUFF_REQUIRES = {
    "Brewmaster": ("drunk", "Only while drunk: add a \"Drunk from Ale\" buff. Brewmaster also removes the drunk "
                            "dizziness and makes drunken effects last 1.5x longer (the Ale's drunk becomes \"Sacred Water\")."),
}


# Festive Ale effects: property names in this table don't line up with our mappings (e.g. the Luck value is
# stored under "ManualDexterity"); these are the wiki's readings of the same numbers.
DRINK_FIX = {
    "FestiveAleConsume_FestivalGrace": [{"stat": "Luck", "value": 100}],
    "FestiveAleConsume_LuckyCup": [{"stat": "Luck", "value": 30}],
    "FestiveAleConsume_HeartyDrink": [{"stat": "MaxHealthAdd", "value": 10}],
    "FestiveAleConsume_LightFootsteps": [{"stat": "MoveSpeedAdd", "value": 3}],
}


def drink_buff(key):
    """Ale / Festive Ale effects from the item-cosmetic status table: (name, tags) or None to skip.
    Sacred Water is the same Ale strength under Brewmaster, so it is not listed separately."""
    m = re.match(r"AleConsumeDrunken_(\d)001$", key)
    if m:
        return f"Drunk from Ale {m.group(1)}001", ["drunk"]
    m = re.match(r"FestiveAleConsume_(\w+)$", key)
    if m:
        return "Festive Ale: " + humanize(m.group(1)), (["drunk"] if m.group(1) == "DrunkenCourage" else [])
    return None


def build_buffs(raw, classes):
    """Status effects with combat-relevant stat modifiers, linked to their source."""
    sdir = os.path.join(raw.v2, "ActorStatus", "StatusEffect")
    if not os.path.isdir(sdir):
        return {}
    # drink effects (Ale "drunk", Festive Ale) live in a separate status table
    ddir = os.path.join(raw.v2, "ActorStatusItemCosmetic", "StatusEffect")
    files = [(sdir, fn) for fn in sorted(os.listdir(sdir))]
    if os.path.isdir(ddir):
        files += [(ddir, fn) for fn in sorted(os.listdir(ddir)) if drink_buff(fn[:-5].replace("Id_ActorStatusEffect_", ""))]
    # reverse index: which assets reference which status effect / ability
    refs = defaultdict(set)
    pat = re.compile(r"/(Id_[A-Za-z0-9_]+)\.")
    for folder in BUFF_OWNER_FOLDERS:
        d = os.path.join(raw.v2, folder)
        if not os.path.isdir(d):
            continue
        for fn in os.listdir(d):
            with open(os.path.join(d, fn), encoding="utf8") as f:
                txt = f.read()
            for ref in set(pat.findall(txt)):
                refs[ref].add((folder, fn[:-5]))
    owner_prefix = {"Perk/Perk": "Id_Perk_", "Skill/Skill": "Id_Skill_", "Spell/Spell": "Id_Spell_", "Item/Item": "Id_Item_"}

    def resolve(asset, depth=0):
        """Follow Ability/Effect assets back to the owning perk/skill/spell/item."""
        out = set()
        for folder, aid in refs.get(asset, ()):
            if folder in owner_prefix:
                out.add((BUFF_OWNER_FOLDERS[folder], aid.replace(owner_prefix[folder], "")))
            elif depth < 3:
                sub = resolve(aid, depth + 1)
                out |= sub or {(BUFF_OWNER_FOLDERS[folder], re.sub(r"^Id_[A-Za-z]+?_", "", aid))}
        return out

    buffs = {}
    for fdir, fn in files:
        with open(os.path.join(fdir, fn), encoding="utf8") as f:
            ex = json.load(f)
        p = ex[0].get("Properties", {}) if ex else {}
        mods, conf = [], set()
        for k, v in p.items():
            if k in STATUS_FIELDS and isinstance(v, (int, float)) and v:
                stat, scale, c = STATUS_FIELDS[k]
                mods.append({"stat": stat, "value": round(v * scale, 6)})
                conf.add(c)
        if fn[:-5].replace("Id_ActorStatusEffect_", "") in DRINK_FIX:
            mods, conf = [dict(m) for m in DRINK_FIX[fn[:-5].replace("Id_ActorStatusEffect_", "")]], {"inferred"}
        if not mods:
            continue
        aid = fn[:-5]
        key = aid.replace("Id_ActorStatusEffect_", "")
        owners = sorted(o for o in resolve(aid) if o[0] != "Other")
        if not owners:  # fall back to matching the name prefix against class abilities
            stem = re.sub(r"(Granted)?(_.*)?$", "", key)
            for t in ("Perk", "Skill", "Spell"):
                if any(stem in classes[c][t.lower() + "s"] for c in classes):
                    owners = [(t, stem)]
                    break
        if not owners and re.search(r"_(Bad|Good|Perfect)$", key):
            owners = [("Bard song", re.sub(r"(Granted)?_(Bad|Good|Perfect)$", "", key))]
        if not owners:
            m = re.match(r"(CharacterSkin|Religion|Luck|ShapeShift|DeathSwarm|FloorRule)", key)
            owners = [(humanize(m.group(1)) if m else "Other", key.split("_")[0])]
        cls = sorted({c for t, o in owners if t in ("Perk", "Skill", "Spell") for c in classes if o in classes[c][t.lower() + "s"]})
        if any(t == "Bard song" for t, _ in owners) and "Bard" in classes:
            cls = ["Bard"]
        if fdir == ddir:  # drinks: the source is the Ale item itself
            owners, cls = [("Item", "FestiveAle" if key.startswith("FestiveAle") else "Ale")], []
        dur = p.get("Duration")
        drink = drink_buff(key) if fdir == ddir else None
        req = BUFF_REQUIRES.get(key)
        buffs[key] = {
            "name": drink[0] if drink else humanize(key),
            "tags": drink[1] if drink else [],
            "requires": req[0] if req else None,
            "note": req[1] if req else None,
            "mods": mods,
            "confidence": "verified" if conf == {"verified"} else "inferred",
            "sources": [{"type": t, "id": o} for t, o in owners],
            "classes": cls,
            "duration": dur if isinstance(dur, (int, float)) else None,
            "debuff": bool(re.search(r"debuff|curse|slow|weak|hit$|bleed|poison|burn", key, re.I)) or all(m["value"] < 0 for m in mods),
        }
    return buffs


DMG_FIELDS = {
    "ExecMagicalDamageBase": ("magical", False), "ExecMagicalDamageTrue": ("magical", True),
    "ExecPhysicalDamageBase": ("physical", False), "ExecPhysicalDamageTrue": ("physical", True),
}


def build_skins(raw, buffs):
    """Character skins that change stats (e.g. Felidian Panther: +2 Agility, -1 Strength, -1 Vigor).
    The Man/Woman versions of a skin share one status effect, so they become one entry keyed by that effect.
    Their effects are taken out of `buffs`: a character wears exactly one skin, it isn't a buff."""
    skins = {}
    for aid, p in raw.folder("CharacterSkin/CharacterSkin").items():
        for ref in p.get("Effects", []):
            eff = (asset_name(ref) or "").replace("Id_ActorStatusEffect_", "")
            b = buffs.get(eff)
            if not eff.startswith("CharacterSkin_") or not b or not b.get("mods"):
                continue
            key = eff.replace("CharacterSkin_", "")
            cls = sorted({c.split("::")[-1] for c in p.get("TargetCharacterClasses", []) if isinstance(c, str)})
            sk = skins.setdefault(key, {"name": humanize(key), "classes": [], "mods": b["mods"],
                                        "confidence": b.get("confidence", "inferred"), "variants": []})
            sk["classes"] = sorted(set(sk["classes"]) | set(cls))
            sk["variants"].append(aid.replace("Id_CharacterSkin_", ""))
    # portrait: the skin's shop art (Data/Art/Shop/CharacterSkin/Shop_CharacterSkin_<variant> -> ShopIconTexture),
    # exported to data/icons/skin by update.ps1. First variant that has one wins (Man before Woman).
    art_dir = os.path.join(raw.v2, "..", "..", "Art", "Shop", "CharacterSkin")
    icon_dir = os.path.join(ROOT, "data", "icons", "skin")
    for key, sk in skins.items():
        sk["variants"].sort(key=lambda v, k=key: (v not in (k, k + "Man"), not v.endswith("Man"), v))
        for v in sk["variants"]:
            fp = os.path.join(art_dir, f"Shop_CharacterSkin_{v}.json")
            if not os.path.exists(fp):
                continue
            with open(fp, encoding="utf8") as f:
                ex = json.load(f)
            tex = (ex[0].get("Properties", {}).get("ShopIconTexture") or {}).get("ObjectPath", "") if ex else ""
            name = tex.rsplit("/", 1)[-1].split(".")[0]
            if name and os.path.exists(os.path.join(icon_dir, name + ".webp")):
                sk["icon"] = "skin/" + name
                break
    for key in skins:
        buffs.pop("CharacterSkin_" + key, None)
    return dict(sorted(skins.items()))


def build_damage_abilities(raw, classes):
    """Damage components of spells and skills: direct hit / splash / damage over time.

    ExecHealthCost on a damage effect is the Attribute Bonus Ratio ("Scaling") in per-mille,
    Duration is in ms. DoT tick interval lives in the GE blueprint; every documented DoT is 1s.
    """
    def load(folder, aid):
        p = os.path.join(raw.v2, folder, aid + ".json")
        if not os.path.exists(p):
            return None
        with open(p, encoding="utf8") as f:
            ex = json.load(f)
        return ex[0].get("Properties", {}) if ex else {}

    perks_by_class = {c: set(v["perks"]) for c, v in classes.items()}
    out = {}
    for kind, folder, prefix in (("spell", "Spell/SpellAbility", "Id_SpellAbility_"), ("skill", "Skill/SkillAbility", "Id_SkillAbility_")):
        d = os.path.join(raw.v2, folder)
        if not os.path.isdir(d):
            continue
        for fn in sorted(os.listdir(d)):
            aid = fn[:-5]
            owner = aid.replace(prefix, "")
            with open(os.path.join(d, fn), encoding="utf8") as f:
                ex = json.load(f)
            p = ex[0].get("Properties", {}) if ex else {}
            owner_classes = [c for c, v in classes.items() if owner in v[kind + "s"]]
            comps = []
            for ref in p.get("Effects", []):
                eid = asset_name(ref)
                if not eid:
                    continue
                e = None
                for fold in ("Spell/SpellEffect", "Skill/SkillEffect", "ActorStatus/StatusEffect", "Item/ItemEffect"):
                    e = load(fold, eid)
                    if e is not None:
                        break
                if not e:
                    continue
                dmg = [(k, v) for k, v in e.items() if k in DMG_FIELDS and isinstance(v, (int, float)) and v]
                weapon_ratio = e.get("ExecDamageWeaponRatio")
                if not dmg and not weapon_ratio:
                    continue
                event = (e.get("EventTag") or {}).get("TagName", "") if isinstance(e.get("EventTag"), dict) else ""
                tags = [t.get("TagName", "") for t in e.get("GrantedTags", []) if isinstance(t, dict)]
                cls_name = (e.get("EffectClass") or {}).get("ObjectName", "") if isinstance(e.get("EffectClass"), dict) else ""
                is_dot = "DamageOverTime" in event or "Period" in cls_name or ("Duration" in e and "Hit" not in eid.split("_")[-1])
                ckind = "dot" if is_dot else "aoe" if re.search(r"Radius|Aoe|Area|Explosion", event + eid) else "hit"
                suffix = eid.split("_")[-1]
                req_perk = suffix if any(suffix in perks_by_class.get(c, ()) for c in owner_classes) else None
                element = next((t.rsplit(".", 1)[-1] for t in tags if t.startswith("Type.Damage.Magical.") or t.startswith("Type.Damage.Physical.")), None)
                for k, v in dmg or [("weapon", 0)]:
                    dtype, true_dmg = DMG_FIELDS.get(k, ("physical", False))
                    comps.append({
                        "id": eid.replace("Id_SpellEffect_", "").replace("Id_SkillEffect_", "").replace("Id_ActorStatusEffect_", ""),
                        "kind": ckind,
                        "damageType": dtype,
                        "true": true_dmg,
                        "base": v,
                        "weaponRatio": (weapon_ratio / 1000) if weapon_ratio else None,
                        "scaling": (e.get("ExecHealthCost", 1000) or 0) / 1000,
                        "duration": (e["Duration"] / 1000) if isinstance(e.get("Duration"), (int, float)) and e["Duration"] > 0 else None,
                        "tickInterval": 1.0 if ckind == "dot" else None,
                        "element": element,
                        "projectile": "Type.Damage.Projectile" in tags,
                        "requiresPerk": req_perk,
                    })
            if comps:
                out[owner] = {"kind": kind, "name": humanize(owner), "classes": owner_classes, "components": comps}
    return out


ABILITY_SUFFIXES = re.compile(r"(_OnGranted|_Granted|_Route|_Active|Spell)$")


RARITY_BY_DIGIT = {"1": "Poor", "2": "Common", "3": "Uncommon", "4": "Rare", "5": "Epic", "6": "Legend", "7": "Unique", "8": "Artifact"}


def attach_buff_icons(buffs, families):
    """Gives each buff an "icon" path under icons/ (e.g. "buff/Icon_Buff_Rage") and a readable name.
    Order: Icon_Buff_/Icon_Debuff_<name> -> source skill/spell/perk/song icon -> source item icon -> god icon."""
    def index(sub):
        d = os.path.join(ROOT, "data", "icons", sub)
        return {f[:-5]: f"{sub}/{f[:-5]}" if sub else f[:-5] for f in os.listdir(d) if f.endswith(".webp")} if os.path.isdir(d) else {}
    buff_icons, ability_icons = index("buff"), index("ability")
    by_norm = {}
    for stem, path in list(buff_icons.items()) + list(ability_icons.items()):
        kind, _, name = stem.removeprefix("Icon_").partition("_")
        by_norm.setdefault((kind, norm(name)), path)
    fam_by_norm = sorted(((norm(fid), fid) for fid in families), key=lambda x: -len(x[0]))

    def candidates(key):
        parts = key.split("_")
        for i in range(len(parts), 0, -1):
            base = "_".join(parts[:i])
            for b in (base, base.replace("Granted", ""), re.sub(r"(Activate[d]?|Passive|Debuff|Buff|Consume|Shield|Hit|Slow|Attack\d*)+$", "", base)):
                if len(norm(b)) >= 3:
                    yield norm(b)

    def find(key, kinds):
        for c in candidates(key):
            for k in kinds:
                if (k, c) in by_norm:
                    return by_norm[(k, c)]
        return None

    def item_icon(src_id):
        n = norm(src_id)
        m = re.search(r"_(\d)\d{3}", src_id)
        for fn_, fid in fam_by_norm:
            if len(fn_) >= 4 and n.startswith(fn_):
                vs = families[fid].get("variants", {})
                want = RARITY_BY_DIGIT.get(m.group(1)) if m else None
                v = vs.get(want) if want else None
                icon = (v or {}).get("icon") or next((x.get("icon") for x in vs.values() if x.get("icon")), None)
                if icon:
                    return icon
        return None

    def drink_icon(key):
        fam = "FestiveAle" if key.startswith("FestiveAle") else "Ale" if key.startswith("AleConsume") else None
        if not fam or fam not in families:
            return None
        vs = families[fam].get("variants", {})
        m = re.search(r"_(\d)001$", key)
        v = vs.get(RARITY_BY_DIGIT.get(m.group(1))) if m else None
        return (v or {}).get("icon") or next((x.get("icon") for x in vs.values() if x.get("icon")), None)

    for key, b in buffs.items():
        kinds = ["Debuff", "Buff"] if b.get("debuff") else ["Buff", "Debuff"]
        icon = drink_icon(key) or find(key, kinds)
        for src in b.get("sources", []):
            if icon:
                break
            t, sid = src.get("type", ""), src.get("id", "")
            if t == "Shrine / Religion":
                god = sid.split("_")[1] if "_" in sid else sid
                icon = buff_icons.get(f"Icon_Religion_{god}")
            elif t == "Item":
                icon = find(sid, kinds) or item_icon(sid) or item_icon(key)
            else:
                icon = find(sid, kinds + ["Skill", "Spell", "Perk", "Music", "ShapeShift"])
        if not icon and key.startswith("Luck"):
            icon = find("Luck", ["Buff"])
        b["icon"] = icon
        # readable names: "AllegroGranted_Good" -> "Allegro (Good)", "..._1001" -> "(Poor)", "LV07" -> "Lv 7"
        name = b.get("name") or humanize(key)
        name = re.sub(r"\bGranted\b", "", name)
        name = re.sub(r"\b(Good|Bad|Perfect)$", r"(\1)", name.strip())
        name = re.sub(r"\b([1-8])001$", lambda mm: "(" + {"Legend": "Legendary"}.get(RARITY_BY_DIGIT[mm.group(1)], RARITY_BY_DIGIT[mm.group(1)]) + ")", name)
        name = re.sub(r"\bLV ?0?(\d+)", r"Lv \1", name)
        name = re.sub(r"(?<=[a-z]) Of (?=[A-Z])", " of ", name)
        name = re.sub(r"\b(Consume|Shield|Granted)\b", "", name) if "Potion" in name else name
        b["name"] = re.sub(r"\s+", " ", name).strip()


def attach_ability_icons(damage_abilities, perks, skills, spells):
    """Adds "icon" (file under data/icons/ability) to abilities/perks/skills/spells and tidies
    damage-ability names ("BoltofDarkness_Granted" -> "Bolt of Darkness")."""
    icon_dir = os.path.join(ROOT, "data", "icons", "ability")
    if not os.path.isdir(icon_dir):
        return
    by_norm = {}
    for f in sorted(os.listdir(icon_dir)):
        if f.endswith(".webp"):
            stem = f[:-5]
            kind, _, name = stem.removeprefix("Icon_").partition("_")
            by_norm.setdefault((kind, norm(name)), stem)
            by_norm.setdefault(("*", norm(name)), stem)

    def find(key, kinds):
        base = key
        while True:
            nb = ABILITY_SUFFIXES.sub("", base)
            if nb == base:
                break
            base = nb
        cands = [base, base.removeprefix("Summon"), base.removesuffix("Spell"), base.rstrip("0123456789"), base.split("_")[0]]
        for c in cands:
            for k in kinds:
                hit = by_norm.get((k, norm(c)))
                if hit:
                    return hit, base
        return None, base

    def tidy(base):
        return humanize(re.sub(r"(?<=[a-z])(of|Of)(?=[A-Z])", "_of_", base))

    for key, a in damage_abilities.items():
        kinds = ["Skill", "Spell", "Music", "*"] if a.get("kind") == "skill" else ["Spell", "Skill", "Music", "*"]
        icon, base = find(key, kinds)
        a["icon"] = icon
        a["name"] = tidy(base)
    for table, kinds in ((perks, ["Perk"]), (skills, ["Skill", "Music", "ShapeShift"]), (spells, ["Spell", "Music"])):
        for key, p in table.items():
            if isinstance(p, dict):
                p["icon"] = find(key.split("_", 2)[-1] if key.startswith("Id_") else key, kinds)[0]


def humanize(s):
    s = s.replace("_", " ")
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])", " ", s)
    s = re.sub(r"(?<=[a-zA-Z])(?=\d)", " ", s)
    return re.sub(r"\s+", " ", s).strip()


def norm(s):
    return re.sub(r"[^a-z0-9]", "", s.lower())


def asset_name(ref):
    """'/Game/.../Id_Item_X_5001.Id_Item_X_5001' -> 'Id_Item_X_5001' (or None)."""
    if isinstance(ref, dict):
        ref = ref.get("AssetPathName") or ref.get("ObjectPath") or ref.get("PrimaryAssetName") or ""
    if not ref:
        return None
    return ref.rsplit("/", 1)[-1].split(".")[0]


def tag(v):
    if isinstance(v, dict):
        v = v.get("TagName", "")
    return v.rsplit(".", 1)[-1] if v else None


class Raw:
    def __init__(self, build):
        base = os.path.join(ROOT, "data", "raw", build)
        # Older exports kept the full DungeonCrawler/Content/DungeonCrawler prefix.
        legacy = os.path.join(base, "DungeonCrawler", "Content", "DungeonCrawler")
        self.base = legacy if os.path.isdir(legacy) else base
        self.v2 = os.path.join(self.base, "Data", "Generated", "V2")

    def folder(self, rel):
        d = os.path.join(self.v2, rel)
        if not os.path.isdir(d):
            return {}
        out = {}
        for fn in sorted(os.listdir(d)):
            if fn.endswith(".json"):
                with open(os.path.join(d, fn), encoding="utf8") as f:
                    exports = json.load(f)
                out[fn[:-5]] = exports[0].get("Properties", {}) if exports else {}
        return out

    def health(self):
        stats = {}
        for dirpath, _, files in os.walk(self.v2):
            js = [f for f in files if f.endswith(".json")]
            if not js:
                continue
            empty = 0
            for fn in js:
                with open(os.path.join(dirpath, fn), encoding="utf8") as f:
                    if not any(e.get("Properties") for e in json.load(f)):
                        empty += 1
            stats[os.path.relpath(dirpath, self.v2).replace(os.sep, "/")] = [len(js), empty]
        return stats


def load_wiki():
    """Optional English-name / price enrichment cached by fetch_wiki.py."""
    out = {}
    d = os.path.join(ROOT, "data", "wiki")
    if not os.path.isdir(d):
        return out
    for fn in os.listdir(d):
        if not fn.endswith(".json") or fn.startswith("_"):
            continue
        with open(os.path.join(d, fn), encoding="utf8") as f:
            data = json.load(f)
        for group in data.values():
            if not isinstance(group, dict):
                continue
            for name, entry in group.items():
                if isinstance(entry, dict) and entry.get("name"):
                    out[norm(entry["name"])] = entry
    return out


def build(build_id):
    raw = Raw(build_id)
    wiki = load_wiki()

    # ---- property types -------------------------------------------------
    prop_types = {}
    for aid, p in raw.folder("ItemProperty/ItemPropertyType").items():
        key = aid.replace("Id_ItemPropertyType_", "").replace("Effect_", "")
        ratio = p.get("ValueRatio")
        prop_types[key] = {
            "name": PROP_NAME_OVERRIDES.get(key, humanize(key)),
            "pct": bool(ratio and ratio < 1),
            "ratio": ratio or 1,
        }

    def props(arr):
        out = []
        for e in arr or []:
            key = (asset_name(e.get("PropertyTypeId")) or "").replace("Id_ItemPropertyType_", "").replace("Effect_", "")
            row = {"stat": key, "min": e.get("MinValue", 0), "max": e.get("MaxValue", 0)}
            if e.get("PropertyRate"):
                row["weight"] = e["PropertyRate"]
            if e.get("EnchantMinValue") or e.get("EnchantMaxValue"):
                row["enchMin"], row["enchMax"] = e.get("EnchantMinValue"), e.get("EnchantMaxValue")
            out.append(row)
        return out

    item_props = {aid: props(p.get("ItemPropertyItemArray")) for aid, p in raw.folder("ItemProperty/ItemProperty").items()}
    secondary_pools = {aid.replace("Id_ItemProperty_Secondary_", ""): v for aid, v in item_props.items() if "_Secondary_" in aid}
    requirements = {
        aid.replace("Id_ItemRequirement_", ""): sorted({asset_name(c).replace("Id_PlayerCharacter_", "") for c in p.get("ClassRequirements", []) if asset_name(c)})
        for aid, p in raw.folder("Item/ItemRequirement").items()
    }

    # ---- item icons (exported by `DadExtractor icons` / `artmap` into data/icons) ----
    icon_dir = os.path.join(ROOT, "data", "icons")
    icon_names = {f[:-5] for f in os.listdir(icon_dir) if f.endswith(".webp")} if os.path.isdir(icon_dir) else set()
    icon_bases = {}  # item asset id -> [icon base names from its art data]
    art_tsv = os.path.join(icon_dir, "_artmap.tsv")
    if os.path.exists(art_tsv):
        with open(art_tsv, encoding="utf8") as fh:
            for line in fh:
                cols = line.rstrip("\n").split("\t")
                if len(cols) >= 3 and cols[2] and not cols[2].startswith("error"):
                    icon_bases.setdefault(cols[0], []).extend(cols[2].split(","))

    def resolve_icon(aid, base, suffix):
        """Exact '<icon>_<rarity>' first, then the closest lower rarity, then the bare name."""
        bases = icon_bases.get(aid, []) + [f"Item_Icon_{base}"]
        # reskins ("RubysilverFineCuirass") fall back to the base item's art ("FineCuirass")
        words = re.findall(r"[A-Z][a-z0-9]*", base)
        if base.startswith("Rubysilver"):  # Rubysilver sets have their own recolored art: <Base>_RS
            bases.insert(0, f"Item_Icon_{base[len('Rubysilver'):]}_RS")
        bases +=[f"Item_Icon_{''.join(words[i:])}" for i in range(1, len(words))]
        r = int(suffix) if suffix else 0
        for b in bases:
            if suffix and f"{b}_{suffix}" in icon_names:
                return f"{b}_{suffix}"
            lower = sorted((int(n[len(b) + 1:]), n) for n in icon_names
                           if n.startswith(b + "_") and n[len(b) + 1:].isdigit() and int(n[len(b) + 1:]) <= r)
            if lower:
                return lower[-1][1]
            if b in icon_names:
                return b
        return None

    # ---- items, grouped into families with one variant per rarity --------
    families = {}
    for aid, p in raw.folder("Item/Item").items():
        if not p:
            continue
        m = re.match(r"Id_Item_(.+?)(?:_(\d{4}))?$", aid)
        base, suffix = m.group(1), m.group(2)
        rarity = tag(p.get("RarityType")) or "None"
        fam = families.get(base)
        if fam is None:
            w = wiki.get(norm(humanize(base))) or wiki.get(norm(base)) or {}
            fam = families[base] = {
                "id": base,
                "name": w.get("name") or humanize(base),
                "nameSource": "wiki" if w else "id",
                "itemType": (p.get("ItemType") or "").split("::")[-1],
                "slot": tag(p.get("SlotType")),
                "hand": tag(p.get("HandType")),
                "armorType": tag(p.get("ArmorType")),
                "weaponTypes": [tag(t) for t in p.get("WeaponTypes", [])],
                "utilityType": tag(p.get("UtilityType")),
                "miscType": tag(p.get("MiscType")),
                "accessoryType": tag(p.get("AccessoryType")),
                "maxCount": p.get("MaxCount"),
                "classes": [],
                "variants": {},
            }
            if w:
                fam["wiki"] = {k: w[k] for k in ("flavortext", "invwidth", "invheight", "sellprices", "numenchants", "gearscore", "iscraftableby", "incraftingrecipefor", "isartifact") if k in w}
                attacks = wiki_attacks(w)
                if attacks:
                    fam["attacks"] = attacks
        key = f"{base}_{suffix}" if suffix else base
        primary = asset_name(p.get("PrimaryPropertyDataAsset")) or f"Id_ItemProperty_Primary_{key}"
        fam["variants"][rarity] = {
            "assetId": aid,
            "props": item_props.get(primary, []),
            "classes": requirements.get(key, []),
            "icon": resolve_icon(aid, base, suffix),
        }
        for c in requirements.get(key, []):
            if c not in fam["classes"]:
                fam["classes"].append(c)

    for fam in families.values():
        fam["variants"] = dict(sorted(fam["variants"].items(), key=lambda kv: RARITIES.index(kv[0]) if kv[0] in RARITIES else 99))
        fam["classes"] = sorted(c for c in fam["classes"] if not c.startswith("GrandMaster"))
        fam["secondaryPool"] = pick_secondary_pool(fam, secondary_pools)

    # ---- crafting recipes ----------------------------------------------
    def item_ref(ref):
        a = asset_name(ref)
        if not a or not a.startswith("Id_Item_"):
            return None
        m = re.match(r"Id_Item_(.+?)(?:_(\d{4}))?$", a)
        base, suffix = m.group(1), m.group(2)
        fam = families.get(base)
        rarity = None
        if fam:
            for r, v in fam["variants"].items():
                if v["assetId"] == a:
                    rarity = r
        return {"item": base, "rarity": rarity}

    recipes = []
    for aid, p in raw.folder("Merchant/StockCraft").items():
        merchant = re.sub(r"^Id_StockCraft_|_\d+$", "", aid)
        for e in p.get("StockCraftItemArray", []):
            result = item_ref(e.get("ItemId"))
            if not result:
                continue
            ing = []
            for i in range(1, 6):
                r = item_ref(e.get(f"RequiredItemId_{i:02d}"))
                amt = e.get(f"RequiredAmount{i:02d}", 0)
                if r and amt:
                    ing.append({**r, "amount": amt})
            recipes.append({"merchant": merchant, "uid": e.get("UniqueID"), "result": result, "ingredients": ing})

    # ---- curves ----------------------------------------------------------
    curves = {}
    ga = os.path.join(raw.base, "Data", "GameplayAbility")
    if os.path.isdir(ga):
        for fn in sorted(os.listdir(ga)):
            if fn.startswith("CT_") and fn.endswith(".json"):
                with open(os.path.join(ga, fn), encoding="utf8") as f:
                    for e in json.load(f):
                        for row, v in e.get("Rows", {}).items():
                            curves[f"{fn[:-5]}.{row}"] = [[round(k["Time"], 4), round(k["Value"], 5)] for k in v.get("Keys", [])]
    derived = [
        {"key": k, "label": label, "curve": f"{t}.{r}", "inputs": inputs, "unit": unit,
         "weightsSource": "game" if all(w == 1 for w in inputs.values()) else "wiki"}
        for k, label, (t, r), inputs, unit in DERIVED if f"{t}.{r}" in curves
    ]

    # ---- classes, perks, skills, spells ---------------------------------
    effects = raw.folder("PlayerCharacter/PlayerCharacterEffect")
    classes = {}
    for aid, p in raw.folder("PlayerCharacter/PlayerCharacter").items():
        cid = aid.replace("Id_PlayerCharacter_", "")
        if cid.startswith("GrandMaster") or cid == "Tutorial" or not p.get("CanUse", True):
            continue
        eff = effects.get(f"Id_PlayerCharacterEffect_{cid}", {})
        if "StrengthBase" not in eff:
            continue
        classes[cid] = {
            "name": humanize(cid),
            "attributes": {a: eff.get(f"{a}Base", 0) for a in ATTRS},
            "moveSpeed": eff.get("MoveSpeedMod"),
            "perks": [], "skills": [], "spells": [],
        }

    def owners(p):
        return [asset_name(c).replace("Id_PlayerCharacter_", "") for c in p.get("Classes", []) if asset_name(c)]

    def abilities(folder, prefix, kind, extra):
        out = {}
        for aid, p in raw.folder(folder).items():
            if not p or p.get("CanUse") is False:
                continue
            key = aid.replace(prefix, "")
            row = {"name": humanize(key), "classes": [c for c in owners(p) if c in classes]}
            row.update({k2: f(p) for k2, f in extra.items() if f(p) is not None})
            out[key] = row
            for c in row["classes"]:
                classes[c][kind].append(key)
        return out

    perks = abilities("Perk/Perk", "Id_Perk_", "perks", {})
    skills = abilities("Skill/Skill", "Id_Skill_", "skills", {"tier": lambda p: p.get("SkillTier"), "type": lambda p: tag(p.get("SkillType"))})
    spells = abilities("Spell/Spell", "Id_Spell_", "spells", {
        "tier": lambda p: p.get("SpellTier"), "castTime": lambda p: p.get("CastingTime"),
        "cooldown": lambda p: p.get("CooldownDuration"), "charges": lambda p: p.get("MaxCount"),
        "source": lambda p: tag(p.get("SourceType")),
    })
    for c in classes.values():
        for k in ("perks", "skills", "spells"):
            c[k].sort()

    constants = {}
    for aid, p in raw.folder("Constant/Constant").items():
        v = p.get("FloatValue", p.get("Int32Value"))
        if v is not None:
            constants[aid.replace("Id_Constant_", "")] = v

    buffs = build_buffs(raw, classes)
    skins = build_skins(raw, buffs)
    damage_abilities = build_damage_abilities(raw, classes)
    attach_ability_icons(damage_abilities, perks, skills, spells)
    attach_buff_icons(buffs, families)
    health = raw.health()
    db = {
        "meta": {
            "build": build_id,
            "generatedAt": datetime.now(timezone.utc).isoformat(timespec="seconds"),
            "counts": {"itemFamilies": len(families), "itemVariants": sum(len(f["variants"]) for f in families.values()),
                       "recipes": len(recipes), "perks": len(perks), "skills": len(skills), "spells": len(spells),
                       "curves": len(curves), "constants": len(constants), "buffs": len(buffs),
                       "damageAbilities": len(damage_abilities)},
            "decodeHealth": {
                "assets": sum(t for t, _ in health.values()),
                "empty": sum(e for _, e in health.values()),
                "brokenFolders": {k: v for k, v in sorted(health.items()) if v[1] / v[0] > 0.25},
            },
            # no hand-typed corrections: health comes only from the game's CT_MaxHealthBase curve
            "notes": {},
        },
        "rarities": RARITIES,
        "attributes": ATTRS,
        "propertyTypes": prop_types,
        "items": dict(sorted(families.items())),
        "secondaryPools": secondary_pools,
        "recipes": recipes,
        "curves": curves,
        "derived": derived,
        "classes": dict(sorted(classes.items())),
        "perks": perks, "skills": skills, "spells": spells,
        "buffs": buffs,
        "skins": skins,
        "damageAbilities": damage_abilities,
        "constants": constants,
    }
    return db


def wiki_attacks(w):
    """Melee combo data (combo %, impact zones) from the wiki's weapon abilities."""
    def pct(s):
        try:
            return float(str(s).rstrip("%")) / 100
        except ValueError:
            return None
    out = []
    for group in ("Primary", "Special"):
        g = (w.get("abilities") or {}).get(group) or {}
        for name in g.get("order", []):
            a = g.get(name) or {}
            if "combodamage" not in a:
                continue
            anim = (((w.get("animationtimes") or {}).get(group) or {}).get(name) or {})
            windup = anim.get("Windup")
            cycle = None
            if isinstance(windup, (int, float)):
                cycle = windup + (anim.get("Hit") or 0) + (anim.get("Recover") or 0)
            elif isinstance(anim.get("Animation Time"), (int, float)):
                cycle = anim["Animation Time"]
            out.append({
                "name": name if group == "Primary" else f"{name}",
                "group": group,
                "damageType": a.get("damagetype"),
                "combo": pct(a["combodamage"]),
                "zones": [z for z in (pct(x) for x in a.get("impactzones", [])) if z is not None] or [1.0],
                "windup": windup if isinstance(windup, (int, float)) else None,
                "cycle": cycle,
            })
    return out


def pick_secondary_pool(fam, pools):
    slot, hand = fam["slot"], fam["hand"]
    simple = {"Chest": "Chest", "Back": "Cloak", "Foot": "Foot", "Hands": "Hands", "Head": "Head",
              "Legs": "Legs", "Necklace": "Necklace", "Ring": "Ring"}
    if slot in simple:
        return simple[slot]
    if slot not in ("Primary", "Secondary"):
        return None
    stats = {p["stat"] for v in fam["variants"].values() for p in v["props"]}
    shield = "Shield" in fam["weaponTypes"] or "ArmorRating" in stats and "PhysicalWeaponDamage" not in stats
    if hand == "OneHanded":
        return "Onehand_Shield" if shield else "Onehand_Weapon"
    if shield:
        return "Twohand_Shield"
    phys, mag = "PhysicalWeaponDamage" in stats, "MagicalWeaponDamage" in stats
    if mag and phys:
        return "Twohand_HybridWeapon"
    if mag:
        return "Twohand_MagicOnly"
    return "Twohand_PhysicalWeapon"


def main():
    build_id = sys.argv[1]
    db = build(build_id)
    os.makedirs(os.path.join(ROOT, "data", "builds"), exist_ok=True)
    out = os.path.join(ROOT, "data", "builds", f"{build_id}.json")
    with open(out, "w", encoding="utf8") as f:
        json.dump(db, f, indent=1, ensure_ascii=False)
    os.makedirs(os.path.join(ROOT, "app", "data"), exist_ok=True)
    with open(os.path.join(ROOT, "app", "data", "game.json"), "w", encoding="utf8") as f:
        json.dump(db, f, separators=(",", ":"), ensure_ascii=False)
    with open(os.path.join(ROOT, "app", "data", "game.js"), "w", encoding="utf8") as f:
        f.write("window.DAD_DATA = ")
        json.dump(db, f, separators=(",", ":"), ensure_ascii=False)
        f.write(";\n")
    c = db["meta"]["counts"]
    h = db["meta"]["decodeHealth"]
    print(f"{out}: {c}")
    print(f"decode health: {h['assets'] - h['empty']}/{h['assets']} assets decoded; {len(h['brokenFolders'])} folders mostly broken")


if __name__ == "__main__":
    main()
