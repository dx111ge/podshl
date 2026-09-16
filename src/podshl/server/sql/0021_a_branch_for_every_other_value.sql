-- A tree may branch on "everything the author did not name".
--
-- A solution that says nothing about a switch applies under every value of it.
-- Until now it could only live inside the branches some *other* solution
-- happened to create, so a reading outside those fell off the tree and took
-- that solution with it -- an answer published, mirrored, signed, and
-- unreachable.
--
-- engram found it on 2026-09-16. `wrong-archive-for-this-system` constrains the
-- operating system and which archive was downloaded and says nothing about the
-- processor architecture. Another rule named `os.arch: aarch64`, so `os.arch`
-- became the switch and grew exactly one child -- and on an ordinary x86_64
-- Linux desktop nothing matched, the walk stopped at the answer above it, and
-- somebody holding the Windows archive was told to go and find out which
-- archive they had.
--
-- `any` matches every value, including one the comparator cannot read, which is
-- sound precisely because the solutions under it do not mention the fact at
-- all. It is always the last child of its node and never an `eq`, so a named
-- value is always preferred; `cluster_tree.validate_tree` refuses a tree where
-- it is not.

ALTER TABLE tree_node DROP CONSTRAINT IF EXISTS tree_node_match_op_check;

ALTER TABLE tree_node ADD CONSTRAINT tree_node_match_op_check
    CHECK (match_op IN ('eq', 'lt', 'le', 'gt', 'ge', 'in', 'any'));

COMMENT ON COLUMN tree_node.match_op IS
    'The predicate on the parent''s switch that selects this node. One closed '
    'set of total orders and equalities, plus `any` -- the branch for every '
    'value the author did not name, which carries the solutions that say '
    'nothing about this switch and is always the last child.';
