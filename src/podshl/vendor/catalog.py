"""Vendor-side skills and the bounded generation step.

`Generator` is where the vendor's *pinned* model sits. It is a rule generator
today; a model plugs in at the same interface and is bound by the same contract:
it may only emit `ActionCall`s naming ids from the published vocabulary, and the
client enforces that independently. The pinned `model_id` travels in the remedy
so a finding is always attributable to a specific version.

**What the vendor says lives in `content/<lang>.json`, not here.** This file
holds the structure — which probes, which readings, which rule decides what —
and every sentence a person reads comes from the content file for the language
being served. A skill exists in a language exactly when that file has it: the
precision skill is in German and English, the other two only in German, which
is what `serve()` refuses to hand out under English (`LG3`).
"""
from __future__ import annotations

import json
import os
from functools import lru_cache
from pathlib import Path

from ..model import ActionCall, Escalation, Finding, Probe, Remedy, SkillDescriptor

CONTENT_DIR = Path(__file__).resolve().parent / "content"
#: The vendor's own language: the one every skill is written in first.
HOME = "de"


@lru_cache(maxsize=None)
def content(lang: str) -> dict:
    path = CONTENT_DIR / f"{lang}.json"
    return json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}


def languages() -> list[str]:
    return sorted(p.stem for p in CONTENT_DIR.glob("*.json"))


def said(lang: str, key: str, /, **values) -> str:
    """One of the vendor's own sentences — a receipt, a reply note — in `lang`
    if the vendor has it, otherwise in English."""
    text = content(lang).get("vendor", {}).get(key) or content("en")["vendor"][key]
    return text.format(**values) if values else text


def _app_read(key: str) -> dict:
    """Path stated by the vendor, but the client only permits reads beneath its
    own configuration roots — a path outside them is refused before the user is
    ever asked."""
    cfg = os.environ.get("XDG_CONFIG_HOME") or str(Path.home() / ".config")
    return {"op": "read_file_key", "path": f"{cfg}/accounting-suite/install.json", "key": key}


GENERATOR_ID = "rules-2026.09.07"          # what a pinned model id would be

_NVSMI = lambda q: {"op": "run_tool", "tool": "nvidia-smi", "args": [f"--query-gpu={q}", "--format=csv,noheader"]}

# The structure of each skill: probe id, content key, and what is not text.
# A content key differs from the id only where one probe is asked for twice
# with a different reason — first in the skill, then again in a `need`.
_STRUCTURE: dict[str, dict] = {
    "torch.precision.consumer-gpu": {
        "version": "3.1.0",
        "applies_to": {"product": "toolkit", "component": "gpu"},
        "kb_url": "https://example.invalid/kb/enable-bf16",
        "probes": [
            dict(id="gpu.name", kind="machine", collector="gpu.name", read=_NVSMI("name")),
            dict(id="gpu.compute_capability", kind="machine", collector="gpu.compute_capability",
                 read=_NVSMI("compute_cap")),
            # No reading of its own: the client derives this from the compute
            # capability, so the vendor cannot assert an interpretation of a
            # value it did not read.
            dict(id="gpu.bf16_native", kind="machine", collector="gpu.bf16_native", derived=True),
        ],
    },
    "warranty.rma.precheck": {
        "version": "1.4.2",
        "applies_to": {"component": "gpu"},
        "kb_url": "https://example.invalid/kb/rma",
        "probes": [
            dict(id="gpu.name", kind="machine", collector="gpu.name", read=_NVSMI("name")),
            dict(id="gpu.serial", kind="machine", collector="gpu.serial", read=_NVSMI("serial"),
                 required=False),
            dict(id="gpu.driver_version", kind="machine", collector="gpu.driver_version",
                 read=_NVSMI("driver_version")),
            # Fires only when the firmware did not supply a serial — which is
            # the normal case on consumer cards.
            dict(id="serial.printed", kind="human", when_missing="gpu.serial",
                 example="0324718061234", pattern=r"^\d{10,16}$"),
            dict(id="symptom", kind="human"),
        ],
    },
    # Non-technical, and the strongest case for the architecture: the user base
    # is bound by statutory professional secrecy (§ 203 StGB), so a cloud
    # assistant that must *see* client bookkeeping to help is not inconvenient
    # but unlawful. Local execution with only a question signature leaving the
    # machine is the only compliant shape. Abstention here is a legal duty
    # rather than a courtesy: there is no postcondition to verify on an answer.
    "accounting.booking.guidance": {
        "version": "2026.3.1",
        "applies_to": {"product": "accounting-suite"},
        "kb_url": "https://example.invalid/kb/booking",
        "probes": [
            dict(id="app.version", kind="machine", collector="app.version", read=_app_read("version")),
            dict(id="app.chart_of_accounts", kind="machine", collector="app.chart_of_accounts",
                 read=_app_read("chart_of_accounts"), required=False),
            # The version-tree case: a bounded traversal rather than a single
            # key, and still fully enumerable before it runs.
            dict(id="app.modules", kind="machine", required=False,
                 read={"op": "enumerate_read", "root": "config", "glob": "*/manifest.json",
                       "keys": ["name", "version"]}),
            dict(id="chart.confirmed", kind="human", when_missing="app.chart_of_accounts"),
            dict(id="question", kind="human"),
        ],
    },
}


