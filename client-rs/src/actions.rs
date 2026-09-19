//! The validated action vocabulary — the boundary that bounds generation.
//!
//! **The registry lives here, on the client.** A vendor selects an action id and
//! fills declared parameters; it cannot ship a capability. A hallucinating or
//! compromised vendor agent can therefore mis-parameterise a tested operation —
//! which validation and dry-run catch — but cannot introduce one. This is why a
//! signed *script* would be strictly worse: a signature over arbitrary code
//! certifies origin while granting unbounded effect.

use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone)]
pub struct ActionSpec {
    pub id: &'static str,
    pub describes: &'static str,
    pub mutating: bool,
    pub reversible: bool,
    /// Needs administrator rights, and is therefore performed by the separate
    /// helper `podshl-elevate`, never in this process (`elevate.rs`). Windows
    /// only, for now.
    pub elevated: bool,
    /// parameter name -> anchored validation pattern
    pub params: &'static [(&'static str, &'static str)],
}

pub const VOCABULARY: &[ActionSpec] = &[
    ActionSpec {
        id: "report_only",
        describes: "State a finding, change nothing",
        mutating: false,
        reversible: true,
        elevated: false,
        params: &[],
    },
    ActionSpec {
        // Undo is a first-class capability, not a courtesy. A mutating action
        // that keeps a rollback copy but offers no way back leaves the user
        // holding a change they cannot evaluate.
        id: "restore_backup",
        describes: "Undo a change",
        mutating: true,
        reversible: false,
        elevated: false,
        params: &[("file", r"[\w./-]+\.(toml|ini|cfg|conf)")],
    },
    ActionSpec {
        id: "set_config_key",
        describes: "Set a key in a configuration file",
        mutating: true,
        reversible: true,
        elevated: false,
        params: &[
            ("file", r"[\w./-]+\.(toml|ini|cfg|conf)"),
            ("key", r"[A-Za-z_][\w.]{0,64}"),
            ("value", r"[\w.:/+-]{1,128}"),
        ],
    },
    // Three examples of what needs administrator rights, to be sharpened. The
    // patterns and the lists of what they never touch are in `elevated.rs`,
    // which the helper checks again.
    ActionSpec {
        id: "restart_service",
        describes: "Restart a Windows service (administrator)",
        mutating: true,
        reversible: false,
        elevated: true,
        params: &[("service", crate::elevated::SERVICE)],
    },
    ActionSpec {
        id: "set_service_start",
        describes: "Set how a Windows service starts (administrator)",
        mutating: true,
        reversible: true,
        elevated: true,
        params: &[
            ("service", crate::elevated::SERVICE),
            ("start", crate::elevated::START),
        ],
    },
    ActionSpec {
        id: "set_machine_env",
        describes: "Set or remove a machine-wide environment variable (administrator)",
        mutating: true,
        reversible: true,
        elevated: true,
        params: &[
            ("name", crate::elevated::ENV_NAME),
            ("value", crate::elevated::ENV_VALUE),
        ],
    },
];

pub fn spec(id: &str) -> Option<&'static ActionSpec> {
    VOCABULARY.iter().find(|a| a.id == id)
}

pub fn validate(id: &str, params: &Value) -> Result<BTreeMap<String, String>, String> {
    let s = spec(id).ok_or_else(|| {
        let known: Vec<&str> = VOCABULARY.iter().map(|a| a.id).collect();
        m!(
            "action_unknown",
            id = format!("{id:?}"),
            known = format!("{known:?}")
        )
    })?;
    let obj = params.as_object().ok_or_else(|| m!("params_not_object"))?;
    let mut out = BTreeMap::new();
    for (name, pattern) in s.params {
        let v = obj
            .get(*name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| m!("param_missing", id = id, name = format!("{name:?}")))?;
        let re = Regex::new(&format!("^(?:{pattern})$")).map_err(|e| e.to_string())?;
        if !re.is_match(v) {
            return Err(m!(
                "param_violates",
                id = id,
                name = name,
                v = format!("{v:?}"),
                pattern = pattern
            ));
        }
        out.insert((*name).to_string(), v.to_string());
    }
    for k in obj.keys() {
        if !s.params.iter().any(|(n, _)| n == k) {
            return Err(m!("param_unexpected", id = id, k = format!("{k:?}")));
        }
    }
    if s.elevated {
        crate::elevate::check(id, &out)?;
    }
    Ok(out)
}

