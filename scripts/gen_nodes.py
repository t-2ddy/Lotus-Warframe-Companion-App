"""Generate src-tauri/data/nodes.json from the Warframe wiki's mission data module.

Keeps only facts the app needs per node: internal tag, name, planet, mastery XP, and whether
the node counts toward star chart progress (IsTracked). Source: Warframe wiki,
Module:Missions/data (CC BY-SA 3.0) — credited in the app and docs.

Usage: python scripts/gen_nodes.py [path to downloaded module .lua]
       (downloads it if no path is given)
"""
import json
import os
import re
import sys
import urllib.request

URL = "https://wiki.warframe.com/w/Module:Missions/data?action=raw"
OUT = os.path.join(os.path.dirname(__file__), "..", "src-tauri", "data", "nodes.json")


def field(line: str, key: str, pattern: str):
    m = re.search(rf"\b{key}\s*=\s*{pattern}", line)
    return m.group(1) if m else None


def enemy(line: str) -> str:
    """Enemy is either "Grineer" or a table like {"Infested", "Grineer"}."""
    single = field(line, "Enemy", r'"([^"]*)"')
    if single is not None:
        return single
    table = field(line, "Enemy", r"\{([^}]*)\}")
    return " / ".join(re.findall(r'"([^"]+)"', table)) if table else ""


def parse(lua: str) -> list[dict]:
    nodes, seen = [], set()
    for line in lua.splitlines():
        tag = field(line, "InternalName", r'"([^"]+)"')
        if not tag or tag in seen:
            continue
        seen.add(tag)
        nodes.append(
            {
                "tag": tag,
                "name": field(line, "Name", r'"([^"]*)"') or tag,
                "planet": field(line, "Planet", r'"([^"]*)"') or "",
                "type": field(line, "Type", r'"([^"]*)"') or "",
                "enemy": enemy(line),
                "xp": int(field(line, "MasteryExp", r"(\d+)") or 0),
                "tracked": field(line, "IsTracked", r"(true|false)") == "true",
            }
        )
    return nodes


def main() -> None:
    if len(sys.argv) > 1:
        lua = open(sys.argv[1], encoding="utf-8").read()
    else:
        req = urllib.request.Request(URL, headers={"User-Agent": "wf-companion-dev/0.1"})
        lua = urllib.request.urlopen(req, timeout=30).read().decode("utf-8")
    nodes = parse(lua)
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        json.dump({"source": "Warframe wiki, Module:Missions/data (CC BY-SA 3.0)", "nodes": nodes}, f, indent=1)
    counted = [n for n in nodes if n["tracked"] and n["xp"] > 0]
    print(f"{len(nodes)} nodes, {len(counted)} tracked with mastery XP, total {sum(n['xp'] for n in counted):,} XP -> {OUT}")


if __name__ == "__main__":
    main()
