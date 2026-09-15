//! Where a log excerpt comes from, before the user cuts it down.
//!
//! A publisher may attach a log source to a free-text question — "the lines
//! Ollama wrote when the chat failed" — and the client can then fill the box
//! instead of asking somebody to find a log file they have never opened. What
//! is loaded here is **shown and nothing else**: it goes into an editable box
//! on this machine, the user keeps the part that matters, and only that part,
//! anonymised and shown again, can travel — under its own consent.
//!
//! Two sources, and neither one is a search:
//!
//! * **A container**, named by the publisher as an image. Docker is asked
//!   which containers run it and for the tail of one's output. Nothing is
//!   executed inside it.
//! * **A file the user points at.** The publisher may say what it is usually
//!   called; where it is on this machine is the user's answer, never a scan.

use serde_json::{json, Value};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::reads;

/// What is loaded for the user to cut from. Generous compared to what may
/// travel (`redact::MAX_LINES`), because the point is to find the failure in
/// it, and the failure is rarely in the last dozen lines.
pub const LOAD_LINES: usize = 400;
const LOAD_BYTES: u64 = 256 * 1024;

/// The last lines of a running container's output.
///
/// `docker logs` writes the container's stdout and stderr to two pipes, and
/// reading one after the other would put every error after every normal line.
/// `--timestamps` is asked for so the two can be put back in the order they
/// happened; the timestamps themselves are then removed, and would be removed
/// again by the anonymiser if they were not.
pub fn from_container(image: &str) -> Result<Value, String> {
    let re = regex::Regex::new(&format!("^(?:{})$", reads::IMAGE_NAME)).map_err(|e| e.to_string())?;
    if image.len() > 128 || !re.is_match(image) {
        return Err(m!("image_not_allowed", image = format!("{image:?}")));
    }
    let docker = reads::find_on_path("docker").ok_or_else(|| m!("tool_absent", tool = "docker"))?;
    let (id, reference) = reads::containers_of(image)
        .into_iter()
        .next()
        .ok_or_else(|| m!("no_container_of", image = image))?;
    let tail = LOAD_LINES.to_string();
    let (_, text) = reads::run_bounded(&docker, &["logs", "--timestamps", "--tail", &tail, &id])
        .ok_or_else(|| m!("docker_logs_silent"))?;
    let mut lines: Vec<(String, String)> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match l.split_once(' ') {
            Some((ts, rest)) if ts.len() >= 20 && ts.as_bytes()[4] == b'-' => (ts.to_string(), rest.to_string()),
            _ => (String::new(), l.to_string()),
        })
        .collect();
    // RFC 3339 with a fixed width sorts as text; lines without one keep their
    // place relative to each other.
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let start = lines.len().saturating_sub(LOAD_LINES);
    let body: Vec<String> = lines[start..].iter().map(|(_, l)| l.clone()).collect();
    Ok(json!({
        "text": body.join("\n"),
        "lines": body.len(),
        "from": format!("docker logs {} ({reference})", &id[..12.min(id.len())]),
    }))
}

/// The end of a file the user named.
///
/// The deny list applies: consent does not unlock it for reads, and it does not
/// unlock it here either — "paste your `.env` into the box" is not a request
/// this client will help anybody make.
pub fn from_file(path: &str) -> Result<Value, String> {
    let p = Path::new(path.trim().trim_matches('"'));
    if let Some(d) = reads::deny_hit(&p.to_string_lossy()) {
        return Err(m!("denied_even_with_consent", d = d));
    }
    let meta = std::fs::metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !meta.is_file() {
        return Err(m!("not_a_file", p = p.display()));
    }
    let mut f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let len = meta.len();
    if len > LOAD_BYTES {
        f.seek(SeekFrom::Start(len - LOAD_BYTES)).map_err(|e| e.to_string())?;
    }
    let mut buf = Vec::new();
    f.take(LOAD_BYTES).read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf).replace("\r\n", "\n");
    let lines: Vec<&str> = text.lines().collect();
    // A seek into the middle of the file lands mid-line; that first fragment
    // is not a line anybody wrote.
    let from = if len > LOAD_BYTES { 1 } else { 0 };
    let start = lines.len().saturating_sub(LOAD_LINES).max(from.min(lines.len()));
    let body = lines[start..].join("\n");
    Ok(json!({ "text": body, "lines": lines.len() - start, "from": p.display().to_string() }))
}