/// Truthful, always. The consent prompt shows this, so a dry-run that
/// under-reports its own effect defeats the entire design.
pub fn dry_run(id: &str, params: &Value) -> Result<String, String> {
    let p = validate(id, params)?;
    Ok(match id {
        "report_only" => m!("dry_report_only"),
        "set_config_key" => m!(
            "dry_set_config_key",
            key = p["key"],
            value = p["value"],
            file = p["file"]
        ),
        "restore_backup" => m!("dry_restore_backup", file = p["file"]),
        "restart_service" => m!("dry_restart_service", s = p["service"]),
        "set_service_start" => m!(
            "dry_set_service_start",
            s = p["service"],
            start = p["start"]
        ),
        "set_machine_env" if p["value"].is_empty() => m!("dry_unset_machine_env", n = p["name"]),
        "set_machine_env" => m!("dry_set_machine_env", n = p["name"], v = p["value"]),
        _ => "—".into(),
    })
}

pub fn execute(id: &str, params: &Value, root: &Path) -> Result<Value, String> {
    let p = validate(id, params)?;
    match id {
        "report_only" => Ok(serde_json::json!({ "reported": true })),
        "set_config_key" => set_config_key(&p, root),
        "restore_backup" => restore_backup(&p, root),
        _ if spec(id).is_some_and(|s| s.elevated) => crate::elevate::run(id, &p),
        _ => Err(m!("not_implemented", id = id)),
    }
}

fn set_config_key(p: &BTreeMap<String, String>, root: &Path) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target: PathBuf = root.join(&p["file"]);
    let target = target
        .canonicalize()
        .map_err(|e| format!("{}: {e}", p["file"]))?;
    if !target.starts_with(&root) {
        return Err(m!("path_leaves_root_named", p = p["file"]));
    }
    let text = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
    let backup = target.with_extension(format!(
        "{}.bak",
        target.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&backup, &text).map_err(|e| e.to_string())?;

    let re = Regex::new(&format!(
        r"(?m)^(\s*{}\s*=\s*).*$",
        regex::escape(&p["key"])
    ))
    .map_err(|e| e.to_string())?;
    let (new, n) = if re.is_match(&text) {
        (
            re.replace_all(&text, format!("${{1}}{}", p["value"]))
                .to_string(),
            1,
        )
    } else {
        (
            format!("{}\n{} = {}\n", text.trim_end(), p["key"], p["value"]),
            1,
        )
    };
    std::fs::write(&target, new).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "file": target.display().to_string(),
        "replaced": n,
        "backup": backup.display().to_string()
    }))
}

