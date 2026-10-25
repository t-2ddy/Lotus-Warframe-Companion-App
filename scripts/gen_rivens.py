"""Builds src-tauri/data/rivens.json from Warframe Public Export Plus (calamity-inc).

Usage: python scripts/gen_rivens.py <folder with ExportUpgrades.json, ExportWeapons.json, dict.en.json>
       python scripts/gen_rivens.py --de-stats <folder>   (only add DE's newer stats; see below)

Download the three files from
https://cdn.jsdelivr.net/gh/calamity-inc/warframe-public-export-plus@senpai/<file>

Dispositions change on DE's servers without the export changing, so the Warframe wiki's values
(Module:Weapons/data/*, CC BY-SA 3.0) replace the export's where the wiki has one clear value
(kitgun chambers are listed twice with different values and keep the export's). The script
downloads them and saves them as wiki-dispositions.json in the same folder, as the app does.
Weapons the export doesn't have yet (Corufell Prime, October 2026) are added from the wiki as
their family's riven weapon.

Public Export Plus also lags DE's own Public Export for riven stats (October 2026: the spliced
traits and Zoom on shotgun rivens). DE's ExportUpgrades_en.json is downloaded too (saved in the
same folder, as the app does) and any riven stat the export lacks is added from it.

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
import lzma
import re
import struct
import sys
import urllib.request
from pathlib import Path

PREFIX = "/Lotus/Upgrades/Mods/Randomized/"
DEST = Path(__file__).resolve().parent.parent / "src-tauri" / "data" / "rivens.json"

DE_URL = "https://content.warframe.com/PublicExport/"


def de_upgrades(save_dir):
    """DE's ExportUpgrades_en.json (see riven_data.rs). Its index is LZMA without an end marker,
    which Python's lzma only reads in raw mode, stopping at the size in the header."""

    def get(url):
        return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "Lotus gen_rivens.py"})).read()

    packed = get(DE_URL + "index_en.txt.lzma")
    props, dict_size, size = packed[0], struct.unpack("<I", packed[1:5])[0], struct.unpack("<Q", packed[5:13])[0]
    lzma1 = {"id": lzma.FILTER_LZMA1, "dict_size": dict_size, "lc": props % 9, "lp": props // 9 % 5, "pb": props // 45}
    index = lzma.LZMADecompressor(lzma.FORMAT_RAW, filters=[lzma1]).decompress(packed[13:], size).decode("utf-8")
    name = next(l.strip() for l in index.splitlines() if l.startswith("ExportUpgrades_en.json!"))
    body = get(DE_URL + "Manifest/" + name)
    (save_dir / "ExportUpgrades_en.json").write_bytes(body)
    return body


def de_rivens(body):
    """Riven path -> DE's entry. The file has raw line breaks inside strings."""
    text = body.decode("utf-8").replace("\r", " ").replace("\n", " ").replace("\t", " ")
    return {e["uniqueName"]: e for e in json.loads(text)["ExportUpgrades"] if e["uniqueName"].startswith(PREFIX)}


def clean_label(label):
    # "<DT_FIRE_COLOR>Heat" -> "Heat"; one riven stat uses |STAT1| instead of |val|.
    label = re.sub(r"<[^>]+>", "", label).replace("|STAT1|", "|val|")
    return " ".join(label.split())


def de_stat(e):
    """One of DE's upgradeEntries as a stat. Its float32 values (0.0099999998) get the export's
    spelling (0.01): 7 significant digits. Its text is already in English."""
    values = e.get("upgradeValues") or [{}]
    return {
        "value": float(f"{values[0].get('value', 0):.6e}"),
        "prefix": e.get("prefixTag") or "",
        "suffix": e.get("suffixTag") or "",
        "label": clean_label(values[0].get("locTag") or e["tag"]),
    }


SOURCE = "Warframe Public Export Plus (calamity-inc/warframe-public-export-plus); newer riven stats from DE's Public Export; dispositions from the Warframe wiki (Module:Weapons/data, CC BY-SA 3.0); formulas from calamity-inc/warframe-riven-info"

if sys.argv[1] == "--de-stats":
    # python scripts/gen_rivens.py --de-stats <folder to save DE's file in>
    # Only DE's newer riven stats, added to the bundled file as it is. Its weapons and
    # dispositions stay: test builds grade cards captured before every weapon went to 1.2 with
    # them (the wiki now says 1.2 everywhere).
    out = json.loads(DEST.read_text(encoding="utf-8"))
    de = de_rivens(de_upgrades(Path(sys.argv[2])))
    added = 0
    for kind, t in out["types"].items():
        for e in de.get(PREFIX + kind, {}).get("upgradeEntries", []):
            if e["tag"] not in t["stats"]:
                t["stats"][e["tag"]] = de_stat(e)
                added += 1
    out["source"] = SOURCE
    out["built"] = datetime.date.today().isoformat()
    DEST.write_text(json.dumps(out, ensure_ascii=False, separators=(",", ":"), sort_keys=True), encoding="utf-8")
    print(f"added {added} stats from DE's export -> {DEST}")
    sys.exit()

src = Path(sys.argv[1])
upgrades = json.loads((src / "ExportUpgrades.json").read_text(encoding="utf-8"))
weapons = json.loads((src / "ExportWeapons.json").read_text(encoding="utf-8"))
words = json.loads((src / "dict.en.json").read_text(encoding="utf-8"))

WIKI_URL = "https://wiki.warframe.com/w/Module:Weapons/data/{}?action=raw"
WIKI_PAGES = ["primary", "secondary", "melee", "archwing", "companion", "modular"]


def wiki_dispositions():
    """Weapon path -> {disposition, name, family} from the wiki's weapon modules (see
    riven_data.rs)."""
    found = {}
    for page in WIKI_PAGES:
        request = urllib.request.Request(WIKI_URL.format(page), headers={"User-Agent": "Lotus gen_rivens.py"})
        lua = urllib.request.urlopen(request).read().decode("utf-8")
        path = value = None
        name = family = ""
        for line in lua.split("\n"):
            # A weapon opens and closes one tab in; its own fields are two tabs in.
            if line.startswith("\t") and not line.startswith("\t\t"):
                if path is not None and value is not None:
                    found.setdefault(path, []).append({"disposition": value, "name": name, "family": family})
                path = value = None
                name = family = ""
            elif line.startswith('\t\tInternalName = "'):
                path = line.split('"')[1]
            elif line.startswith("\t\tDisposition = "):
                try:
                    value = float(line.split("=", 1)[1].strip().rstrip(","))
                except ValueError:
                    value = None
            elif line.startswith('\t\tName = "'):
                name = line.split('"')[1]
            elif line.startswith('\t\tFamily = "'):
                family = line.split('"')[1]
    return {
        p: v[0]
        for p, v in sorted(found.items())
        if all(abs(x["disposition"] - v[0]["disposition"]) < 1e-9 for x in v) and 0.5 <= v[0]["disposition"] <= 1.6
    }


dispositions = wiki_dispositions()
(src / "wiki-dispositions.json").write_text(json.dumps(dispositions, separators=(",", ":")), encoding="utf-8")

de = de_rivens(de_upgrades(src))


def text(tag):
    return words.get(tag) if tag else None


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
    # Stats only DE's export has yet.
    for e in de.get(path, {}).get("upgradeEntries", []):
        stats.setdefault(e["tag"], de_stat(e))
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
        disposition = dispositions[path]["disposition"] if path in dispositions else w["omegaAttenuation"]
        entry = {"name": text(w.get("name")) or path.rsplit("/", 1)[-1], "disposition": disposition}
        base = base_of(path)
        if base:
            entry["kind"] = riven_for[base]
            entry["base"] = base
        riven_weapons[path] = entry

# Weapons newer than the export: the riven type and base of their family's riven weapon (the
# base weapon's own entry first).
family = {}
for path, entry in sorted(riven_weapons.items(), key=lambda pe: (pe[1].get("base") != pe[0], pe[0])):
    w = dispositions.get(path)
    if "kind" in entry and w and w["family"]:
        family.setdefault(w["family"], (entry["kind"], entry["base"]))
for path, w in dispositions.items():
    if path not in weapons and w["name"] and w["family"] in family:
        kind, base = family[w["family"]]
        riven_weapons[path] = {"name": w["name"], "disposition": w["disposition"], "kind": kind, "base": base}

out = {
    "source": SOURCE,
    "built": datetime.date.today().isoformat(),
    "types": types,
    "veiled": veiled,
    "weapons": riven_weapons,
}
dest = DEST
dest.write_text(json.dumps(out, ensure_ascii=False, separators=(",", ":"), sort_keys=True), encoding="utf-8")
print(f"{len(types)} riven types, {len(veiled)} veiled, {len(riven_weapons)} weapons -> {dest} ({dest.stat().st_size // 1024} KB)")
