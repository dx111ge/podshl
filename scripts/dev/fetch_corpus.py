#!/usr/bin/env python3
"""Fetch the ticket corpus into data/raw/ (gitignored; ~15 MB).

Kept as a script rather than a doc instruction so `mise run measure` is
reproducible on a clean checkout.
"""
from pathlib import Path

from datasets import load_dataset

OUT = Path(__file__).resolve().parents[2] / "data/raw/tobi_bueck_tickets.parquet"

if OUT.exists():
    print(f"{OUT} already present — nothing to do")
else:
    ds = load_dataset("Tobi-Bueck/customer-support-tickets", split="train")
    OUT.parent.mkdir(parents=True, exist_ok=True)
    ds.to_parquet(OUT)
    print(f"wrote {OUT} ({len(ds):,} rows)")
