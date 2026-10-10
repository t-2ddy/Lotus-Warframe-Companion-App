"""Builds src-tauri/data/rivens.json from Warframe Public Export Plus (calamity-inc).

Usage: python scripts/gen_rivens.py <folder with ExportUpgrades.json, ExportWeapons.json, dict.en.json>

Download the three files from
https://cdn.jsdelivr.net/gh/calamity-inc/warframe-public-export-plus@senpai/<file>

Output:
  types:   riven type (e.g. LotusRifleRandomModRare) -> { label, stats: { tag -> stat } }
           stat = { value, prefix, suffix, label }  (label uses |val| for the number)
  veiled:  veiled riven path -> label
  weapons: weapon path -> { name, disposition, kind?, base? }  (only weapons with a riven
           disposition). kind = the riven type for the weapon; base = the weapon a riven is for
           (Latron Prime -> Latron). Found by walking parentName up to a riven type's
           compatibleItems; pets, Amps and modular parts other than Kitgun chambers and Zaw
           strikes have neither.
  built:   the date this file was made (the app keeps the newer of this and its own download)
"""

import datetime
import json
import re
import sys
from pathlib import Path

src = Path(sys.argv[1])
upgrades = json.loads((src / "ExportUpgrades.json").read_text(encoding="utf-8"))
weapons = json.loads((src / "ExportWeapons.json").read_text(encoding="utf-8"))
words = json.loads((src / "dict.en.json").read_text(encoding="utf-8"))

PREFIX = "/Lotus/Upgrades/Mods/Randomized/"


def text(tag):
    return words.get(tag) if tag else None


def clean_label(label):
    # "<DT_FIRE_COLOR>Heat" -> "Heat"; one riven stat uses |STAT1| instead of |val|.
    label = re.sub(r"<[^>]+>", "", label).replace("|STAT1|", "|val|")
    return " ".join(label.split())


types, veiled, riven_for = {}, {}, {}
for path, entry in upgrades.items():
    if not path.startswith(PREFIX):
        continue
    kind = path[len(PREFIX):]
    label = text(entry.get("name")) or kind
    if kind.startswith("Raw"):
        veiled[path] = label
        continue
    stats = {}
    for e in entry.get("upgradeEntries", []):
        values = e.get("upgradeValues") or [{}]
        loc = text(values[0].get("locTag"))
        stats[e["tag"]] = {
            "value": values[0].get("value", 0),
            "prefix": text(e.get("prefixTag")) or "",
            "suffix": text(e.get("suffixTag")) or "",
            "label": clean_label(loc) if loc else e["tag"],
        }
    types[kind] = {"label": label, "stats": stats}
    for item in entry.get("compatibleItems", []):
        riven_for[item] = kind


def base_of(path):
    """The weapon in a riven type's compatibleItems that `path` descends from."""
    for _ in range(16):
        if path is None:
            return None
        if path in riven_for:
            return path
        path = weapons.get(path, {}).get("parentName")
    return None


riven_weapons = {}
for path, w in weapons.items():
    if "omegaAttenuation" in w:
        entry = {"name": text(w.get("name")) or path.rsplit("/", 1)[-1], "disposition": w["omegaAttenuation"]}
        base = base_of(path)
        if base:
            entry["kind"] = riven_for[base]
            entry["base"] = base
        riven_weapons[path] = entry

out = {
    "source": "Warframe Public Export Plus (calamity-inc/warframe-public-export-plus); formulas from calamity-inc/warframe-riven-info",
    "built": datetime.date.today().isoformat(),
    "types": types,
    "veiled": veiled,
    "weapons": riven_weapons,
}
dest = Path(__file__).resolve().parent.parent / "src-tauri" / "data" / "rivens.json"
dest.write_text(json.dumps(out, ensure_ascii=False, separators=(",", ":"), sort_keys=True), encoding="utf-8")
print(f"{len(types)} riven types, {len(veiled)} veiled, {len(riven_weapons)} weapons -> {dest} ({dest.stat().st_size // 1024} KB)")
