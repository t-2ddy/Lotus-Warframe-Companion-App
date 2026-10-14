"""Builds the riven card regression set (fixtures/riven-cards/) from real card reads.

Usage: python scripts/riven_cards.py [--aleca <relicLogs dir>] [--lotus <app data dir>]

Defaults: AlecaFrame's %LOCALAPPDATA%/AlecaFrame/relicLogs and Lotus's %APPDATA%/io.lotus.companion.

Truth comes from AlecaFrame's riven logs (*.alecalog: a zip with the game screenshot, the card
crops and a trace). Only reads its own checks accepted are used: every stat's rank came out
the same, so the numbers, stats and weapon variant are right. Each becomes up to three cases:

  aleca-text   AlecaFrame's (server) OCR text through Lotus's parser: the parser on clean text.
  aleca-image  The screenshot through Lotus's crop, cleanup and Windows OCR (the #[ignore]d
               image test): the whole reading path.
  lotus-text   Lotus's own OCR reads of the same card, from overlay.log ("ocr inspect" etc., all
               cleaned-up copies of one read) and riven_misfits.jsonl.

A Lotus read is matched to a truth by the riven's name and two of its numbers appearing in its
text (a reroll can keep the name with new numbers). Reads of a card
still fading in are kept: the overlay re-reads those, and a fix that makes them work out wrong
fails the check.

Cards AlecaFrame never read can be added by hand to fixtures/riven-cards/manual.json (same
shape as a case; a null number is one nobody can know), which is merged in on every run.

Run it before editing the overlay's Rust when overlay.log has a session worth keeping: a running
`tauri dev` restarts on the edit and clears overlay.log.

Output: fixtures/riven-cards/cards.json (merged with what's there, by id) and images/*.jpg.
The folder is gitignored (it holds one account's cards and screenshots).
"""

import hashlib
import json
import os
import re
import sys
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "fixtures" / "riven-cards"
RIVENS = ROOT / "src-tauri" / "data" / "rivens.json"


def arg(name, default):
    if name in sys.argv:
        return Path(sys.argv[sys.argv.index(name) + 1])
    return default


def letters(s):
    return "".join(c for c in s.lower() if c.isalpha())


def label_key(label):
    """`|val|% <DT_FIRE_COLOR>Heat` → `heat`."""
    label = re.sub(r"<[^>]*>", "", label.replace("|val|", "").replace("|STAT1|", ""))
    return letters(label)


def parse_trace(text, data):
    """The verified card in one AlecaFrame trace, or None."""
    text = text.replace("\r", "")
    m = re.search(r"OCR result: (.*?)\n\[", text, re.S)
    parsed = re.search(r"Parsed OCR result: \n(.*?)\n\[", text, re.S)
    kind = re.search(r"Riven mod type found: \S+/(\w+)", text)
    levels = re.search(r"Detected mod level by attributes: (.*)", text)
    if not (m and parsed and kind and levels) or "Exception" in text.split("Detected mod level")[1]:
        return None
    rows = parsed.group(1).strip().split("\n")
    weapon = rows[0].removeprefix("Weapon:").strip()
    riven = rows[1].removeprefix("Riven:").strip()
    attrs = []
    for row in rows[2:]:
        name, _, value = row.strip().lstrip("-").rpartition("|")
        attrs.append((name.strip(), float(value)))
    # Tags in card order: the positives from the name, matched to an OCR line, then the negative.
    kind = kind.group(1)
    stats = data["types"].get(kind, {}).get("stats", {})
    by_key = {label_key(s["label"]): tag for tag, s in stats.items()}
    tag_of = {}
    for tag, value in re.findall(r"\] -(\w+)\n\[[\d.]+\] \t- .*? -> .*? \(([-\d.]+)\)", text):
        tag_of[float(value)] = tag
    neg = re.search(r"\t(\|val\|.*?) -> .*? \(([-\d.]+)\)\n", text)
    if neg:
        tag = by_key.get(label_key(neg.group(1)))
        if not tag:
            # A stat Lotus's data doesn't have (a splice): AlecaFrame's own tag isn't logged
            # for negatives, so keep the label.
            tag = "?" + label_key(neg.group(1))
        tag_of[float(neg.group(2))] = tag
    expected = []
    for name, value in attrs:
        tag = tag_of.get(value)
        if tag is None:
            return None
        expected.append([tag, value])
    tried = re.findall(r"Trying weapon: (.*)", text)
    lv = [float(x) for x in levels.group(1).split()]
    rank = 8 if sum(lv) / len(lv) > 4 else 0
    return {
        "title": f"{weapon} {riven}",
        "riven": riven,
        "kind": kind,
        "weapon": tried[-1].strip() if tried else weapon,
        "stats": expected,
        "rank": rank,
        "ocr": m.group(1).split("\n"),
    }


