-- The consent constraint did nothing in the one case that mattered.
--
-- A CHECK passes when it evaluates to NULL, not only when it evaluates to true.
-- The original read:
--
--   description IS NULL OR (description_consent ? 'destination' AND ...)
--
-- and with `description_consent` NULL the right-hand side is NULL, so the whole
-- expression was `false OR NULL` = NULL — which Postgres accepts. Free text with
-- no consent at all, which is the case the constraint exists for, went straight
-- in. Three-valued logic turned a guard into a comment.
--
-- Caught by SV22 asserting the stated expectation rather than the behaviour.
-- The lesson generalises: every CHECK guarding a nullable column needs an
-- explicit IS NOT NULL, or it is strongest exactly where the data is weakest.

ALTER TABLE observation DROP CONSTRAINT free_text_carries_its_own_consent;

-- Rows that got in while the guard was decorative hold free text nobody
-- consented to send. There is exactly one defensible thing to do with them, and
-- keeping them is not it: the text is deleted and the observation is kept, so
-- the count a user contributed to survives while the words they never agreed to
-- share do not.
UPDATE observation SET description = NULL, description_consent = NULL
WHERE description IS NOT NULL
  AND (description_consent IS NULL
       OR NOT (description_consent ? 'destination')
       OR NOT (description_consent ? 'granted_at')
       OR NOT COALESCE((description_consent ->> 'granted')::boolean, false));

ALTER TABLE observation ADD CONSTRAINT free_text_carries_its_own_consent CHECK (
    description IS NULL OR (
        description_consent IS NOT NULL
        AND description_consent ? 'destination'
        AND description_consent ? 'granted_at'
        AND COALESCE((description_consent ->> 'granted')::boolean, false)
    )
);
