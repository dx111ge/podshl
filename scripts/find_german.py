"""Lists string literals in source files that look German.

The client's own sentences live in `client-rs/ui/i18n/*.json`, and German is
one of those files like any other language; a German sentence in the source is
a sentence only a German speaker was ever meant to read. This finds them —
by umlaut or by a common German word — so they can be moved to the language
files. Comments are skipped; test fixtures that stand for a German-speaking
user's own input are expected to show up and are judged by eye.

    python scripts/find_german.py client-rs/src src/podshl
"""
from __future__ import annotations

import re
import sys
from pathlib import Path

WORDS = {
    "nicht", "kein", "keine", "keinen", "keiner", "ist", "wird", "werden", "wurde",
    "der", "die", "das", "und", "oder", "für", "mit", "auf", "aus", "eine", "einer",
    "einen", "nach", "noch", "nur", "auch", "bereits", "datei", "antwort", "unbekannt",
    "fehlt", "hersteller", "modell", "signatur", "schlüssel", "unlesbar", "erreichbar",
    "verzeichnis", "pfad", "meldung", "gerät", "eingerichtet", "abgelehnt", "gesperrt",
    "lesen", "wert", "frage", "befund", "sind", "hat", "dem", "den", "des", "zu",
    "sich", "bitte", "dein", "deine", "weiß", "gesamt", "aktuell", "installierte",
    "läuft", "über", "unter", "zurück", "ohne", "wie", "was", "wenn", "dieser", "diese",
}
LITERAL = re.compile(r'"((?:[^"\\\n]|\\.)*)"')
WORD = re.compile(r"[A-Za-zÄÖÜäöüß]+")
SUFFIXES = {".rs", ".py", ".html", ".js", ".mjs"}


def german(text: str) -> bool:
    if re.search(r"[äöüÄÖÜß]", text):
        return True
    words = [w.lower() for w in WORD.findall(text)]
    return sum(w in WORDS for w in words) >= 2


MULTILINE = re.compile(r'"((?:[^"\\]|\\.)*)"', re.S)


def scan(path: Path) -> list[tuple[int, str]]:
    # Comment lines blanked rather than removed, so line numbers stay true and
    # a literal continued over several lines (`"…\` in Rust) is still one.
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    kept = ["" if l.strip().startswith(("//", "#", "*", "/*", "<!--")) else l for l in lines]
    text = "\n".join(kept)
    hits = []
    for m in MULTILINE.finditer(text):
        lit = m.group(1)
        if len(lit) > 600:          # a quote pairing across code, not a string
            continue
        if german(lit):
            hits.append((text.count("\n", 0, m.start()) + 1, " ".join(lit.split())))
    return hits


def main(roots: list[str]) -> int:
    count = 0
    for root in roots:
        base = Path(root)
        files = [base] if base.is_file() else sorted(p for p in base.rglob("*") if p.suffix in SUFFIXES)
        for f in files:
            if "node_modules" in f.parts or "target" in f.parts:
                continue
            for n, lit in scan(f):
                count += 1
                print(f"{f}:{n}: {lit[:110]}")
    print(f"{count} literal(s)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or ["client-rs/src"]))
