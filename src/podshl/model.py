"""Domain model shared by both sides of the wire.

Two ideas carry the safety properties and are worth reading first:

`Probe` — a fact the skill needs. Its `kind` is either `machine` (an agent
reads it) or `human` (the user is asked). Human probes are not a fallback bolted
on: a printed serial is frequently absent from firmware, a board that will not
POST reports nothing at all, and what the user *sees* — flicker pattern, LED
colour, artefacts, noise — has no telemetry equivalent. A human probe may be
gated on `when_missing`, so the user is only asked for what the machine could
not supply.

`ActionCall` — a *selection from a validated vocabulary*, never a script. The
vendor's model chooses an action id and fills declared parameters; it cannot
emit arbitrary code. Hallucination therefore lands inside a space whose every
element has been tested, which is the operational answer to owning the risk of
generation.
"""
from __future__ import annotations

from typing import Any, Literal

from pydantic import BaseModel, Field

ProbeKind = Literal["machine", "human"]
Severity = Literal["info", "low", "medium", "high"]


class Probe(BaseModel):
    id: str
    kind: ProbeKind
    describes: str                      # shown to the user, in their language
    why: str                            # why it is needed — consent needs a reason
    # machine probes
    collector: str | None = None        # reference client: named collector
    # Native client: a parameterised instruction from the bounded read
    # vocabulary. The vendor says *what* to look at; the client decides whether
    # that class of reading is permitted at all, and the user approves the
    # concrete instance before it runs.
    read: dict | None = None
    derived: bool = False               # computed by the client from other readings
    # human probes
    prompt: str | None = None
    example: str | None = None
    pattern: str | None = None          # local validation before anything is sent
    choices: list[str] | None = None
    when_missing: str | None = None     # only ask if this probe id yielded nothing
    required: bool = True


class ActionSpec(BaseModel):
    """A vendor-registered, pre-tested operation. The vocabulary generation is bounded to."""
    id: str
    describes: str
    mutating: bool
    reversible: bool = True
    params: dict[str, str] = Field(default_factory=dict)   # name -> validation regex
    requires_elevation: bool = False


class ActionCall(BaseModel):
    action: str
    params: dict[str, Any] = Field(default_factory=dict)
    because: str                        # evidence sentence shown next to the consent prompt


class SkillDescriptor(BaseModel):
    """What the vendor sends when asked to handle a problem.

    **English is required, every other language is optional.** One extra
    language is a small burden on a vendor and it guarantees every user a
    fallback they can read — without pushing safety-relevant text through a
    machine translation. Where a vendor does serve the user's language, that is
    what the client shows and there is nothing to compare against.
    """
    id: str
    version: str
    title: str
    applies_to: dict[str, str] = Field(default_factory=dict)
    probes: list[Probe]
    # The comparison that makes rot visible rather than asserted.
    static_kb_url: str | None = None
    static_kb_says: str | None = None
    # Which language this descriptor's text is written in.
    lang: str = "en"


class Finding(BaseModel):
    id: str
    severity: Severity
    summary: str
    evidence: list[str] = Field(default_factory=list)
    contradicts_kb: bool = False


class Escalation(BaseModel):
    """Hand-off to a human on the vendor side.

    **The vendor defines the hand-off, not the client.** Where it goes is the
    vendor's routing decision and stays opaque here; what is needed to open the
    case is declared as probes, so "your email address" is a field the vendor
    asks for rather than a column baked into every client; and how the answer
    comes back is chosen from a vocabulary the client bounds — the vendor
    selects a channel, it cannot invent one.

    Abstention without a destination is a dead end; with one it is a routing
    decision. What matters as much as the hand-off is *what travels with it*:
    the incident record means the human starts from machine facts, the user's
    own answers and the failed attempts rather than from "it doesn't work".

    `human_verification` marks the case where a person must decide because
    something is at stake that an unattested claim cannot carry — warranty,
    payout, a regulated answer.
    """
    reason: str
    queue: str
    include: list[str] = Field(default_factory=list)   # record fields to transmit
    human_verification: bool = False
    # Opaque routing target on the vendor's side — their ITSM, their queue.
    # The client passes it through and never interprets it.
    target: str | None = None
    # Extra facts the vendor needs to open the case, as ordinary probes. A
    # contact address is therefore consented and validated like any other value.
    require: list[Probe] = Field(default_factory=list)
    # How the answer comes back. Chosen from the client's bounded set:
    # "ticket_url" | "email" | "none". A vendor that will not reply must say so
    # rather than leave the user waiting.
    reply_via: list[str] = Field(default_factory=lambda: ["none"])


class Remedy(BaseModel):
    """Vendor output: findings plus a bounded plan. Signed before it leaves.

    `nonce` and `facts_sha256` are the request this answers, signed back. A
    signature proved who wrote a remedy and nothing about what for: an answer
    to somebody else's readings, or last month's answer to these, verified
    just as well, and anybody on the wire could replay one. They are optional
    on the model because a remedy read out of a log predates them; the client
    requires both of a remedy it is about to act on.
    """
    skill_id: str
    skill_version: str
    nonce: str | None = None
    facts_sha256: str | None = None
    model_id: str                       # pinned — auditability requires it
    findings: list[Finding]
    plan: list[ActionCall] = Field(default_factory=list)
    verify: list[ActionCall] = Field(default_factory=list)
    abstained: bool = False
    abstain_reason: str | None = None
    escalate: Escalation | None = None
    # The third outcome: not an answer and not a refusal, but "I still need X".
    #
    # This is how a decision tree is walked without guessing. Where two problems
    # look alike, the endpoint asks for the fact that separates them rather than
    # estimating which one this is — a switch is deterministic, a similarity
    # threshold is wrong in both directions at once.
    #
    # The probes come back through exactly the loop the client already runs with
    # the local model, so a vendor endpoint may ask follow-up questions the same
    # way, and the client needs one mechanism rather than two.
    need: list[Probe] = Field(default_factory=list)
    need_reason: str | None = None


class ConsentEvent(BaseModel):
    at: str
    what: str
    detail: dict[str, Any] = Field(default_factory=dict)
    granted: bool


class TraceStep(BaseModel):
    at: str
    kind: Literal["discover", "verify", "probe", "send", "receive", "dry-run", "execute", "check"]
    detail: dict[str, Any] = Field(default_factory=dict)
    ok: bool = True


class IncidentRecord(BaseModel):
    """The trace IS the ticket.

    Not a ticket with a resolution field somebody should have filled in: a
    complete, structured record produced as a by-product of the work — including
    the dead ends, which is the half FINDINGS.md measured as empty in 82.4 % of
    real resolutions.
    """
    incident_id: str
    opened_at: str
    closed_at: str | None = None
    reported: str
    vendor: str
    vendor_verified: bool
    vendor_identity: dict[str, Any] = Field(default_factory=dict)
    skill_id: str | None = None
    skill_version: str | None = None
    model_id: str | None = None
    facts_machine: dict[str, Any] = Field(default_factory=dict)
    facts_human: dict[str, Any] = Field(default_factory=dict)
    transmitted: dict[str, Any] = Field(default_factory=dict)
    findings: list[Finding] = Field(default_factory=list)
    attempted: list[ActionCall] = Field(default_factory=list)
    consents: list[ConsentEvent] = Field(default_factory=list)
    trace: list[TraceStep] = Field(default_factory=list)
    outcome: Literal["resolved", "unresolved", "escalated", "abstained"] | None = None
    verified_by: str | None = None
    escalation_ref: str | None = None
