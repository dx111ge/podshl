---
id: ingest-slow-without-gpu
answers:
  problem_class: engram.ingest.ner-slow-without-gpu
  when:
    engram.symptom: "document ingest is extremely slow"
severity: low
proposes:
  - action: report_only
    params: {}
    because: >-
      Nothing here is broken, so there is nothing to change. What is missing is
      a sentence in the documentation, and this is that sentence
---
Ingest is slow because entity extraction is running on the CPU.

The NER stage is GLiNER2 through ONNX. With an NVIDIA GPU it is
GPU-accelerated; without one it falls back to the CPU and keeps working — which
is the right behaviour and a bad experience, because nothing tells you it
happened. A document that takes seconds on a GPU machine takes minutes here.

If the readings above show no GPU, this is expected and not a bug.

What helps, in the order worth trying:

* **Ingest in batches and leave it running.** The cost is per document and it
  is one-off — querying a built brain does not touch NER at all.
* **Build the brain on a machine with a GPU and copy the file.** One `.brain`
  file is the whole knowledge base, so this is a copy, not a migration.
* **Cut the documents down before ingesting.** PDF and HTML extraction feed
  everything to NER; a table of contents and a bibliography cost the same as
  the chapter you wanted.

If you do have an NVIDIA GPU and ingest is still slow, that is worth an issue,
and the GPU name and driver version in this report are the first thing to look
at.
