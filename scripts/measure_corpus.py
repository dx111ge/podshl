#!/usr/bin/env python3
"""Measure the corpus properties that FINDINGS.md cites.

Reproducible via `mise run measure`. Every number in FINDINGS.md comes from
here; nothing in that document is asserted without a line in this file that
produces it.
"""
import re
import sys
from pathlib import Path

import pandas as pd

ROOT = Path(__file__).resolve().parent.parent
TICKETS = ROOT / "data/raw/tobi_bueck_tickets.parquet"
OMQ = ROOT / "data/raw/omq/German_emails.csv"

# "Actionable" = the answer contains something a second agent could execute:
# an imperative, a version number, a path, or a named setting. Deliberately
# generous — it over-counts, which makes the resulting finding conservative.
ACTIONABLE = re.compile(
    r"(?:klicken|öffnen|wählen|setzen|starten|installieren|deinstallieren|prüfen|"
    r"aktualisieren|neu starten|drücken|eingeben|Version \d|\d+\.\d+|"
    r"[A-Z]:\\|/etc/|/var/|Einstellungen|Systemsteuerung)",
    re.IGNORECASE,
)


def pct(n, d):
    return f"{n:,} ({100 * n / d:.1f} %)"


def measure_tickets():
    if not TICKETS.exists():
        sys.exit(f"missing {TICKETS} — run `mise run fetch-corpus` first")
    df = pd.read_parquet(TICKETS)
    print("== Tobi-Bueck/customer-support-tickets ==")
    print(f"rows total          {len(df):,}")
    print(f"languages           {df.language.value_counts().to_dict()}")

    de = df[df.language == "de"].copy()
    n = len(de)
    de["has_answer"] = de.answer.fillna("").str.strip().ne("")
    de["has_type"] = de.type.fillna("").str.strip().ne("")
    print(f"\nde rows             {n:,}")
    print(f"  answer AND type   {pct((de.has_answer & de.has_type).sum(), n)}")
    print(f"  neither           {pct((~de.has_answer & ~de.has_type).sum(), n)}")
    print(f"  type, no answer   {pct((~de.has_answer & de.has_type).sum(), n)}")
    print(f"  answer, no type   {pct((de.has_answer & ~de.has_type).sum(), n)}")

    print(f"\ntype               {de.type.value_counts().to_dict()}")
    print(f"priority           {de.priority.value_counts().to_dict()}")
    print(f"queues             {de.queue.nunique()} distinct")

    # The labelled block is the only part usable for anything supervised.
    lab = de[de.has_answer & de.has_type]
    m = len(lab)
    a = lab.answer
    print(f"\n-- labelled block ({m:,} rows) --")
    print(f"answer chars        mean {a.str.len().mean():.0f}  median {a.str.len().median():.0f}  max {a.str.len().max()}")
    act = a.str.contains(ACTIONABLE).sum()
    print(f"actionable answers  {pct(act, m)}")
    print(f"NOT actionable      {pct(m - act, m)}   <-- the headline number")

    # Canned-reply structure: a real helpdesk shows one boilerplate text
    # hundreds of times. Synthetic data shows near-unique texts.
    vc = a.value_counts()
    print(f"\nunique answers      {pct(a.nunique(), m)}")
    print(f"most repeated text  appears {vc.iloc[0]}x")
    print(f"rows in a repeated  {pct(int(vc[vc > 1].sum()), m)}")


def measure_omq():
    if not OMQ.exists():
        print("\n(omq not present, skipping)")
        return
    df = pd.read_csv(OMQ)
    n = len(df)
    print(f"\n== OMQ German helpdesk emails ==")
    print(f"rows                {n:,}   unique texts {df.text.str.strip().nunique():,}")
    print(f"categories          {df.category.nunique()} (numeric ids, no names)")
    print(f"body chars          mean {df.text.str.len().mean():.0f}  median {df.text.str.len().median():.0f}")
    print(f"relevantText chars  mean {df.relevantText.str.len().mean():.0f}")
    literal = sum(1 for _, r in df.iterrows() if str(r.relevantText).strip() in str(r.text))
    print(f"span is literal     {pct(literal, n)}")
    print(f"resolution texts    0   <-- OMQ has no answers at all")


if __name__ == "__main__":
    measure_tickets()
    measure_omq()
