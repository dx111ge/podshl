//! Client identity for reporting — accountable without being trackable.
//!
//! Unauthenticated reports are farmable, so a recipient needs *some* way to
//! rate-limit and to block a client that behaves badly. But a stable client id
//! is a tracking identifier: it lets a vendor correlate every report a machine
//! ever sent, and that destroys the anonymity the report channel promises.
//!
//! The two requirements separate cleanly, because **"one client, one vote" is
//! not the same question as "which client"**.
//!
//! What is implemented here is a **per-vendor, per-epoch pseudonym**:
//! `HMAC(secret, vendor_domain ‖ epoch)`. Within a month a vendor can count,
//! rate-limit and block. Across vendors it cannot link, because each gets a
//! different value. Across months it cannot link either, because the epoch
//! rotates. The residual leak is bounded and stated: within one epoch, one
//! vendor can tell that two reports came from the same client.
//!
//! The target is Privacy Pass (RFC 9576): blind-signed tokens, where an issuer
//! knows it gave a client N tokens but a recipient cannot link a redeemed token
//! back to the client. That removes even the residual leak. The pseudonym is
//! the honest intermediate, not the destination.
//!
//! The secret is **locally generated randomness, never a hardware or machine
//! fingerprint**, and the user can reset it. Resetting forfeits accumulated
//! standing and restores unlinkability — that trade belongs to the user.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::path::PathBuf;

type HmacSha256 = Hmac<Sha256>;

/// Where this machine's secret lives — and, under test, where it does not.
///
/// The suite used the real one. `stable_under_concurrent_first_use` deletes the
/// file to reach first use, so every run of `cargo test` on a machine that also
/// runs the client threw away that client's identity: the next report from the
/// same person arrived under a new pseudonym, and counted as a second reporter.
/// Found by reporting twice from one machine through the window, with the suite
/// run in between, and watching the operator count two people. The floor of five
/// distinct reporters is what stops a rare constellation identifying somebody;
/// a suite that mints new people is the same failure as having no floor.
fn path() -> Option<PathBuf> {
    #[cfg(test)]
    {
        return Some(
            std::env::temp_dir()
                .join(format!("podshl-test-identity-{}", std::process::id()))
                .join("client_secret"),
        );
    }
    #[allow(unreachable_code)]
    Some(dirs::config_dir()?.join("podshl").join("client_secret"))
}

/// 32 bytes of local randomness. Nothing about the machine goes into it, so it
/// cannot be reconstructed from hardware and cannot be correlated with anything
/// the user has not chosen to link.
fn secret() -> Result<Vec<u8>, String> {
    let p = path().ok_or_else(|| m!("no_config_dir"))?;
    match std::fs::read(&p) {
        Ok(b) if b.len() == 32 => return Ok(b),
        // A file of the wrong length is not a secret; replace it.
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        // **Not a reason to mint a new one.** A locked file, a denied ACL or a
        // network home directory that is briefly unavailable are transient, and
        // silently regenerating turns a transient error into a permanent change
        // of identity: the same person then counts as a *new* reporter, and the
        // floor of five distinct reporters is what stops a rare constellation
        // identifying somebody. Over-counting one person into five is the same
        // failure as having no floor at all.
        Err(e) => return Err(m!("secret_unreadable", e = e)),
    }

    use rand::RngCore;
    let mut buf = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut buf);
    let dir = p.parent().ok_or_else(|| m!("no_config_dir"))?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    // Write-then-rename, because `fs::write` is not atomic and this function has
    // more than one caller. Two of them arriving together both saw no file, both
    // generated, and the second overwrote the first — so two pseudonyms minted
    // moments apart differed, which is exactly what must never happen. Found by
    // `stable_within_vendor_and_epoch` failing on Windows, where the partially
    // written file is visible and the read fails; the race is the same
    // everywhere and Linux merely hid it.
    // Unique per *attempt*, not per process: keyed on the process id alone, two
    // threads of one process share the temp path and clobber each other. The
    // randomness just generated is already unique, so reuse it.
    let stamp: String = buf[..8].iter().map(|b| format!("{b:02x}")).collect();
    let tmp = dir.join(format!("client_secret.{}.{stamp}.tmp", std::process::id()));
    std::fs::write(&tmp, buf).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Before it is linked into place, so the file is never briefly
        // world-readable under its real name.
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
    }

    // **Link, not rename.** A rename replaces whatever is there, so every one of
    // eight concurrent first-users overwrote the last and each read back a
    // different secret — write-then-rename fixes torn reads and does nothing at
    // all about "only one of us may win". `hard_link` fails when the destination
    // exists, which is exactly create-if-absent, and it publishes a file that was
    // complete before it had a name.
    //
    // Losing is the normal path and not an error: whoever got there first holds
    // the identity of this machine, and ours was never it.
    let linked = std::fs::hard_link(&tmp, &p);
    let _ = std::fs::remove_file(&tmp);
    match linked {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(m!("secret_uncreatable", e = e)),
    }

    match std::fs::read(&p) {
        Ok(b) if b.len() == 32 => Ok(b),
        Ok(_) => Err(m!("secret_wrong_length")),
        Err(e) => Err(m!("secret_unreadable", e = e)),
    }
}

