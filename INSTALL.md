# Installing the client

The client is the part that runs on **your** machine. It reads what you allow it
to read, shows you a fix before applying it, and can undo what it did.

Nothing about your machine is uploaded until you have seen it and agreed. Every
reading is shown to you first; when the answer has to come from somewhere else
— the project's operator, a vendor's own endpoint, or the model you chose — the
client names the recipient, shows the values, and sends only after you say so.

## What is actually built today

Being straight about this, because the alternative is you discovering it after
downloading:

| Platform | State |
|---|---|
| **Linux x86_64** | Built and tested. A `.deb` and a plain binary |
| **Windows** | **Not built as a package.** The client builds and runs from source there — a Rust toolchain and the WebView2 runtime, which Windows 10 and 11 already carry — its whole suite passes, and the published, vendor and model paths have been walked end to end through the real window. There is no installer, and the elevated helper does not exist |
| **macOS** | **Not built.** Planned via GitHub-hosted runners, Apple Silicon only |

Tauri release builds do not cross-compile, so Windows and macOS need machines
running those systems. macOS will be built on GitHub-hosted runners, arm64 only
— an Intel artefact nobody has run would be coverage in name only. On Windows there is a second,
deliberate gap: the elevated helper must be a separate binary, because an
application able to elevate itself in-process cannot honestly claim bounded
effect. It is not written. See [RELEASING.md](RELEASING.md).

## Linux

From `var/release/<version>/` after a build, or from a release artefact:

```bash
sudo apt install ./PODSHL_0.1.0_amd64.deb
podshl-client
```

The binary is `podshl-client`; the package is named `podshl`. It declares what
it needs rather than bundling it:

    libwebkit2gtk-4.1-0, libgtk-3-0

That is on purpose. Hiding 140-odd system libraries inside a hundred-megabyte
download would cost the one property that makes a binary of under 7 MB
auditable.

Or run the binary directly, with no install:

```bash
chmod +x podshl-client-0.1.0-linux-x86_64
./podshl-client-0.1.0-linux-x86_64
```

**On NVIDIA + Wayland** set `WEBKIT_DISABLE_DMABUF_RENDERER=1` in the
environment before starting it, or the window fails with a Wayland protocol
error. That is a common desktop rather than an exotic one, and the packaged
launcher does not set it for you yet — whichever way you start the client, you
set it.

**The artefacts are not signed.** A signature says who built it, and there is no
key-management story here yet worth signing with. Run `sha256sum -c SHA256SUMS`
in the directory you downloaded into — that tells you the files arrived intact,
not who made them.

### What it needs on the machine

The client asks the operating system's credential store for an API key, so on
Linux that is a Secret Service provider — GNOME Keyring or KWallet — and
without one the model settings cannot store a key. Readings that name a program
need that program: `docker` for anything about a running container,
`nvidia-smi` for the graphics card, `lspci` for the PCI list. `doctor` says
which of these are missing here and what that leaves unreadable.

### There is no public operator yet

The binary talks to `127.0.0.1:8725` for the operator and `127.0.0.1:8723` for
the index unless `PODSHL_SERVER_URL` and `PODSHL_INDEX_URL` say otherwise — the
development stack from the repository, not a service on the internet. Vendor
keys are looked up in DNS by default (`VS_TRUST`), and verifying the index's
signed log head needs the operator's public key in `VS_LOG_KEY`. Until an
operator is running somewhere public, an installed client on its own finds
nobody to ask, and says so rather than guessing.

## First, ask what it can do here

```
$ podshl-client doctor

PODSHL — what this client can do on this machine

  Platform      linux / x86_64
  Readable      9 values
                · gpu.name
                · os.version
                ...
  Not readable  5 — and why:
                · pci.devices  lspci is not available on this device — this reading cannot be taken here.
  Actions       3
                · report_only
                · restore_backup  (changes things)
                · set_config_key  (changes things)

  Limits        at most 24 readings per diagnosis
```

This is the honest inventory: what it can read on *this* machine, what it
cannot and why, and every action it is capable of. Nothing outside that list can
be asked of it by anyone.

`podshl-client demo` walks the whole argument in five acts against live services.

## Do I need to set up an AI model?

**No.** For any project that has published support files, everything is matched
against rules on the operator's endpoint, using the readings you agreed to send.
No model is involved.

