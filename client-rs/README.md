# Support Agent — the client

One native binary per platform. Tauri v2 + Rust, no JavaScript build step and no
`node_modules`: the frontend is hand-written HTML/CSS in `ui/`, because a
dependency tree is a supply-chain liability in a product whose entire job is
being trustworthy.

    cargo test                  # the whole client suite — needs `mise run services`,
                                # `mise run db`, `mise run migrate` and `mise run server`
    cargo run                   # the window
    cargo run -- doctor         # what this client can do on this machine
    cargo run -- demo           # the whole argument in five acts
    cargo build --release

The suite reaches the counterparty on loopback and the operator server with its
database, so all four have to be up first; `mise run client-test` is the same
`cargo test` and starts none of them.

Environment, all optional:

| | |
|---|---|
| `VS_TRUST` | The out-of-band key source. `dns`, the default, resolves vendor keys through the system resolver; anything else is a path to a pinned file, which announces itself as a stand-in |
| `VS_ROOT` | Bounds where file actions may write, and where `index_cache.json` goes when set. Unset, `%APPDATA%\podshl` (the config directory's `podshl` elsewhere) |
| `VS_LOG_KEY` | The operator's log public key, for verifying the signed head the index is served under. Unset, the key the build was made with; `../var/log_key.json` in debug builds only |
| `VS_DIRECTORY` | A vendor directory file, in place of the index. `../var/vendor_directory.json` in debug builds only |
| `PODSHL_SERVER_URL` | The operator, the build's `PODSHL_BUILD_SERVER_URL` or `http://127.0.0.1:8725` by default. Not a stored setting: a page that could change it could redirect a report |
| `PODSHL_INDEX_URL` | The index, the build's `PODSHL_BUILD_INDEX_URL` or `http://127.0.0.1:8723` by default |

At **build** time, `PODSHL_BUILD_SERVER_URL`, `PODSHL_BUILD_INDEX_URL` and
`PODSHL_BUILD_LOG_KEY` (the public JWK's text) compile one operator into a
release, because an installed client has nobody to set its environment.
`scripts/build/build_windows_installer.ps1` sets them and builds the per-user NSIS
setup (`L3`, `L9`, `L10`).
| `VS_EXTRA_ROOT` | One more readable root. **Debug builds only** — a release binary does not read it (`EN5`) |

## What is verified here, not asserted

* **JCS (RFC 8785) agrees with the Python vendor side.** `cargo test` verifies a
  card actually signed by the vendor implementation, and rejects a tampered one.
  This is the test that matters: a canonicalisation differing by one byte
  reports "signature invalid" on a perfectly good card, which is the worst
  failure mode available — it looks like an attack. The card is fetched from the
  running vendor when it is not on disk; it used to be read from a file nothing
  generates, and the test returned early when it was missing, so on a fresh
  checkout the interop test reported green having verified nothing.
* **The vocabularies are held to `spec/vocabulary/`**, not to the other
  implementation. They had drifted — one side carried `set_env_var`, this one
  carries `restore_backup` — while the case guarding that boundary asserted only
  an id common to both, and so could not see it.
* **The wire format is typed**, in `wire.rs`, and each struct is checked against
  the schema this project publishes. Unknown fields are ignored rather than
  refused: a vendor extending its card must not break a client that predates the
  extension.
* **Ed25519 with `verify_strict`**, which rejects small-order keys. The
  permissive variant has no business at a trust boundary.
* **The client never signs.** There is no signing code, so it cannot be tricked
  into producing a signature.
* **The action vocabulary lives here.** A vendor selects an id and fills declared
  parameters; it cannot ship a capability. Unknown ids are refused with the
  client's own vocabulary named.
* **Out-of-band trust only.** A card is never verified against a key it carries.

## Consent details that are deliberate

* **The refusing button takes focus.** A stray Return must never grant consent.
* **Dry-run output is shown before the decision**, and must be truthful — a
  dry-run that under-reports its own effect defeats the whole design.
* Mutating actions keep a `.bak` rollback copy.

## Platform reality

| | |
|---|---|
| **Linux** | WebKitGTK is a genuine packaging dependency (143 shared libs). **On Wayland the app exits with `Gdk-Message: Error 71 (Protocol error)` and creates no window at all unless `WEBKIT_DISABLE_DMABUF_RENDERER=1` is set** — measured on Omarchy 4.0.2 / NVIDIA 610.57.04, so not a legacy-driver case. The binary now sets it for its own process before the window is created and honours a value the person set, because a desktop entry is `Terminal=false` and that error reaches nobody: the icon simply does nothing. Not a launcher — a launcher would miss the bare binary, which `INSTALL.md` offers. |
| **Windows** | WebView2 ships with Windows 10+/11. The elevated helper must be a **separate binary**: an app able to elevate itself in-process cannot honestly claim bounded effect. |
| **macOS** | WKWebView is part of the OS. TCC can refuse a probe *after* the user consented in our own UI, and that divergence has to reach the user rather than looking like a failed read. |

**Release builds cannot be cross-compiled from Linux.** Windows and macOS
artefacts need CI runners on those operating systems. That is infrastructure to
plan for, not a detail.

## Translations

One file per language in `ui/i18n/`, as JSON, so a translation is one pull
request and one review. `cargo test` fails if any language is missing a key, or
loses a `{placeholder}` English declares — a missing string falls back to
English silently and looks like a design choice rather than a gap.

They were `.js` files assigning into a global, which meant reading them the way
the client reads them needed a JavaScript engine, so the check shelled out to
`node`. An earlier version of that check matched keys with a regular expression
and saw only the first key on each line: a quarter of the table was invisible to
it, and a checker with blind spots reports clean exactly where it cannot look.

**The binary's own sentences are in the same files.** Everything it says to a
person — an error, the dry run, the reason a reading was refused — is an `m_*`
entry in `en.json`, read at compile time through `m!("code", …)`
(`src/msg.rs`); the source holds codes and no language of its own. The window
recognises a message by its English template and says it again in the chosen
language. `cargo test` fails if a code has no sentence, a sentence has no code,
or a sentence could not be recognised again.

German and English were written for this product. **French and Spanish were not
reviewed by native speakers**, and this text includes consent dialogues: a
subtly wrong translation is where somebody agrees to something they did not
mean. Corrections and further languages are the most useful contribution
anyone can make here.