def _texts(skill_id: str, lang: str) -> dict:
    return content(lang).get("skills", {}).get(skill_id, {})


def _probe(skill_id: str, lang: str, key: str | None = None, **structure) -> Probe:
    """A probe with the words for it in `lang`: what it is, why it is asked,
    and — for a question — how it is put and the answers it offers."""
    t = _texts(skill_id, lang)["probes"][key or structure["id"]]
    return Probe(describes=t["describes"], why=t.get("why", ""), prompt=t.get("prompt"),
                 choices=t.get("choices"), **structure)


def _build(skill_id: str, lang: str) -> SkillDescriptor:
    s, t = _STRUCTURE[skill_id], _texts(skill_id, lang)
    return SkillDescriptor(
        id=skill_id, version=s["version"], title=t["title"], applies_to=s["applies_to"],
        probes=[_probe(skill_id, lang, **p) for p in s["probes"]],
        static_kb_url=s["kb_url"], static_kb_says=t.get("kb"), lang=lang)


CATALOG = {sid: _build(sid, HOME) for sid in _STRUCTURE}
PRECISION, RMA, ADVISORY = (CATALOG[sid] for sid in _STRUCTURE)

# The obligation, met rather than asserted: a skill in another language is the
# same structure with that language's words, and it exists exactly where the
# content file has it.
TRANSLATIONS = {
    sid: {lang: _build(sid, lang) for lang in languages() if lang != HOME and _texts(sid, lang)}
    for sid in _STRUCTURE
}


class MissingEnglish(RuntimeError):
    """A skill without an English variant cannot be served. English is the one
    language a vendor is obliged to provide, and a vendor that has not met it
    should hear so rather than quietly hand out a language the user may not
    read."""


def serve(skill: SkillDescriptor, want: str) -> tuple[SkillDescriptor, str]:
    """Return the best available variant and the language actually served.

    Preference order: the language the client asked for, then English. There is
    no third option — a vendor that cannot produce English has not met the
    protocol.
    """
    variants = TRANSLATIONS.get(skill.id, {})
    if want == skill.lang:
        return skill, skill.lang
    if want in variants:
        return variants[want], want
    if skill.lang == "en":
        return skill, "en"
    if "en" in variants:
        return variants["en"], "en"
    raise MissingEnglish(said(want, "missing_english", id=skill.id, lang=skill.lang))


def triage(problem: str, context: dict) -> SkillDescriptor | None:
    """Which skill a problem is about, by the words a person uses for it — in
    any language the vendor writes in."""
    p = problem.lower()
    for sid in _STRUCTURE:
        words = {w for lang in languages() for w in content(lang).get("triage", {}).get(sid, [])}
        if any(w in p for w in words):
            return CATALOG[sid]
    return None


