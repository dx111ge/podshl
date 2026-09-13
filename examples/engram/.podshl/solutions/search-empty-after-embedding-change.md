---
id: search-empty-after-embedding-change
answers:
  problem_class: engram.search.empty-after-embedding-change
  when:
    engram.symptom: "search returns nothing, and it used to work"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: >-
      Re-embedding rewrites every vector in the brain and can take a long time
      on a large one. That is a decision for whoever owns the data, made
      knowingly, not something to run behind a dialog
---
Semantic search returning nothing on a brain that used to answer is almost
always one thing: the embedding model changed, and the vectors already in the
`.brain` file were written by the old one.

They are not corrupt. They are in a different space, so nothing is near
anything, and the search returns an empty result rather than an error — which
is why this reads as data loss and is not.

    engram reindex my.brain

That re-embeds every node with the model now configured. It runs for a while on
a large brain, and it is safe to run again if it is interrupted.

Two things worth checking first:

* **Full-text search is a good test.** BM25 does not use vectors. If a keyword
  search still finds the node and a semantic one does not, this is definitely
  the cause.
* **Back up before you start.** One file, so it is one copy: `cp my.brain
  my.brain.bak`. Reindex rewrites in place.

If search is empty for keyword queries too, this is not it — that is an
ingestion problem rather than an embedding one, and worth an issue.
