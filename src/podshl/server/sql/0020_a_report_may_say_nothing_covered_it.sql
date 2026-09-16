-- A fourth outcome: nothing the project published covered this at all.
--
-- The three that existed describe what happened to an *answer*: it resolved the
-- problem, it did not, or the walk abstained. All three presuppose that the
-- project had something to say. The case with no answer at all had no label, so
-- it could not be reported -- and a maintainer's dashboard is at its most useful
-- exactly there: `resolved 4 of 6` is a number about an answer that exists,
-- while "eleven people reached nothing" is the thing that tells you to write a
-- new one.
--
-- **It was reachable before this and by accident.** A published path that
-- produced no finding fell through to the local model, and the model path ended
-- with a report of its own -- so the operator heard, labelled as a no-vendor
-- report about a project that plainly had a vendor. Taking that fall-through
-- away on 2026-09-16 removed the only route, which is how the gap was noticed:
-- somebody walked the real window on Omarchy, reached "nothing matched", and
-- the maintainer would never have learnt it.
--
-- Two different things arrive under this one label, and that is deliberate:
--
--   * the rules ran over the readings and produced no statement
--   * the person was shown the published problems and said none of them is
--     what they are seeing
--
-- The second is the stronger signal -- it is a judgement by somebody looking at
-- their own machine, not a gap between rules -- but splitting them would ask a
-- maintainer to read two columns to answer one question, which is whether to
-- write something new.
--
-- The two are still told apart by what they carry, and no column was needed for
-- it: rules that matched nothing were reached *after* the readings, so that
-- report holds them -- exactly the constellation the rules failed on. A person
-- saying none of these fit says so at the class picker, before anything is
-- read, so that report holds no readings at all. An empty cluster under this
-- outcome is a human judgement; a full one is a gap between rules.

ALTER TABLE observation DROP CONSTRAINT IF EXISTS observation_outcome_check;

ALTER TABLE observation ADD CONSTRAINT observation_outcome_check
    CHECK (outcome IN ('resolved', 'unresolved', 'escalated', 'abstained', 'uncovered'));

COMMENT ON COLUMN observation.outcome IS
    'What became of the diagnosis. `resolved` and `unresolved` are about an '
    'answer the project published: it worked, or it did not. `escalated` went to '
    'a person, `abstained` is a walk that declined to state anything, and '
    '`uncovered` is the case the project has not written down at all -- either '
    'its rules produced nothing, or the person said none of its published '
    'problems is the one in front of them.';