def weapon_path(data, name):
    for path, w in sorted(data["weapons"].items()):
        if w["name"].lower() == name.lower():
            return path
    return None


def lotus_reads(app):
    """Each logged card read: the texts of its cleaned-up copies, as lines."""
    log = app / "overlay.log"
    if log.exists():
        for line in log.read_text(encoding="utf-8", errors="replace").splitlines():
            m = re.match(r"\d{13} ocr (inspect|reroll-left|reroll-right|reroll-centre) \([^)]*\): (.*)", line)
            if m and m.group(2) != "(nothing)":
                reads = [r.split(" / ") for r in m.group(2).split(" || ") if not r.startswith("(")]
                if reads:
                    yield "overlay.log", reads
    misfits = app / "riven_misfits.jsonl"
    if misfits.exists():
        for line in misfits.read_text(encoding="utf-8").splitlines():
            ocr = json.loads(line).get("ocr")
            if ocr:
                yield "misfits", [ocr.split(" / ")]


def main():
    aleca = arg("--aleca", Path(os.environ.get("LOCALAPPDATA", "")) / "AlecaFrame" / "relicLogs")
    app = arg("--lotus", Path(os.environ.get("APPDATA", "")) / "io.lotus.companion")
    data = json.loads(RIVENS.read_text(encoding="utf-8"))
    (OUT / "images").mkdir(parents=True, exist_ok=True)
    cases_file = OUT / "cards.json"
    cases = {c["id"]: c for c in json.loads(cases_file.read_text(encoding="utf-8"))} if cases_file.exists() else {}

    truths = []
    for log in sorted(aleca.glob("*_RIVEN.alecalog")):
        try:
            z = zipfile.ZipFile(log)
            text = z.read("log.txt").decode("utf-8", "replace")
        except (zipfile.BadZipFile, KeyError):
            continue
        t = parse_trace(text, data)
        if not t:
            continue
        stamp = log.name.removesuffix("_RIVEN.alecalog")
        path = weapon_path(data, t["weapon"])
        expect = {"title": t["title"], "stats": t["stats"], "rank": t["rank"]}
        base = {"type": t["kind"], "weapon": path, "expect": expect}
        cases[f"aleca-text-{stamp}"] = {"id": f"aleca-text-{stamp}", "source": "aleca-text", "reads": [t["ocr"]], **base}
        image = OUT / "images" / f"{stamp}.jpg"
        if not image.exists() and "original.jpeg" in z.namelist():
            image.write_bytes(z.read("original.jpeg"))
        if image.exists():
            cases[f"aleca-image-{stamp}"] = {"id": f"aleca-image-{stamp}", "source": "aleca-image", "image": f"images/{image.name}", **base}
        truths.append((t, base))

    # The same card read by Lotus: the riven's name in the read's text.
    seen = set()
    unmatched = 0
    for origin, reads in lotus_reads(app):
        key = json.dumps(reads)
        if key in seen:
            continue
        seen.add(key)
        raw = " ".join(" ".join(r) for r in reads)
        text = letters(raw)
        numbers = set(re.findall(r"\d+\.?\d*", raw.replace(" ", "")))

        def same_card(t):
            # A reroll can keep the riven's name with new numbers: two of the numbers must match.
            shown = {f"{abs(v):g}" for _, v in t["stats"]}
            return letters(t["riven"]) in text and letters(t["weapon"].split()[0]) in text and len(shown & numbers) >= 2

        match = next((b for t, b in truths if same_card(t)), None)
        if not match:
            unmatched += 1
            continue
        cid = "lotus-text-" + hashlib.sha1(key.encode()).hexdigest()[:10]
        if not any(c.get("reads") == reads for c in cases.values()):
            cases[cid] = {"id": cid, "source": f"lotus-text ({origin})", "reads": reads, **match}

    # Cases written by hand (cards AlecaFrame never read, with the truth worked out from the
    # card and the name), kept in their own file so a rebuild doesn't lose them.
    manual = OUT / "manual.json"
    if manual.exists():
        for c in json.loads(manual.read_text(encoding="utf-8")):
            cases[c["id"]] = c

    out = sorted(cases.values(), key=lambda c: c["id"])
    cases_file.write_text(json.dumps(out, indent=1, ensure_ascii=False), encoding="utf-8")
    by_source = {}
    for c in out:
        s = c["source"].split(" ")[0]
        by_source[s] = by_source.get(s, 0) + 1
    print(f"{len(out)} cases {by_source}; {len(truths)} verified AlecaFrame reads; {unmatched} Lotus reads with no truth")


if __name__ == "__main__":
    main()
