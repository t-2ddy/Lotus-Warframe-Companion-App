"""Generate the Chat page's emoji list: src/lib/emoji.json and static/emoji/<code>.png.

Source: the "Chat Emoji Codes" table on the Warframe wiki's Chat page
(https://wiki.warframe.com/w/Chat), read as wikitext; images are the table's default ones,
found through the wiki's API. Rerun after the game adds emojis.

Usage: python scripts/gen_emoji.py
"""
import json
import os
import re
import urllib.parse
import urllib.request

WIKI = "https://wiki.warframe.com"
ROOT = os.path.join(os.path.dirname(__file__), "..")
OUT_JSON = os.path.join(ROOT, "src", "lib", "emoji.json")
OUT_DIR = os.path.join(ROOT, "static", "emoji")

# | Angry || <code>:angry:</code> || <code>>:(</code> || [[File:AngryEmoji.png|class=icon|32px]]
ROW = re.compile(
    r"^\|\s*(?P<name>[^|]+?)\s*\|\|\s*<code>:(?P<code>[a-z0-9_]+):</code>\s*\|\|\s*(?P<short>.*?)\s*\|\|\s*\[\[File:(?P<file>[^|\]]+)"
)


def get(url: str) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "Lotus-Warframe-Companion"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read()


def main() -> None:
    text = get(f"{WIKI}/w/Chat?action=raw").decode("utf-8")
    rows = []
    for line in text.splitlines():
        m = ROW.match(line)
        if not m:
            continue
        short = re.sub(r"</?(code|nowiki)>", "", m["short"]).strip()
        rows.append({"name": m["name"], "code": m["code"], "shortcut": short or None, "file": m["file"]})
    if not rows:
        raise SystemExit("no emoji rows found: the wiki table changed")

    # File name -> image URL, 50 titles per API call.
    urls: dict[str, str] = {}
    files = sorted({r["file"] for r in rows})
    for i in range(0, len(files), 50):
        titles = "|".join(f"File:{f}" for f in files[i : i + 50])
        q = urllib.parse.urlencode(
            {"action": "query", "titles": titles, "prop": "imageinfo", "iiprop": "url", "format": "json"}
        )
        doc = json.loads(get(f"{WIKI}/api.php?{q}"))
        for page in doc["query"]["pages"].values():
            info = page.get("imageinfo")
            if info:
                urls[page["title"].removeprefix("File:")] = info[0]["url"]

    os.makedirs(OUT_DIR, exist_ok=True)
    out = []
    for r in rows:
        url = urls.get(r["file"].replace("_", " ")) or urls.get(r["file"])
        if not url:
            print(f"no image for :{r['code']}: ({r['file']})")
            continue
        with open(os.path.join(OUT_DIR, f"{r['code']}.png"), "wb") as f:
            f.write(get(url))
        out.append({"name": r["name"], "code": r["code"], "shortcut": r["shortcut"]})

    with open(OUT_JSON, "w", encoding="utf-8", newline="\n") as f:
        json.dump(out, f, indent=1, ensure_ascii=False)
        f.write("\n")
    print(f"{len(out)} emojis -> {OUT_JSON}")


if __name__ == "__main__":
    main()
