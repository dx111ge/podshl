//! Checking the operator's log on this machine, instead of taking its word.
//!
//! A published answer says "recorded in the public log as entry 310, so anybody
//! can check". Until this module that sentence was true of the log and not of
//! the client: nobody on the user's side checked anything. Now the client does
//! the check it tells people is possible — the one a Certificate Transparency
//! client does — and the window says which it was:
//!
//! 1. The **signed tree head** verifies against the log key pinned out of band,
//!    never one the response carries.
//! 2. The **entry** hashes, as RFC 6962 leaf, to the leaf the proof starts from.
//! 3. The **inclusion proof**, for exactly that head's size, folds up to the
//!    head's root.
//! 4. The entry attests **what the mirror is serving**: the same content hash
//!    and commit. An entry that is in the log but about different files proves
//!    nothing about the answer on screen.
//! 5. The head **extends the last head this client accepted**, by a consistency
//!    proof against the size and root it remembered. One session proving a log
//!    self-consistent proves very little — a server showing this machine one
//!    tree and everybody else another is consistent with itself every time it
//!    is asked. What catches that is the second look, and the client is the
//!    only party that can take it on the user's behalf.
//!
//! The arithmetic is `src/podshl/server/merkle.py`'s, which the published
//! monitor vendors; this is the same verifier in the client's language.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{index, jcs, jws};

fn leaf_hash(data: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([0u8]);
    h.update(data);
    h.finalize().into()
}

fn node_hash(left: &[u8], right: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update([1u8]);
    h.update(left);
    h.update(right);
    h.finalize().into()
}

/// RFC 6962 inclusion, folded from the leaf upward. `fn_`/`sn` are the node's
/// index and the last index at the current level; the `fn_ == sn` case is the
/// ragged right edge, where the sibling supplied is on the left — which is
/// exactly where a fresh append lands, so getting it wrong fails the newest
/// entries first.
pub fn verify_inclusion(index: u64, size: u64, leaf: &[u8; 32], path: &[[u8; 32]], root: &[u8; 32]) -> bool {
    if index >= size {
        return false;
    }
    let (mut fn_, mut sn) = (index, size - 1);
    let mut acc = *leaf;
    for sibling in path {
        if sn == 0 {
            return false;
        }
        if fn_ & 1 == 1 || fn_ == sn {
            acc = node_hash(sibling, &acc);
            while fn_ != 0 && fn_ & 1 == 0 {
                fn_ >>= 1;
                sn >>= 1;
            }
        } else {
            acc = node_hash(&acc, sibling);
        }
        fn_ >>= 1;
        sn >>= 1;
    }
    sn == 0 && &acc == root
}

fn hex32(s: &str) -> Result<[u8; 32], String> {
    if s.len() != 64 || !s.is_ascii() {
        return Err(m!("not_sha256_hex", v = format!("{s:?}")));
    }
    let mut out = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let pair = std::str::from_utf8(chunk).map_err(|e| e.to_string())?;
        out[i] = u8::from_str_radix(pair, 16).map_err(|e| format!("{pair:?}: {e}"))?;
    }
    Ok(out)
}

async fn get(client: &reqwest::Client, url: String) -> Result<Value, String> {
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("{url}: {e}"))?;
    crate::http::json_capped(resp, crate::http::MAX_BODY)
        .await
        .map_err(|e| format!("{url}: {e}"))
}

