"""Compare two normalized builds and write a human-readable patch changelog.

    python scripts/diff_builds.py build-80 build-81

Writes data/changelogs/<old>_to_<new>.md and .json, and app/data/changelog.js
(the latest diff, so the companion app can show "what changed this patch").
"""
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def load(build):
    with open(os.path.join(ROOT, "data", "builds", f"{build}.json"), encoding="utf8") as f:
        return json.load(f)


def load_filelist(build):
    p = os.path.join(ROOT, "data", "raw", build, "_filelist.tsv")
    if not os.path.exists(p):
        return None
    with open(p, encoding="utf8") as f:
        return {line.split("\t")[0] for line in f if line.strip()}


def fmt_props(props, pt):
    def one(p):
        name = pt.get(p["stat"], {}).get("name", p["stat"])
        rng = f"{p['min']}" if p["min"] == p["max"] else f"{p['min']}~{p['max']}"
        return f"{name} {rng}"
    return ", ".join(one(p) for p in props) or "(none)"


def diff(old, new):
    pt = new["propertyTypes"]
    ch = {"items": [], "recipes": [], "curves": [], "classes": [], "constants": [], "abilities": [], "files": {}}

    oi, ni = old["items"], new["items"]
    for k in sorted(set(ni) - set(oi)):
        ch["items"].append({"kind": "added", "item": k, "name": ni[k]["name"]})
    for k in sorted(set(oi) - set(ni)):
        ch["items"].append({"kind": "removed", "item": k, "name": oi[k]["name"]})
    for k in sorted(set(oi) & set(ni)):
        a, b = oi[k], ni[k]
        for r in sorted(set(a["variants"]) | set(b["variants"]), key=lambda r: new["rarities"].index(r) if r in new["rarities"] else 99):
            va, vb = a["variants"].get(r), b["variants"].get(r)
            if va is None:
                ch["items"].append({"kind": "rarity-added", "item": k, "name": b["name"], "rarity": r, "after": fmt_props(vb["props"], pt)})
            elif vb is None:
                ch["items"].append({"kind": "rarity-removed", "item": k, "name": a["name"], "rarity": r})
            elif va["props"] != vb["props"]:
                ch["items"].append({"kind": "stats", "item": k, "name": b["name"], "rarity": r,
                                    "before": fmt_props(va["props"], pt), "after": fmt_props(vb["props"], pt)})
        if a["classes"] != b["classes"]:
            ch["items"].append({"kind": "classes", "item": k, "name": b["name"], "before": ", ".join(a["classes"]), "after": ", ".join(b["classes"])})

    def rkey(r):
        return (r["merchant"], r["result"]["item"], r["result"]["rarity"])

    def rtext(r):
        return " + ".join(f"{i['amount']}x {i['item']}({i['rarity']})" for i in r["ingredients"])

    orc = {rkey(r): r for r in old["recipes"]}
    nrc = {rkey(r): r for r in new["recipes"]}
    for k in sorted(set(nrc) | set(orc), key=str):
        a, b = orc.get(k), nrc.get(k)
        label = f"{k[0]}: {k[1]} ({k[2]})"
        if a is None:
            ch["recipes"].append({"kind": "added", "recipe": label, "after": rtext(b)})
        elif b is None:
            ch["recipes"].append({"kind": "removed", "recipe": label, "before": rtext(a)})
        elif a["ingredients"] != b["ingredients"]:
            ch["recipes"].append({"kind": "changed", "recipe": label, "before": rtext(a), "after": rtext(b)})

    for k in sorted(set(old["curves"]) | set(new["curves"])):
        if old["curves"].get(k) != new["curves"].get(k):
            ch["curves"].append({"curve": k, "before": old["curves"].get(k), "after": new["curves"].get(k)})

    for k in sorted(set(old["classes"]) | set(new["classes"])):
        a, b = old["classes"].get(k), new["classes"].get(k)
        if a is None or b is None:
            ch["classes"].append({"class": k, "kind": "added" if a is None else "removed"})
            continue
        if a["attributes"] != b["attributes"] or a.get("moveSpeed") != b.get("moveSpeed"):
            ch["classes"].append({"class": k, "kind": "attributes", "before": a["attributes"], "after": b["attributes"]})
        for kind in ("perks", "skills", "spells"):
            added, removed = set(b[kind]) - set(a[kind]), set(a[kind]) - set(b[kind])
            if added or removed:
                ch["classes"].append({"class": k, "kind": kind, "added": sorted(added), "removed": sorted(removed)})

    for kind in ("perks", "skills", "spells", "buffs"):
        oa, na = old.get(kind, {}), new.get(kind, {})
        for k in sorted(set(oa) | set(na)):
            if oa.get(k) != na.get(k):
                ch["abilities"].append({"type": kind[:-1], "id": k, "before": oa.get(k), "after": na.get(k)})

    for k in sorted(set(old["constants"]) | set(new["constants"])):
        if old["constants"].get(k) != new["constants"].get(k):
            ch["constants"].append({"constant": k, "before": old["constants"].get(k), "after": new["constants"].get(k)})

    fo, fn = load_filelist(old["meta"]["build"]), load_filelist(new["meta"]["build"])
    if fo is not None and fn is not None:
        ch["files"] = {"added": sorted(fn - fo), "removed": sorted(fo - fn)}
    return ch