A model is used in exactly one case: **nobody has published anything about your
problem**, and there is then no vendor to own what a model would say — so it
falls to one you chose. If you want that fallback, set it up under the chip in
the top right:

| | |
|---|---|
| **Free cloud tier** | GitHub Models, Google AI Studio, Cerebras, Groq, Mistral, OpenRouter's `:free` models. A free key, no card |
| **Local** | Ollama, LM Studio, llama.cpp. The only option where the question genuinely stays on your machine |
| **Paid cloud** | Anthropic, OpenAI, DeepSeek, Together |

**A cloud model means your question and your readings do leave the device** —
to the provider you picked, not to us. The settings screen says so rather than
letting "your own model" imply "stays local". Your API key goes into the
operating system's credential store, never into a config file, and it is only
ever sent to the host its provider preset names. The provider's address and
model name are kept in `llm.json` under your config directory, in plain text.

## What leaves your machine, and when

Nothing, unless you press a button that says so.

* **Readings are collected on your machine** and shown to you. To get an answer
  they then travel to whoever holds the rules — and you are asked first, with
  the recipient named and the values on the screen. For a project that
  published files that is the operator: it uses the readings, as read, to walk
  the project's rules, and does not keep them. For a vendor with its own agent
  it is the vendor's endpoint. Where nobody published anything, and only then,
  they go to the model you configured — which for a cloud model means the
  provider you picked.
* **Every read is a separate consent**, in plain language, with the evidence one
  disclosure behind. Read, transmit and change are three different questions.
  The **refusing** button holds focus, so a stray Return can never grant.
* **What you withhold becomes a question**, not a dead end.
* **Every change is dry-run first** and can be rolled back. The client can only
  perform operations it already implements — a project names one and fills in
  declared parameters; it cannot ship new capability.
* **A program may be asked its version** — only if the project asked, only after
  you agreed to that reading, and only the version number is kept. If the program
  is not on your search path you are asked where it is; your disk is never
  searched. System programs, shells and launchers are never started for this.
* **Reporting is optional and comes last**, after you have your answer and have
  said whether it worked. If you report, what travels is: the project's domain;
  the readings you consented to, coarsened first — a version reduced to
  major.minor, a memory size to a bucket, a serial never; the answers you gave
  from a list, and the name of a question you were asked but not the text you
  typed; whether it worked; which facts the answer turned on; the names of the
  readings you withheld; any action that failed; what resolved it and the
  version of the solution; the size class of your model, where one was used at
  all; a pseudonym; the month. There is no timestamp finer than the month, and
  the consent record for free text carries the month too. Every string in it
  passes the anonymiser before it leaves, on every path. Free text, including
  the lines of a log, travels only if you agree to that separately: you see the
  exact words first, with names, addresses, tokens and times already replaced,
  and you can edit every one of them.

### The pseudonym

Reports carry a pseudonym so a recipient can count people rather than posts, and
rate-limit abuse. It is **different for every project** and **changes every
month**, derived from 32 bytes of local randomness that is never a hardware
fingerprint. Across projects nothing links; across months nothing links. You can
reset it at any time — *Reset reporting identity*, in the settings — that forfeits
accumulated standing with every vendor, which is reset along with it, and
restores unlinkability. It is your call, not ours.

Nothing is shown to a maintainer until **five distinct people** have reported the
same thing. Below that a constellation is identifying. Five is the operator's
setting (`PODSHL_K`), not something a client or a maintainer can lower.

## Where it keeps things

Under your config directory — `~/.config/podshl/` on Linux, `%APPDATA%\podshl`
on Windows:

| | |
|---|---|
| `client_secret` | The 32 bytes the pseudonym is derived from. Deleting it is the reset |
| `llm.json` | Which model provider and model you chose, in plain text. The key itself is in the credential store |
| `index_cache.json` | The last index fetched, so the next start can ask whether it changed rather than fetch it again |
| `vendor_standing.json` | Your standing with each vendor, per pseudonym, reset with it |

A change the client applies leaves a `.bak` beside the file it changed, and it
never overwrites one that is already there.

## Uninstalling

```bash
sudo apt remove podshl
```

That removes the program and leaves your data where it was: the files above,
the API key in the credential store, and any `.bak` beside a file the client
changed. Remove those yourself to leave nothing behind.
