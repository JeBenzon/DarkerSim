"""Cache the spellsandguns wiki's datamined JSON pages (English names, sell prices,
inventory size, enchant counts) into data/wiki/. Optional enrichment only: the game
files stay the source of truth for stats.

    python scripts/fetch_wiki.py
"""
import json
import os
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = "https://darkanddarker.wiki.spellsandguns.com/Data:{}.json?action=raw"
PAGES = ["Weapon", "Armor", "Accessory", "Utility", "Misc", "Merchant", "Enchantments", "Monster"]


def main():
    out = os.path.join(ROOT, "data", "wiki")
    os.makedirs(out, exist_ok=True)
    for page in PAGES:
        req = urllib.request.Request(BASE.format(page), headers={"User-Agent": "DaD-companion/0.1 (personal tool)"})
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                body = r.read().decode("utf8")
            json.loads(body)  # validate
        except Exception as e:  # keep the previous cache on failure
            print(f"  {page}: skipped ({e})")
            continue
        with open(os.path.join(out, f"{page}.json"), "w", encoding="utf8") as f:
            f.write(body)
        print(f"  {page}: {len(body):,} bytes")


if __name__ == "__main__":
    main()