/// Everything above, against a running operator. `expected` is what the
/// mirror said it serves — `content_sha256` and `commit` — and the entry must
/// attest exactly that.
pub async fn prove(base: &str, seq: u64, expected: &Value) -> Result<Value, String> {
    let base = base.trim_end_matches('/');
    let client = crate::http::client();

    // 1. The head, against the pinned key.
    let head = get(&client, format!("{base}/log/sth")).await?;
    let (sth, sig) = (&head["sth"], &head["signature"]);
    // Stored heads come back with their signature as the JSON text it was
    // saved as; freshly issued ones as an object. Both are the same signature.
    let sig: Value = match sig {
        Value::String(s) => serde_json::from_str(s).map_err(|e| m!("signature_unreadable", e = e))?,
        other => other.clone(),
    };
    let jwk = index::pinned_key().ok_or_else(|| m!("log_no_pinned_key"))?;
    jws::verify_detached(&jwk, sth, &sig).map_err(|e| m!("sth_signature_invalid", e = e))?;
    let size = sth["tree_size"].as_u64().ok_or_else(|| m!("sth_no_size"))?;
    let root = hex32(sth["root_hash"].as_str().ok_or_else(|| m!("sth_no_root"))?)?;
    if seq >= size {
        return Err(m!("entry_beyond_head", seq = seq, size = size));
    }

    // 1a. And this head extends the last one this client accepted. Before the
    // entry rather than after it: if the log forked, what a single entry
    // proves about a tree nobody else is being shown is nothing at all.
    let chain = hold_against_last_seen(base, sth).await?;

    // 2. The entry, hashed as the log hashed it.
    let page = get(&client, format!("{base}/log/entries?start={seq}&end={}", seq + 1)).await?;
    let entry = page["entries"].get(0).and_then(|e| e.get("entry")).ok_or_else(|| m!("entry_missing"))?;
    if entry["seq"].as_u64() != Some(seq) {
        return Err(m!("entry_wrong_seq", seq = seq, got = entry["seq"]));
    }
    let leaf = leaf_hash(&jcs::canonicalize(entry)?);

    // 3. The proof, for exactly the head we verified.
    let proof = get(&client, format!("{base}/log/proof/inclusion?seq={seq}&size={size}")).await?;
    if proof["tree_size"].as_u64() != Some(size) {
        return Err(m!("proof_wrong_size"));
    }
    let path: Vec<[u8; 32]> = proof["path"]
        .as_array()
        .ok_or_else(|| m!("proof_no_path"))?
        .iter()
        .map(|p| hex32(p.as_str().unwrap_or("")))
        .collect::<Result<_, _>>()?;
    if !verify_inclusion(seq, size, &leaf, &path, &root) {
        return Err(m!("entry_not_in_log", seq = seq, size = size));
    }

    // 4. And it is about the files on screen.
    for field in ["content_sha256", "commit"] {
        if let Some(want) = expected.get(field).filter(|v| !v.is_null()) {
            if entry.get(field) != Some(want) {
                return Err(m!("entry_attests_other", seq = seq, field = field,
                              got = entry.get(field).unwrap_or(&Value::Null), want = want));
            }
        }
    }
    Ok(json!({
        "verified": true, "seq": seq, "tree_size": size,
        "timestamp": sth["timestamp"], "kind": entry["kind"],
        "chain": chain,
    }))
}

/// That the tree of `old_size` is a prefix of the tree of `new_size`.
///
/// The check a naive monitor forgets. An inclusion proof says an entry is in
/// *some* tree; it says nothing about whether that tree is the one you were
/// shown yesterday. Both roots are recomputed from the same path here, so the
/// old one is not taken on trust either — a server that wanted to rewrite
/// history would have to produce a path that folds to a root it already
/// published, which is the thing it cannot do.
///
/// The arithmetic is `merkle.py`'s `verify_consistency`, RFC 6962 section 2.1.2.
pub fn verify_consistency(old_size: u64, new_size: u64, old_root: &[u8; 32],
                          new_root: &[u8; 32], path: &[[u8; 32]]) -> bool {
    if old_size > new_size {
        return false;
    }
    if old_size == new_size {
        return path.is_empty() && old_root == new_root;
    }
    if old_size == 0 {
        return path.is_empty();
    }
    if path.is_empty() {
        return false;
    }

    let (mut fnode, mut snode) = (old_size - 1, new_size - 1);
    // Climb out of the old tree's right spine. Landing at zero means the old
    // tree is a complete subtree, so its root is not carried in the proof.
    while fnode & 1 == 1 {
        fnode >>= 1;
        snode >>= 1;
    }

    let (mut old_acc, mut new_acc, rest) = if fnode == 0 {
        (*old_root, *old_root, &path[..])
    } else {
        (path[0], path[0], &path[1..])
    };

    for sibling in rest {
        if snode == 0 {
            return false;
        }
        if fnode & 1 == 1 || fnode == snode {
            old_acc = node_hash(sibling, &old_acc);
            new_acc = node_hash(sibling, &new_acc);
            while fnode != 0 && fnode & 1 == 0 {
                fnode >>= 1;
                snode >>= 1;
            }
        } else {
            new_acc = node_hash(&new_acc, sibling);
        }
        fnode >>= 1;
        snode >>= 1;
    }

    snode == 0 && old_acc == *old_root && new_acc == *new_root
}