def generate(skill: SkillDescriptor, facts: dict, lang: str = HOME) -> Remedy:
    """The bounded generation step.

    `lang` is the language the client asked for when it diagnosed. The skill
    was served in it or in English, and the finding follows the same rule — it
    used to come back German on every screen, because `diagnose` carried no
    language. A skill written only in the vendor's own language answers in it;
    it is refused under English at triage (`LG3`) and never gets this far there.
    """
    if lang not in (skill.lang, *TRANSLATIONS.get(skill.id, {})):
        lang = "en" if "en" in TRANSLATIONS.get(skill.id, {}) else skill.lang
    if skill.id == PRECISION.id:
        return _precision(skill, facts, lang)
    if skill.id == RMA.id:
        return _rma(skill, facts, lang)
    if skill.id == ADVISORY.id:
        return _advisory(skill, facts, lang)
    return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                  findings=[], abstained=True, abstain_reason=said(lang, "no_rule"))


def _precision(skill, f: dict, lang: str) -> Remedy:
    text = _texts(skill.id, lang)["text"]
    native = f.get("gpu.bf16_native")
    cc, name = f.get("gpu.compute_capability"), f.get("gpu.name")
    # What the *library* claims is not a machine fact — it is knowledge about
    # our own product. The library reports support for any CUDA device because
    # it counts emulation, so the vendor derives it rather than asking for it.
    reported = native is not None
    if native is None:
        return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                      findings=[], abstained=True,
                      abstain_reason=text["unreadable"],
                      escalate=Escalation(reason=text["no_facts"], queue="toolkit-l2",
                                          include=["facts_machine", "facts_human", "trace"]))
    if reported and not native:
        return Remedy(
            skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
            findings=[Finding(
                id="bf16-emulated", severity="high", contradicts_kb=True,
                summary=text["emulated"].format(name=name, cc=cc),
                evidence=[text["evidence_cc"].format(cc=cc),
                          f"torch.bf16_reported = {reported}",
                          f"torch.bf16_native = {native}"],
            )],
            plan=[ActionCall(action="set_config_key",
                             params={"file": "training.toml", "key": "precision", "value": "fp16"},
                             because=text["because_fp16"])],
            verify=[ActionCall(action="report_only", params={},
                               because=text["because_verify"])],
        )
    return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                  findings=[Finding(id="bf16-native", severity="info",
                                    summary=text["native"],
                                    evidence=[f"torch.bf16_native = {native}"])])


_EMAIL = r"^[^@\s]+@[^@\s]+\.[A-Za-z]{2,}$"


def _rma(skill, f: dict, lang: str) -> Remedy:
    text = _texts(skill.id, lang)["text"]
    serial = f.get("gpu.serial") or f.get("serial.printed")
    # `<probe id>.declined`, and the probe is `serial.printed`. This read
    # `serial.declined`, which nothing ever sends — so the "ask once more"
    # branch was taken every round and the endpoint asked for the serial
    # forever, which `SPEC.md` forbids in the same breath as it defines the
    # convention. Invisible until a card with no firmware serial walked the
    # path: with a serial present the branch was never reached.
    if not serial and not f.get("serial.printed.declined"):
        # A second chance rather than a refusal: the firmware had none and the
        # user skipped the question, so ask once more with the reason stated.
        return Remedy(
            skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
            findings=[], need_reason=text["need_serial"],
            need=[_probe(skill.id, lang, "serial.printed.again", id="serial.printed", kind="human",
                         example="0324718061234", pattern=r"^\d{10,16}$", required=False)])
    if not serial:
        return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                      findings=[], abstained=True,
                      abstain_reason=text["no_serial"],
                      escalate=Escalation(reason=text["serial_missing"], queue="rma-desk",
                                          target="itsm://acme/rma",
                                          include=["facts_machine", "facts_human"],
                                          human_verification=True,
                                          reply_via=["email"],
                                          require=[_probe(skill.id, lang, id="contact.email",
                                                          kind="human", pattern=_EMAIL)]))
    source = text["source_firmware"] if f.get("gpu.serial") else text["source_sticker"]
    return Remedy(
        skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
        findings=[Finding(id="rma-precheck", severity="medium",
                          summary=text["precheck"].format(gpu=f.get("gpu.name"), serial=serial,
                                                          source=source, symptom=f.get("symptom")),
                          evidence=[f"serial = {serial} ({source})",
                                    f"driver = {f.get('gpu.driver_version')}",
                                    f"symptom = {f.get('symptom')}"])],
        plan=[ActionCall(action="report_only", params={}, because=text["because_info"])],
        # Money is attached, so an unattested claim from the customer's own
        # harness cannot settle it. The agent authorises the return; a person
        # decides the payout.
        escalate=Escalation(
            reason=text["needs_human"],
            queue="rma-desk", target="itsm://acme/rma",
            include=["facts_machine", "facts_human", "findings"],
            human_verification=True,
            # Declared by the vendor, not assumed by the client: an RMA needs a
            # way back to the customer and an address to ship to.
            require=[
                _probe(skill.id, lang, "contact.email.rma", id="contact.email", kind="human",
                       pattern=_EMAIL, example="name@example.de"),
                _probe(skill.id, lang, id="contact.country", kind="human"),
            ],
            reply_via=["email", "ticket_url"],
        ),
    )