def to_markdown(old, new, ch):
    L = [f"# Patch diff: {old['meta']['build']} -> {new['meta']['build']}", ""]
    counts = {k: len(v) if isinstance(v, list) else sum(len(x) for x in v.values()) for k, v in ch.items()}
    L.append("| Area | Changes |\n|---|---|")
    L += [f"| {k} | {v} |" for k, v in counts.items()]
    L.append("")
    if ch["items"]:
        L.append("## Items")
        for c in ch["items"]:
            r = f" [{c['rarity']}]" if c.get("rarity") else ""
            if c["kind"] in ("stats", "classes"):
                L.append(f"- **{c['name']}**{r} {c['kind']}: {c['before']}  ->  {c['after']}")
            else:
                L.append(f"- **{c['name']}**{r} {c['kind']}" + (f": {c['after']}" if c.get("after") else ""))
        L.append("")
    if ch["recipes"]:
        L.append("## Crafting recipes")
        for c in ch["recipes"]:
            L.append(f"- {c['kind']} **{c['recipe']}**: {c.get('before', '')}{'  ->  ' if c['kind'] == 'changed' else ''}{c.get('after', '')}")
        L.append("")
    if ch["curves"]:
        L.append("## Scaling curves (attribute -> stat)")
        for c in ch["curves"]:
            L.append(f"- **{c['curve']}** changed ({len(c['before'] or [])} -> {len(c['after'] or [])} keys)")
        L.append("")
    if ch["classes"]:
        L.append("## Classes")
        for c in ch["classes"]:
            L.append(f"- **{c['class']}** {c['kind']}: " + json.dumps({k: v for k, v in c.items() if k not in ('class', 'kind')}))
        L.append("")
    if ch["constants"]:
        L.append("## Constants")
        L += [f"- `{c['constant']}`: {c['before']} -> {c['after']}" for c in ch["constants"]]
        L.append("")
    if ch["abilities"]:
        L.append("## Perks / skills / spells")
        for c in ch["abilities"]:
            kind = "added" if c["before"] is None else "removed" if c["after"] is None else "changed"
            L.append(f"- {c['type']} **{c['id']}** {kind}")
        L.append("")
    if ch["files"]:
        L.append(f"## Game files: {len(ch['files']['added'])} added, {len(ch['files']['removed'])} removed")
        L += [f"- + `{f}`" for f in ch["files"]["added"][:300]]
        L += [f"- - `{f}`" for f in ch["files"]["removed"][:300]]
    return "\n".join(L) + "\n"


def main():
    a, b = sys.argv[1], sys.argv[2]
    old, new = load(a), load(b)
    ch = diff(old, new)
    d = os.path.join(ROOT, "data", "changelogs")
    os.makedirs(d, exist_ok=True)
    base = os.path.join(d, f"{a}_to_{b}")
    with open(base + ".md", "w", encoding="utf8") as f:
        f.write(to_markdown(old, new, ch))
    with open(base + ".json", "w", encoding="utf8") as f:
        json.dump(ch, f, indent=1)
    with open(os.path.join(ROOT, "app", "data", "changelog.js"), "w", encoding="utf8") as f:
        f.write("window.DAD_CHANGELOG = ")
        json.dump({"from": a, "to": b, "changes": ch}, f, separators=(",", ":"))
        f.write(";\n")
    print(f"{base}.md")
    for k, v in ch.items():
        print(f"  {k}: {len(v) if isinstance(v, list) else {x: len(y) for x, y in v.items()}}")


if __name__ == "__main__":
    main()