/// The last head this client accepted, kept between sessions.
///
/// One session proving a log self-consistent proves very little: a server that
/// serves one tree to this machine and another to everyone else is consistent
/// with itself every time it is asked. What catches that is the second look —
/// the head seen today has to extend the head seen last week, and a log that
/// cannot show it has either lost entries or is not the same log.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
pub struct Seen {
    pub log_id: String,
    pub tree_size: u64,
    pub root_hash: String,
    /// When this client first accepted it, so the window can say how long the
    /// chain it has checked actually is.
    pub seen_at_ms: u64,
}

fn seen_path() -> std::path::PathBuf {
    if let Ok(root) = std::env::var("VS_ROOT") {
        return std::path::PathBuf::from(root).join("log_seen.json");
    }
    dirs::config_dir()
        .map(|p| p.join("podshl").join("log_seen.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("log_seen.json"))
}

pub fn last_seen() -> Option<Seen> {
    serde_json::from_str(&std::fs::read_to_string(seen_path()).ok()?).ok()
}

fn remember(seen: &Seen) {
    let p = seen_path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(&p, serde_json::to_string(seen).unwrap_or_default());
}

/// Hold a freshly verified head against the last one this client accepted.
///
/// Returns what the window should say about it. Three answers, and the middle
/// one is not a failure: `first` — nothing to compare against yet, this head is
/// now the anchor; `extends` — the log grew and the proof holds; and an error,
/// which is the alarm the whole exercise exists to raise.
///
/// A head is remembered only after it has been proved to extend the remembered
/// one. Remembering first would let a single bad answer overwrite the evidence
/// that would have caught it.
pub async fn hold_against_last_seen(base: &str, sth: &Value) -> Result<Value, String> {
    let (out, keep) = hold_against(base, sth, last_seen()).await?;
    remember(&keep);
    Ok(out)
}

/// The decision, with the remembered head passed in and the new one handed
/// back rather than written. Storage is the caller's, so every branch of this
/// can be exercised without a file on disk deciding what the test sees.
pub async fn hold_against(base: &str, sth: &Value, prev: Option<Seen>)
    -> Result<(Value, Seen), String> {
    let size = sth["tree_size"].as_u64().ok_or_else(|| m!("sth_no_size"))?;
    let root = sth["root_hash"].as_str().ok_or_else(|| m!("sth_no_root"))?.to_string();
    let log_id = sth["log_id"].as_str().unwrap_or("").to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let Some(prev) = prev else {
        return Ok((
            json!({"consistent": true, "first": true, "since": Value::Null,
                   "from_size": Value::Null, "tree_size": size}),
            Seen { log_id, tree_size: size, root_hash: root, seen_at_ms: now },
        ));
    };

    // A different key is a different log, and no amount of Merkle arithmetic
    // relates two of them. Said as its own refusal rather than folded into a
    // failed proof, because the two mean entirely different things: one is a
    // rewrite, the other is a client pointed somewhere else.
    if !prev.log_id.is_empty() && !log_id.is_empty() && prev.log_id != log_id {
        return Err(m!("log_changed_identity", was = prev.log_id, now = log_id));
    }
    if size < prev.tree_size {
        return Err(m!("log_shrank", was = prev.tree_size, now = size));
    }

    let old_root = hex32(&prev.root_hash)?;
    let new_root = hex32(&root)?;
    if size == prev.tree_size {
        if old_root != new_root {
            return Err(m!("log_forked", size = size));
        }
        return Ok((
            json!({"consistent": true, "first": false, "since": prev.seen_at_ms,
                   "from_size": prev.tree_size, "tree_size": size}),
            prev,
        ));
    }

    let proof = get(crate::http::client(),
                    format!("{}/log/proof/consistency?first={}&second={}",
                            base.trim_end_matches('/'), prev.tree_size, size)).await?;
    if proof["first"].as_u64() != Some(prev.tree_size) || proof["second"].as_u64() != Some(size) {
        return Err(m!("proof_wrong_size"));
    }
    let path: Vec<[u8; 32]> = proof["path"]
        .as_array()
        .ok_or_else(|| m!("proof_no_path"))?
        .iter()
        .map(|p| hex32(p.as_str().unwrap_or("")))
        .collect::<Result<_, _>>()?;
    if !verify_consistency(prev.tree_size, size, &old_root, &new_root, &path) {
        return Err(m!("log_not_an_extension", was = prev.tree_size, now = size));
    }

    Ok((
        json!({"consistent": true, "first": false, "since": prev.seen_at_ms,
               "from_size": prev.tree_size, "tree_size": size}),
        Seen { log_id, tree_size: size, root_hash: root, seen_at_ms: prev.seen_at_ms },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The same tree, every prefix of it proved to be a prefix, and nothing
    /// else accepted. Built here rather than fetched, so the arithmetic is
    /// checked before any server gets a say in it.
    #[test]
    fn consistency_holds_for_every_prefix_and_nothing_else() {
        fn root_of(leaves: &[[u8; 32]]) -> [u8; 32] {
            match leaves.len() {
                0 => Sha256::digest(b"").into(),
                1 => leaves[0],
                n => {
                    let mut k = 1;
                    while k * 2 < n {
                        k *= 2;
                    }
                    node_hash(&root_of(&leaves[..k]), &root_of(&leaves[k..]))
                }
            }
        }
        // PROOF(m, D[n]), RFC 6962 2.1.2, written the way the server's
        // `merkle.consistency_path` writes it so the two can disagree.
        fn path_of(leaves: &[[u8; 32]], m: usize) -> Vec<[u8; 32]> {
            fn sub(leaves: &[[u8; 32]], m: usize, is_root: bool) -> Vec<[u8; 32]> {
                let n = leaves.len();
                if m == n {
                    return if is_root { vec![] } else { vec![root_of(leaves)] };
                }
                let mut k = 1;
                while k * 2 < n {
                    k *= 2;
                }
                if m <= k {
                    let mut p = sub(&leaves[..k], m, is_root);
                    p.push(root_of(&leaves[k..]));
                    p
                } else {
                    let mut p = sub(&leaves[k..], m - k, false);
                    p.push(root_of(&leaves[..k]));
                    p
                }
            }
            if m == 0 || m > leaves.len() {
                return vec![];
            }
            sub(leaves, m, true)
        }

        for n in 1usize..=17 {
            let leaves: Vec<[u8; 32]> = (0..n)
                .map(|i| leaf_hash(format!("entry {i}").as_bytes()))
                .collect();
            let new_root = root_of(&leaves);
            for m in 1..=n {
                let old_root = root_of(&leaves[..m]);
                let path = path_of(&leaves, m);
                assert!(
                    verify_consistency(m as u64, n as u64, &old_root, &new_root, &path),
                    "{m} is a prefix of {n} and the proof was refused"
                );
                // The old root is recomputed from the path, so claiming a
                // different one cannot be waved through.
                let mut lie = old_root;
                lie[0] ^= 1;
                assert!(!verify_consistency(m as u64, n as u64, &lie, &new_root, &path),
                        "a forged old root passed at {m}/{n}");
                let mut moved = new_root;
                moved[31] ^= 1;
                assert!(!verify_consistency(m as u64, n as u64, &old_root, &moved, &path),
                        "a forged new root passed at {m}/{n}");
                if !path.is_empty() {
                    let mut bent = path.clone();
                    bent[0][0] ^= 1;
                    assert!(!verify_consistency(m as u64, n as u64, &old_root, &new_root, &bent),
                            "a bent path passed at {m}/{n}");
                    assert!(!verify_consistency(m as u64, n as u64, &old_root, &new_root, &[]),
                            "an empty path passed at {m}/{n}");
                }
            }
            // A tree cannot be a prefix of a smaller one.
            assert!(!verify_consistency(n as u64 + 1, n as u64, &new_root, &new_root, &[]));
        }
    }

    /// LC1: a head is held against the last one this client accepted, and the
    /// three ways that can go wrong are three different sentences.
    ///
    /// No network and no file: the decision is passed what it remembered and
    /// hands back what to remember, so this tests the judgement rather than
    /// the disk.
    #[tokio::test]
    async fn a_head_is_held_against_the_last_one_this_client_accepted() {
        let head = |size: u64, root: &str, id: &str| {
            json!({"tree_size": size, "root_hash": root, "log_id": id})
        };
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        let seen = |size: u64, root: &str, id: &str| Seen {
            log_id: id.into(), tree_size: size, root_hash: root.into(), seen_at_ms: 1_000,
        };

        // Nothing remembered: this head becomes the anchor, and says so.
        let (out, keep) = hold_against("http://127.0.0.1:1", &head(9, &a, "L"), None)
            .await
            .expect("a first head was refused");
        assert_eq!(out["first"], true);
        assert_eq!(out["consistent"], true);
        assert_eq!(keep.tree_size, 9);
        assert_eq!(keep.root_hash, a);

        // The same size and the same root is the same head, seen twice.
        let (out, keep) = hold_against("http://127.0.0.1:1", &head(9, &a, "L"),
                                       Some(seen(9, &a, "L")))
            .await
            .expect("the same head twice was refused");
        assert_eq!(out["first"], false);
        assert_eq!(out["since"], 1_000);
        assert_eq!(keep.tree_size, 9);

        // The same size and a different root is two logs wearing one number.
        let e = hold_against("http://127.0.0.1:1", &head(9, &b, "L"), Some(seen(9, &a, "L")))
            .await
            .expect_err("a fork at the same size was accepted");
        assert!(crate::msg::is("log_forked", &e), "{e}");

        // Fewer entries than before. Nothing legitimate does this.
        let e = hold_against("http://127.0.0.1:1", &head(8, &b, "L"), Some(seen(9, &a, "L")))
            .await
            .expect_err("a shrinking log was accepted");
        assert!(crate::msg::is("log_shrank", &e), "{e}");

        // Another key is another log, and no arithmetic relates two of them —
        // said as its own refusal, because "rewritten" and "you are pointed
        // somewhere else" are not the same news.
        let e = hold_against("http://127.0.0.1:1", &head(12, &b, "OTHER"),
                             Some(seen(9, &a, "L")))
            .await
            .expect_err("a different log was accepted as this one grown");
        assert!(crate::msg::is("log_changed_identity", &e), "{e}");
    }

    /// LC2: the client's consistency verifier against the operator's own
    /// proofs, over HTTP — the halves have to agree, and a fixture would only
    /// prove they agree with a file.
    ///
    /// The old root is rebuilt here from the entries themselves rather than
    /// taken from anywhere, so a server that served a convenient root could
    /// not be the reason this passes.
    #[tokio::test]
    async fn the_servers_own_consistency_proofs_verify_here() {
        fn root_of(leaves: &[[u8; 32]]) -> [u8; 32] {
            match leaves.len() {
                0 => Sha256::digest(b"").into(),
                1 => leaves[0],
                n => {
                    let mut k = 1;
                    while k * 2 < n {
                        k *= 2;
                    }
                    node_hash(&root_of(&leaves[..k]), &root_of(&leaves[k..]))
                }
            }
        }
        let base = "http://127.0.0.1:8725";
        let client = crate::http::client();
        let head = get(client, format!("{base}/log/sth")).await
            .expect("the operator server is not answering on :8725 — this test must never pass without a real log");
        let size = head["sth"]["tree_size"].as_u64().expect("no tree size");
        assert!(size >= 2, "a log of {size} entries cannot show growth");
        let root = hex32(head["sth"]["root_hash"].as_str().unwrap()).unwrap();

        // Paged, because `/log/entries` caps a page and the log outgrows it.
        let mut leaves: Vec<[u8; 32]> = Vec::new();
        while (leaves.len() as u64) < size {
            let from = leaves.len() as u64;
            let page = get(client, format!("{base}/log/entries?start={from}&end={size}"))
                .await
                .unwrap_or_else(|e| panic!("no page from {from}: {e}"));
            let got = page["entries"].as_array().expect("no entries");
            assert!(!got.is_empty(), "the log stopped serving at {from} of {size}");
            leaves.extend(got.iter().map(|e| leaf_hash(&jcs::canonicalize(&e["entry"]).unwrap())));
        }
        assert_eq!(leaves.len() as u64, size, "the log served fewer entries than its head claims");
        assert_eq!(root_of(&leaves), root,
                   "the head's root is not the root of the entries the log serves");

        // Not every prefix: the arithmetic is exhausted offline in the case
        // above, and this one is about the two sides agreeing. The sizes that
        // matter are the small ones and the last few - the ragged right edge
        // is where a consistency proof goes wrong - plus a spread across the
        // middle. Asking for a thousand proofs to learn the same thing would
        // make this the slowest case in the suite and no more conclusive.
        let mut sizes: Vec<u64> = (1..=8.min(size - 1)).collect();
        sizes.extend((1..=4).filter_map(|k| size.checked_sub(k)).filter(|m| *m >= 1));
        sizes.extend((1..8).map(|k| (size * k / 8).max(1)));
        sizes.sort_unstable();
        sizes.dedup();
        sizes.retain(|m| *m < size);
        for m in sizes {
            let old_root = root_of(&leaves[..m as usize]);
            let proof = get(client,
                            format!("{base}/log/proof/consistency?first={m}&second={size}"))
                .await
                .unwrap_or_else(|e| panic!("no proof for {m}/{size}: {e}"));
            let path: Vec<[u8; 32]> = proof["path"].as_array().unwrap().iter()
                .map(|p| hex32(p.as_str().unwrap()).unwrap())
                .collect();
            assert!(verify_consistency(m, size, &old_root, &root, &path),
                    "the operator's own proof that {m} is a prefix of {size} did not verify");
        }
    }

    /// A tree built here, every leaf proved against it by the same arithmetic a
    /// server path would produce — including the ragged right edge, where a
    /// fresh append lands — and any change to leaf, index or path refused.
    #[test]
    fn inclusion_verifies_for_every_leaf_and_nothing_else() {
        fn root_of(leaves: &[[u8; 32]]) -> [u8; 32] {
            match leaves.len() {
                0 => Sha256::digest(b"").into(),
                1 => leaves[0],
                n => {
                    let mut k = 1;
                    while k * 2 < n {
                        k *= 2;
                    }
                    node_hash(&root_of(&leaves[..k]), &root_of(&leaves[k..]))
                }
            }
        }
        fn path_of(leaves: &[[u8; 32]], m: usize) -> Vec<[u8; 32]> {
            let n = leaves.len();
            if n <= 1 {
                return vec![];
            }
            let mut k = 1;
            while k * 2 < n {
                k *= 2;
            }
            if m < k {
                let mut p = path_of(&leaves[..k], m);
                p.push(root_of(&leaves[k..]));
                p
            } else {
                let mut p = path_of(&leaves[k..], m - k);
                p.push(root_of(&leaves[..k]));
                p
            }
        }
        for size in [1usize, 2, 3, 5, 7, 8, 13] {
            let leaves: Vec<[u8; 32]> = (0..size).map(|i| leaf_hash(format!("entry {i}").as_bytes())).collect();
            let root = root_of(&leaves);
            for m in 0..size {
                let path = path_of(&leaves, m);
                assert!(verify_inclusion(m as u64, size as u64, &leaves[m], &path, &root),
                        "leaf {m} of {size} did not verify");
                let other = leaf_hash(b"not this");
                assert!(!verify_inclusion(m as u64, size as u64, &other, &path, &root),
                        "a different leaf verified at {m} of {size}");
                if size > 1 {
                    assert!(!verify_inclusion(((m + 1) % size) as u64, size as u64, &leaves[m], &path, &root),
                            "leaf {m} verified at the wrong index in {size}");
                }
            }
        }
        assert!(!verify_inclusion(3, 3, &leaf_hash(b"x"), &[], &[0; 32]), "an index past the tree verified");
    }

    /// AT2, against the running operator: a real entry proves against its
    /// signed head with the pinned key, and one that attests different files
    /// than the mirror serves is refused even though it is in the log.
    #[tokio::test]
    async fn a_real_entry_proves_against_the_signed_head() {
        // `prove` now also holds the head against the last one this client
        // accepted, and remembers it. The starting state is said out loud
        // rather than inherited: a development database that was rebuilt
        // since the last run is a log that shrank, which is the right answer
        // for a user and the wrong reason for this case to be red. The only
        // test that touches this file.
        let _ = std::fs::remove_file(seen_path());
        let base = "http://127.0.0.1:8725";
        let head = reqwest::get(format!("{base}/log/sth")).await
            .expect("the operator server is not answering on :8725 — this test must never pass without a real log")
            .json::<Value>().await.unwrap();
        let size = head["sth"]["tree_size"].as_u64().unwrap();
        assert!(size > 0, "the log is empty — nothing to prove");
        for seq in [0, size / 2, size - 1] {
            let got = prove(base, seq, &json!({})).await
                .unwrap_or_else(|e| panic!("entry {seq} of {size} did not prove: {e}"));
            assert_eq!(got["verified"], true);
        }
        let e = prove(base, 0, &json!({"content_sha256": "0".repeat(64)})).await.unwrap_err();
        assert!(crate::msg::is("entry_attests_other", &e),
                "an entry about other files was accepted for these: {e}");
        assert!(prove(base, size + 1000, &json!({})).await.is_err(), "an entry past the head proved");
    }
}