def _advisory(skill, f: dict, lang: str) -> Remedy:
    text = _texts(skill.id, lang)["text"]
    q = str(f.get("question") or "").strip()
    chart = f.get("app.chart_of_accounts") or f.get("chart.confirmed")
    if not q:
        return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                      findings=[], abstained=True,
                      abstain_reason=text["no_question"],
                      escalate=Escalation(reason=text["question_missing"], queue="application-advice",
                                          include=["facts_machine"]))
    # Questions the vendor will not answer through an agent. Abstention is a
    # legal duty here, not a quality setting: an answer has no postcondition to
    # verify and the liability for wrong tax guidance is not the vendor's to
    # take on casually.
    if any(w in q.lower() for w in text["refuse_words"]):
        return Remedy(
            skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
            findings=[], abstained=True,
            abstain_reason=text["planning"],
            escalate=Escalation(reason=text["planning_escalate"],
                                queue="tax-advice", include=["facts_human"],
                                human_verification=True))
    if not chart:
        # Missing is not the same as unknown: ask for it rather than refuse.
        return Remedy(
            skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
            findings=[], need_reason=text["need_chart"],
            need=[_probe(skill.id, lang, "chart.confirmed.again", id="chart.confirmed", kind="human")])
    if chart == text["dont_know"]:
        # "Don't know" is a valid answer and falls back to the parent — never a
        # dead end, because not everyone knows.
        return Remedy(skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
                      findings=[], abstained=True,
                      abstain_reason=text["chart_unknown"],
                      escalate=Escalation(reason=text["chart_unknown_escalate"],
                                          queue="application-advice",
                                          include=["facts_machine", "facts_human"]))
    version = f.get("app.version")
    if not version:
        # The whole answer asserts that the path was resolved against the
        # *installed* version rather than against the manual, and that is the
        # skill's entire reason to exist. Without the version it was formatting
        # Python's `None` into that sentence and asserting it anyway — a claim
        # about a fact it did not have, which is the one thing an endpoint here
        # may not do. Found by walking the path on a machine with no such
        # program installed, which is every machine that is not a customer's.
        return Remedy(
            skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
            findings=[], abstained=True,
            abstain_reason=text["version_unknown"],
            escalate=Escalation(reason=text["version_unknown_escalate"],
                                queue="application-advice",
                                include=["facts_machine", "facts_human"]))
    return Remedy(
        skill_id=skill.id, skill_version=skill.version, model_id=GENERATOR_ID,
        findings=[Finding(
            id="booking-guidance", severity="info",
            summary=text["answer"].format(chart=chart, version=version),
            evidence=[f"chart_of_accounts = {chart}", f"app.version = {version}",
                      text["evidence_question"].format(q=q[:120])])],
        plan=[ActionCall(action="report_only", params={}, because=text["because_info"])],
    )