/// Year-month. Coarse enough to be useful for rate limiting, short enough that
/// a vendor's view of a client expires on its own.
pub fn epoch() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (mut y, mut d) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if d < len {
            break;
        }
        d -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 1;
    for len in months {
        if d < len {
            break;
        }
        d -= len;
        m += 1;
    }
    format!("{y:04}-{m:02}")
}

/// A different value for every vendor, rotating every month.
pub fn pseudonym(vendor_domain: &str) -> Result<String, String> {
    let mut mac = HmacSha256::new_from_slice(&secret()?).map_err(|e| e.to_string())?;
    mac.update(vendor_domain.as_bytes());
    mac.update(b"|");
    mac.update(epoch().as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(&mac.finalize().into_bytes()[..16]))
}

/// Forfeits standing, restores unlinkability. The user's call, not ours.
pub fn reset() -> Result<(), String> {
    let p = path().ok_or_else(|| m!("no_config_dir"))?;
    let _ = std::fs::remove_file(&p);
    secret().map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every test here touches the one secret file this machine has, and
    /// `cargo test` runs them on separate threads. Without this they interfere:
    /// the test that deletes the file to reach first use pulls the ground out
    /// from under the one asserting stability, and the failure looks exactly
    /// like the bug rather than like the harness.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn serially() -> std::sync::MutexGuard<'static, ()> {
        ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The whole point: a vendor cannot link this client to what it sent
    /// anyone else.
    #[test]
    fn differs_per_vendor() {
        let _guard = serially();
        let a = pseudonym("nvidia.com").unwrap();
        let b = pseudonym("datev.de").unwrap();
        assert_ne!(a, b, "the same pseudonym went to two vendors");
    }

    /// And it must be stable within a vendor and epoch, or rate limiting and
    /// blocking cannot work at all.
    #[test]
    fn stable_within_vendor_and_epoch() {
        let _guard = serially();
        assert_eq!(
            pseudonym("nvidia.com").unwrap(),
            pseudonym("nvidia.com").unwrap()
        );
    }

    /// And stable under concurrency, which is how it stopped being stable.
    ///
    /// `secret()` read, found nothing, generated and wrote. Two callers arriving
    /// together both took that path and the second overwrote the first, so two
    /// pseudonyms minted moments apart differed. A changed pseudonym is a *new
    /// reporter* to the counter, and the floor of five distinct reporters is
    /// what stops a rare constellation identifying somebody — one person counted
    /// five times crosses it alone.
    #[test]
    fn stable_under_concurrent_first_use() {
        let _guard = serially();
        // Start from nothing, which is the state the race needs.
        if let Some(p) = path() {
            let _ = std::fs::remove_file(&p);
        }
        let hands: Vec<_> = (0..8)
            .map(|_| std::thread::spawn(|| pseudonym("nvidia.com")))
            .collect();
        let got: Vec<String> = hands
            .into_iter()
            .map(|h| h.join().unwrap().unwrap())
            .collect();
        assert!(
            got.windows(2).all(|w| w[0] == w[1]),
            "concurrent first use minted more than one identity for this machine: {got:?}"
        );
    }

    /// The suite never touches the identity of the client installed on the
    /// machine it runs on. It did, and every `cargo test` turned the next real
    /// report into a new reporter.
    #[test]
    fn the_suite_does_not_touch_this_machines_real_identity() {
        let real = dirs::config_dir().map(|d| d.join("podshl").join("client_secret"));
        let used = path().expect("no path");
        assert_ne!(
            Some(used.clone()),
            real,
            "the tests use the installed client's secret"
        );
        assert!(
            used.starts_with(std::env::temp_dir()),
            "the test secret is not in a scratch directory: {}",
            used.display()
        );
    }

    #[test]
    fn epoch_is_a_year_month() {
        let e = epoch();
        assert_eq!(e.len(), 7, "unexpected epoch shape: {e}");
        assert!(e.chars().nth(4) == Some('-'), "unexpected epoch shape: {e}");
    }
}
