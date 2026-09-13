-- An answer that stands in for "I would rather not say" is not an answer to a
-- value that contradicts it.
--
-- A problem class with one solution, decided only by asking, gets that solution
-- on its root when the tree is derived — so that somebody who declines the
-- question, on a class they named themselves, is not left with nothing. The walk
-- could not tell that answer from one that is *settled* — a solution whose
-- conditions the path has already met — and on "no branch matches" it returned
-- whatever answer it was carrying.
--
-- So engram's `ollama.endpoint.unreachable`, whose one solution requires the
-- choice "OLLAMA_HOST is not set and I am not running Ollama", answered a
-- machine that had *read* OLLAMA_HOST=0.0.0.0 and Ollama 0.33 with "there is no
-- endpoint configured here, and no Ollama running". The reading contradicted the
-- rule, and the rule's own answer came back. Found by walking the published path
-- in the real window, on a machine where Ollama runs.
--
-- The fallback now says what it is. A missing or declined fact still lands on
-- it; a value that matches no branch does not.

ALTER TABLE tree_node ADD COLUMN fallback_only boolean NOT NULL DEFAULT false;

COMMENT ON COLUMN tree_node.fallback_only IS
    'This node''s solution applies only when the fact switched on below it is '
    'missing or declined — never when that fact has a value matching no branch.';
