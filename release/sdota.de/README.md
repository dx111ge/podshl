# The operator at sdota.de

`log_key.json` is the **public** half of the signing key of the transparency
log served at <https://sdota.de/log>. Every client release built for this
operator has it compiled in (`PODSHL_BUILD_LOG_KEY`), so that client refuses an
index or a log head this key did not sign.

Check it against what the operator serves before trusting a build:

    curl -s https://sdota.de/log/key

The `x` value there must equal the one here; the `log_id` is the SHA-256 of the
key and is `08be4715854d92b61e4733b3aa338979adc9b34d9c8a3247db50901c40b5a6ad`.
A key that changes without a `log_policy` entry in the log announcing it is a
fork, not a rotation.