fn restore_backup(p: &BTreeMap<String, String>, root: &Path) -> Result<Value, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = root
        .join(&p["file"])
        .canonicalize()
        .map_err(|e| e.to_string())?;
    if !target.starts_with(&root) {
        return Err(m!("path_leaves_root"));
    }
    let backup = target.with_extension(format!(
        "{}.bak",
        target.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let text = std::fs::read_to_string(&backup).map_err(|_| m!("no_backup"))?;
    std::fs::write(&target, text).map_err(|e| e.to_string())?;
    Ok(serde_json::json!({ "restored": target.display().to_string() }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("vs-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    /// A1: the order is dry-run, then execute — and the dry-run must name the
    /// file it would touch, because that is what the user is consenting to.
    #[test]
    fn a_known_action_dry_runs_and_then_executes() {
        let root = tmp();
        let f = root.join("training.toml");
        std::fs::write(&f, "precision = \"bf16\"\nbatch_size = 4\n").unwrap();
        let p = json!({"file": "training.toml", "key": "precision", "value": "fp16"});
        let preview = dry_run("set_config_key", &p).expect("dry-run refused a valid action");
        assert!(
            preview.contains("training.toml"),
            "dry-run does not name the file: {preview}"
        );
        assert!(
            std::fs::read_to_string(&f).unwrap().contains("bf16"),
            "the dry-run changed the file — a dry-run that acts defeats the design"
        );
        execute("set_config_key", &p, &root).unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().contains("fp16"));
    }

    /// A2: and the refusal names the whole vocabulary back at the vendor, so a
    /// developer learns what *is* possible rather than only what is not.
    #[test]
    fn an_unknown_action_is_refused_naming_the_vocabulary() {
        let err = validate("run_powershell", &json!({})).unwrap_err();
        for known in VOCABULARY.iter().map(|a| a.id) {
            assert!(
                err.contains(known),
                "the refusal does not name {known}: {err}"
            );
        }
    }

    /// A3-A6: the parameter contract. Each of these reached a real client at
    /// some point in some product; none of them is hypothetical.
    #[test]
    fn parameters_are_validated_before_anything_runs() {
        for (params, why) in [
            (
                json!({"file": "../../etc/passwd", "key": "X", "value": "1"}),
                "path traversal",
            ),
            (
                json!({"file": "training.toml", "key": "X; rm -rf /", "value": "1"}),
                "shell metacharacters",
            ),
            (
                json!({"file": "training.toml", "key": "X", "value": "1", "extra": "y"}),
                "an undeclared parameter",
            ),
            (
                json!({"file": "training.toml", "key": "X"}),
                "a missing required parameter",
            ),
        ] {
            assert!(
                validate("set_config_key", &params).is_err(),
                "accepted {why}: {params}"
            );
        }
    }

    /// A7: a mutating action leaves a way back before it changes anything.
    #[test]
    fn a_mutating_action_leaves_a_rollback_copy() {
        let root = tmp();
        let f = root.join("rollback.toml");
        let bak = root.join("rollback.toml.bak");
        std::fs::write(&f, "precision = \"bf16\"\n").unwrap();
        let _ = std::fs::remove_file(&bak);
        execute(
            "set_config_key",
            &json!({"file": "rollback.toml", "key": "precision", "value": "fp16"}),
            &root,
        )
        .unwrap();
        assert!(bak.exists(), "no .bak written");
        assert!(
            std::fs::read_to_string(&bak).unwrap().contains("bf16"),
            "the backup does not hold the previous content"
        );
    }

    /// The sandbox is bounded by path components, not by a string prefix. The
    /// other implementation used `startswith`, so `../var-evil/x.toml` matched
    /// the declared pattern, resolved to a sibling of the root and passed —
    /// while the case guarding it only ever tried `../../etc/passwd`, which
    /// fails the extension pattern long before the path check runs.
    #[test]
    fn a_sibling_of_the_root_is_not_inside_the_root() {
        let base = std::env::temp_dir().join(format!("vs-escape-{}", std::process::id()));
        let root = base.join("var");
        let sibling = base.join("var-evil");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("x.toml"), "a = 1\n").unwrap();

        let p = json!({"file": "../var-evil/x.toml", "key": "a", "value": "2"});
        assert!(
            validate("set_config_key", &p).is_ok(),
            "the pattern itself permits this, which is why the path check must catch it"
        );
        assert!(
            execute("set_config_key", &p, &root).is_err(),
            "wrote outside the root: a prefix comparison would have allowed this"
        );
        assert_eq!(
            std::fs::read_to_string(sibling.join("x.toml")).unwrap(),
            "a = 1\n",
            "the file outside the root was modified"
        );
    }

    /// A8: a mutating action must leave a way back, and undo must use it.
    #[test]
    fn undo_restores_from_the_backup() {
        let root = tmp();
        let f = root.join("t.toml");
        std::fs::write(&f, "precision = \"bf16\"\n").unwrap();
        let p = json!({"file": "t.toml", "key": "precision", "value": "fp16"});
        execute("set_config_key", &p, &root).unwrap();
        assert!(std::fs::read_to_string(&f).unwrap().contains("fp16"));
        execute("restore_backup", &json!({"file": "t.toml"}), &root).unwrap();
        assert!(
            std::fs::read_to_string(&f).unwrap().contains("bf16"),
            "undo did not restore"
        );
    }

    /// A9: undo with nothing to undo refuses cleanly rather than panicking.
    #[test]
    fn undo_without_a_backup_refuses_cleanly() {
        let root = tmp();
        let f = root.join("nobak.toml");
        std::fs::write(&f, "a = 1\n").unwrap();
        let err = execute("restore_backup", &json!({"file": "nobak.toml"}), &root).unwrap_err();
        assert!(crate::msg::is("no_backup", &err), "unhelpful error: {err}");
    }

    /// The vocabulary is the security boundary, and a boundary that is written
    /// down in two places is two boundaries. This once drifted: the Python
    /// reference carried `set_env_var`, which this client refuses, and nothing
    /// noticed — the case that guards the boundary only asserted that one id
    /// common to both appeared in the refusal.
    #[test]
    fn vocabulary_matches_the_spec() {
        let raw = std::fs::read_to_string("../spec/vocabulary/actions.json")
            .expect("spec/vocabulary/actions.json is missing");
        let spec: Value = serde_json::from_str(&raw).expect("actions.json is not JSON");

        let documented: Vec<Value> = spec["actions"].as_array().cloned().unwrap_or_default();
        let implemented: Vec<Value> = VOCABULARY
            .iter()
            .map(|a| {
                let params: serde_json::Map<String, Value> = a
                    .params
                    .iter()
                    .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
                    .collect();
                json!({"id": a.id, "describes": a.describes, "mutating": a.mutating,
                       "reversible": a.reversible, "elevated": a.elevated, "params": params})
            })
            .collect();

        let ids = |v: &[Value]| -> Vec<String> {
            let mut o: Vec<String> = v
                .iter()
                .map(|a| a["id"].as_str().unwrap_or_default().to_string())
                .collect();
            o.sort();
            o
        };
        assert_eq!(
            ids(&documented),
            ids(&implemented),
            "the specified vocabulary and the implemented one name different actions"
        );

        for want in &documented {
            let id = want["id"].as_str().unwrap();
            let got = implemented.iter().find(|a| a["id"] == want["id"]).unwrap();
            assert_eq!(
                got, want,
                "action {id} differs between the spec and this client"
            );
        }
    }
}
