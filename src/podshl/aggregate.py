"""Cross-client aggregation of vendor responsiveness.

The index is the network effect: computing it needs observations across many
vendors and many users, so no single vendor can produce it. Which is exactly why
it must not become the thing that de-anonymises the people who produce it.

**The set of vendors a client has dealt with is a profile of the software it
runs** — a sharper identifier than any single value in it. So contributions are
submitted *one vendor at a time, unlinkably*: the API accepts a single vendor per
request and carries no client identity, and the client is expected to submit them
separately rather than as a batch. There is nothing to correlate because there is
nothing to correlate *with*.

What travels is a rate rounded to 10 %, plus a coarse weight band — not counts,
because counts are closer to a fingerprint and the rate is the thing wanted.

Ballot stuffing is the real attack and cannot be prevented without identity,
which would cost more than it buys. It is handled by construction instead:
the published figure is a **median**, which a minority of forged contributions
cannot move, publication requires a minimum number of distinct contributors, and
**the contributor count is published alongside the rate** so sparsity is visible
rather than hidden. A vendor that wants to move its own median has to dominate a
contributor count everyone can see.
"""
from __future__ import annotations

from statistics import median

# A client with fewer observations than this has no opinion worth contributing.
MIN_LOCAL_BASIS = 4
# Below this many distinct contributors, no index is published at all.
MIN_CONTRIBUTORS = 5

WEIGHT_BANDS = ((4, "4-9"), (10, "10-24"), (25, "25-99"), (100, "100+"))


def weight_band(n: int) -> str:
    band = WEIGHT_BANDS[0][1]
    for edge, label in WEIGHT_BANDS:
        if n >= edge:
            band = label
    return band


def round_rate(rate: float) -> int:
    """Coarse to 10 % — a precise rate from few observations is identifying."""
    return int(round(rate * 10)) * 10


def contribution(vendor: str, reports: int, acted: int) -> dict | None:
    """One vendor, one submission. Never batch these together."""
    if reports < MIN_LOCAL_BASIS:
        return None
    return {"vendor": vendor, "rate_pct": round_rate(acted / reports),
            "weight": weight_band(reports)}


def publish(rates: list[int]) -> dict:
    """The index as seen from outside, or an honest refusal to state one."""
    if len(rates) < MIN_CONTRIBUTORS:
        return {"published": False, "contributors": len(rates),
                "reason": f"Below {MIN_CONTRIBUTORS} independent contributions no figure is published."}
    return {"published": True, "contributors": len(rates),
            "rate_pct": int(median(rates)),
            "spread_pct": [min(rates), max(rates)]}
