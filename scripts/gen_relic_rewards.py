"""Generate src-tauri/data/relic_rewards.json: every relic's rewards, as a fallback for WFCD.

WFCD's Relics.json lists each relic's rewards with item paths, but on 2026-10-23 (commit
5bbcd29684) every relic came out with "rewards": []. This snapshot keeps the app working when
that happens again. Two sources:

- Relic tables (Intact chances): WFCD drop data, https://drops.warframestat.us/data/relics.json,
  built from DE's official drop tables. It names items but has no paths.
- Reward name -> item path: the rewards of a WFCD Relics.json that still has them.

Usage: python scripts/gen_relic_rewards.py [WFCD git ref with rewards, default master]
"""
import json
import os
import sys
import urllib.request

DROPS_URL = "https://drops.warframestat.us/data/relics.json"
WFCD_URL = "https://raw.githubusercontent.com/WFCD/warframe-items/{ref}/data/json/Relics.json"
OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "data", "relic_rewards.json")


def fetch(url: str):
    # drops.warframestat.us refuses Python's default user agent.
    req = urllib.request.Request(url, headers={"User-Agent": "Lotus-Warframe-Companion"})
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read().decode("utf-8"))


def main() -> None:
    ref = sys.argv[1] if len(sys.argv) > 1 else "master"
    wfcd = fetch(WFCD_URL.format(ref=ref))
    paths: dict[str, str] = {}
    for relic in wfcd:
        for r in relic.get("rewards") or []:
            item = r.get("item") or {}
            if item.get("name") and item.get("uniqueName"):
                paths.setdefault(item["name"], item["uniqueName"])
    if not paths:
        sys.exit(f"WFCD Relics.json at {ref} has no rewards; pass an older ref")

    drops = fetch(DROPS_URL)["relics"]
    relics: dict[str, list] = {}
    for relic in drops:
        if relic.get("state") != "Intact" or not relic.get("relicName"):
            continue
        name = f"{relic['tier']} {relic['relicName']}"
        relics[name] = [[r["itemName"], r["chance"]] for r in relic["rewards"]]

    used = {n for rewards in relics.values() for n, _ in rewards}
    missing = sorted(used - paths.keys())
    if missing:
        print(f"no path for {len(missing)} reward names: {missing}")
    out = {
        "source": f"{DROPS_URL} + WFCD Relics.json @ {ref}",
        "paths": {n: paths[n] for n in sorted(used) if n in paths},
        "relics": dict(sorted(relics.items())),
    }
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, ensure_ascii=False, separators=(",", ":"))
        f.write("\n")
    print(f"wrote {len(relics)} relics, {len(out['paths'])} paths to {os.path.normpath(OUT)}")


if __name__ == "__main__":
    main()
