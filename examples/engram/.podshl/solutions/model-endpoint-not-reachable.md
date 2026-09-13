---
id: model-endpoint-not-reachable
answers:
  problem_class: ollama.endpoint.unreachable
  when:
    engram.symptom: "the chat or debate never answers"
    ollama.host: "OLLAMA_HOST is not set and I am not running Ollama"
severity: medium
proposes:
  - action: report_only
    params: {}
    because: >-
      Starting or installing another vendor's service on somebody's machine is
      well outside what this agent should do, and it is not engram's software
---
Chat, the 47 tools and every debate mode go through an OpenAI-compatible model
endpoint. Storage, search and the graph do not — which is why the rest of
engram looks perfectly healthy while these three do nothing.

There is no endpoint configured here, and no Ollama running.

The recommended local setup:

    ollama pull gemma4:e4b
    ollama serve

Then point engram at it in **System → LLM config** in the web UI on
`http://localhost:3030`. Any OpenAI-compatible endpoint works — Ollama, vLLM,
OpenAI, Azure — so if you already have one, use that instead.

If Ollama *is* running and this still happens, the usual causes are that it is
bound to a different address than engram is asking for, or that the model named
in the config was never pulled. `ollama list` answers the second one.

This is a note about Ollama's behaviour, not a claim about Ollama. engram is
simply the thing you can see when the endpoint is not there.