/// Why a log could not be loaded, as a word the window can translate — by
/// which message it is, not by the words in it.
pub fn error_kind(e: &str) -> &'static str {
    use crate::msg::is;
    if is("tool_absent", e) {
        "no_docker"
    } else if is("no_container_of", e) {
        "no_container"
    } else if is("denied_even_with_consent", e) || is("denied", e) {
        "denied"
    } else if is("not_a_file", e) {
        "not_file"
    } else if is("image_not_allowed", e) || is("log_source_unknown", e) {
        "invalid"
    } else {
        "unreadable"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// LX1: a file the user names is loaded from its end, bounded, and the
    /// deny list still applies to it.
    #[test]
    fn a_named_file_is_read_from_its_end_and_the_deny_list_holds() {
        let dir = std::env::temp_dir().join(format!("podshl-excerpt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("server.log");
        let body: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&log, body).unwrap();

        let v = from_file(&log.to_string_lossy()).expect("a plain log was refused");
        let text = v["text"].as_str().unwrap();
        assert_eq!(v["lines"], LOAD_LINES);
        assert!(text.ends_with("line 999"), "the end of the log is where the failure is");
        assert!(!text.contains("line 0\n"), "the whole file was loaded");

        let env = dir.join(".env");
        std::fs::write(&env, "OPENAI_API_KEY=sk-x").unwrap();
        let e = from_file(&env.to_string_lossy()).unwrap_err();
        assert!(crate::msg::is("denied_even_with_consent", &e), "a denied file was loaded: {e}");
        assert_eq!(error_kind(&e), "denied");

        let e = from_file(&dir.to_string_lossy()).unwrap_err();
        assert_eq!(error_kind(&e), "not_file", "a directory was loaded as a log: {e}");
        assert_eq!(error_kind(&from_file(&dir.join("absent.log").to_string_lossy()).unwrap_err()), "unreadable");
        assert_eq!(error_kind(&from_container("a;b").unwrap_err()), "invalid");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The bounds a publisher reads in the spec are the bounds this client
    /// applies — how much is loaded, and how much may travel.
    #[test]
    fn the_excerpt_bounds_match_the_spec() {
        let raw = std::fs::read_to_string("../spec/vocabulary/reads.json").unwrap();
        let spec: Value = serde_json::from_str(&raw).unwrap();
        let x = &spec["excerpts"];
        assert_eq!(x["load_lines"], LOAD_LINES);
        assert_eq!(x["max_lines"], crate::redact::MAX_LINES);
        assert_eq!(x["max_chars"], crate::redact::MAX_CHARS);
        assert_eq!(spec["programs"]["run_limit_seconds"], 5);
    }

    /// Against a real Docker, where there is one: the running containers of an
    /// image are found by the image's name, its version is read without
    /// entering it, and the tail of its output loads.
    ///
    /// Skipped — and says so — where this machine has no Docker or nothing
    /// running, because a case that silently passes having attempted nothing
    /// is worse than one that admits it.
    #[test]
    fn a_running_container_is_found_by_its_image_and_its_output_loads() {
        let Some(docker) = reads::find_on_path("docker") else {
            eprintln!("no docker on this machine — container sources not attempted");
            return;
        };
        let Some((true, listing)) = reads::run_bounded(&docker, &["ps", "--format", "{{.Image}}"]) else {
            eprintln!("docker is not answering — container sources not attempted");
            return;
        };
        // **An untagged image reports as its own id**, and an id is not a name a
        // publisher could ever write — `containers_of` is right not to match
        // one. A compose file that builds without `image:` produces exactly
        // that, so this case failed on a machine whose own development stack
        // was running. Skipping it is not weakening the case: the property
        // under test is that a container named by its image is found, and a
        // bare id names nothing.
        let is_id = |l: &str| l.len() >= 12 && l.chars().all(|c| c.is_ascii_hexdigit());
        let Some(image) = listing.lines().map(str::trim)
            .find(|l| !l.is_empty() && !l.contains('@') && !is_id(l)) else {
            eprintln!("no container is running under an image name — container sources not attempted");
            return;
        };
        let repo = image.rsplit_once(':').filter(|(_, t)| !t.contains('/')).map(|(r, _)| r).unwrap_or(image);
        let found = reads::containers_of(repo);
        assert!(!found.is_empty(), "{repo} is running and was not found by its name");

        let v = reads::perform(&json!({"op": "container_image_version", "image": repo}));
        assert!(v.is_some(), "a running {repo} gave no version or tag at all");

        let r = from_container(repo).expect("the running container's output could not be loaded");
        assert!(r["from"].as_str().unwrap().starts_with("docker logs"), "{r}");
        assert!(r["lines"].as_u64().unwrap() as usize <= LOAD_LINES);
    }

    /// A publisher names an image, never a command: anything that is not a
    /// repository name is refused before Docker is asked anything.
    #[test]
    fn a_container_source_is_an_image_name_and_nothing_else() {
        for bad in ["ollama; rm -rf /", "Ollama/Ollama", "ollama:latest", "../x", "a b"] {
            let e = from_container(bad).unwrap_err();
            assert!(crate::msg::is("image_not_allowed", &e), "{bad:?} got past the image pattern: {e}");
        }
    }
}
