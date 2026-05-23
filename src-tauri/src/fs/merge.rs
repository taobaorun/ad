//! Deep JSON merge with conflict detection.
//!
//! Used by `commands::apply` when an AD profile's `layers.shared` or
//! `layers.local` is being applied to a project that already has a settings
//! file. The function merges the two JSON values and reports any conflicting
//! leaves so the UI can ask the user to resolve them.
//!
//! Behavior summary:
//! - **Both are objects**: deep merge — keys present only on one side are
//!   carried through; common keys recurse.
//! - **Both equal (anything)**: no conflict; returned value is `existing`.
//! - **Both arrays but unequal**: conflict (we don't try to be clever about
//!   array union/intersection — semantics depend on the field, so we surface
//!   it for explicit user resolution).
//! - **Mismatched types or unequal primitives**: conflict.
//!
//! The caller can resolve a conflict by passing a `Resolution` keyed by the
//! conflict's `key_path` (dot-separated path to the leaf).

use std::collections::{BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A single point of disagreement between `existing` and `incoming`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Conflict {
    /// Dot-separated path from the root, e.g. `permissions.allow.fs`.
    /// The root itself uses `""` (empty string).
    pub key_path: String,
    pub existing: Value,
    pub incoming: Value,
}

/// How to resolve a single conflict. The frontend collects these per
/// `key_path` and passes them back when re-invoking apply.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind", content = "value")]
pub enum Resolution {
    /// Keep what's already on disk.
    KeepExisting,
    /// Use the value from the AD profile.
    UseIncoming,
    /// Replace with a user-supplied value (free-form override).
    Custom(Value),
}

/// Outcome of a merge attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum MergeOutcome {
    /// All conflicts (if any) were resolved; this is the final value to write.
    Merged(Value),
    /// One or more conflicts were not covered by the supplied resolutions.
    /// The frontend should display these and re-invoke with `resolutions`
    /// covering each `key_path`.
    NeedsResolution(Vec<Conflict>),
}

/// Deep-merges `incoming` into `existing`, returning either the merged value
/// or the list of unresolved conflicts. `resolutions` may be empty for the
/// initial probe call.
pub fn merge(
    existing: &Value,
    incoming: &Value,
    resolutions: &HashMap<String, Resolution>,
) -> MergeOutcome {
    let mut unresolved = Vec::new();
    let merged = merge_walk(existing, incoming, "", resolutions, &mut unresolved);
    if unresolved.is_empty() {
        MergeOutcome::Merged(merged)
    } else {
        MergeOutcome::NeedsResolution(unresolved)
    }
}

fn merge_walk(
    existing: &Value,
    incoming: &Value,
    path: &str,
    resolutions: &HashMap<String, Resolution>,
    conflicts: &mut Vec<Conflict>,
) -> Value {
    match (existing, incoming) {
        // Both objects → deep merge.
        (Value::Object(e), Value::Object(i)) => {
            let mut keys: BTreeSet<&String> = BTreeSet::new();
            keys.extend(e.keys());
            keys.extend(i.keys());

            let mut out = serde_json::Map::with_capacity(keys.len());
            for key in keys {
                let new_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                let value = match (e.get(key), i.get(key)) {
                    (Some(ev), Some(iv)) => merge_walk(ev, iv, &new_path, resolutions, conflicts),
                    (Some(ev), None) => ev.clone(),
                    (None, Some(iv)) => iv.clone(),
                    (None, None) => unreachable!("key came from union of both maps"),
                };
                out.insert(key.clone(), value);
            }
            Value::Object(out)
        }
        // Equal at this leaf → no conflict.
        (e, i) if e == i => e.clone(),
        // Disagreement: look up resolution or report.
        (e, i) => match resolutions.get(path) {
            Some(Resolution::KeepExisting) => e.clone(),
            Some(Resolution::UseIncoming) => i.clone(),
            Some(Resolution::Custom(v)) => v.clone(),
            None => {
                conflicts.push(Conflict {
                    key_path: path.to_string(),
                    existing: e.clone(),
                    incoming: i.clone(),
                });
                // Tentative — caller will see NeedsResolution and ignore this.
                e.clone()
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn no_resolutions() -> HashMap<String, Resolution> {
        HashMap::new()
    }

    #[test]
    fn merges_disjoint_keys() {
        let existing = json!({"a": 1});
        let incoming = json!({"b": 2});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::Merged(v) => assert_eq!(v, json!({"a": 1, "b": 2})),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn equal_values_no_conflict() {
        let v = json!({"a": {"b": [1, 2, 3]}});
        match merge(&v, &v, &no_resolutions()) {
            MergeOutcome::Merged(out) => assert_eq!(out, v),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn primitive_difference_creates_conflict() {
        let existing = json!({"model": "sonnet"});
        let incoming = json!({"model": "opus"});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "model");
                assert_eq!(cs[0].existing, json!("sonnet"));
                assert_eq!(cs[0].incoming, json!("opus"));
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn nested_conflict_path_uses_dot_notation() {
        let existing = json!({"permissions": {"allow": {"fs": "ask"}}});
        let incoming = json!({"permissions": {"allow": {"fs": "always"}}});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "permissions.allow.fs");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn arrays_unequal_create_conflict() {
        let existing = json!({"allow": ["a"]});
        let incoming = json!({"allow": ["a", "b"]});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "allow");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn arrays_equal_no_conflict() {
        let existing = json!({"allow": ["a", "b"]});
        let incoming = json!({"allow": ["a", "b"]});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::Merged(v) => assert_eq!(v, json!({"allow": ["a", "b"]})),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn type_mismatch_creates_conflict() {
        let existing = json!({"x": 1});
        let incoming = json!({"x": "one"});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "x");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn object_vs_primitive_creates_conflict_at_parent() {
        let existing = json!({"x": {"a": 1}});
        let incoming = json!({"x": 5});
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "x");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn resolution_keep_existing_wins() {
        let existing = json!({"model": "sonnet"});
        let incoming = json!({"model": "opus"});
        let mut r = HashMap::new();
        r.insert("model".to_string(), Resolution::KeepExisting);
        match merge(&existing, &incoming, &r) {
            MergeOutcome::Merged(v) => assert_eq!(v, json!({"model": "sonnet"})),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn resolution_use_incoming_wins() {
        let existing = json!({"model": "sonnet"});
        let incoming = json!({"model": "opus"});
        let mut r = HashMap::new();
        r.insert("model".to_string(), Resolution::UseIncoming);
        match merge(&existing, &incoming, &r) {
            MergeOutcome::Merged(v) => assert_eq!(v, json!({"model": "opus"})),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn resolution_custom_wins() {
        let existing = json!({"model": "sonnet"});
        let incoming = json!({"model": "opus"});
        let mut r = HashMap::new();
        r.insert(
            "model".to_string(),
            Resolution::Custom(json!("claude-haiku-4-5")),
        );
        match merge(&existing, &incoming, &r) {
            MergeOutcome::Merged(v) => assert_eq!(v, json!({"model": "claude-haiku-4-5"})),
            other => panic!("expected Merged, got {other:?}"),
        }
    }

    #[test]
    fn partial_resolution_still_returns_unresolved() {
        let existing = json!({"a": 1, "b": 2});
        let incoming = json!({"a": 10, "b": 20});
        let mut r = HashMap::new();
        r.insert("a".to_string(), Resolution::UseIncoming);
        // `b` not resolved → should still report it.
        match merge(&existing, &incoming, &r) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "b");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn deep_merge_preserves_unrelated_branches() {
        let existing = json!({
            "permissions": { "allow": ["fs:read"], "deny": [] },
            "model": "sonnet"
        });
        let incoming = json!({
            "permissions": { "allow": ["fs:read"], "deny": ["bash:rm"] },
            "statusLine": { "command": "x" }
        });
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                // Only `permissions.deny` conflicts (arrays differ); model and
                // statusLine come from one side only and merge cleanly.
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "permissions.deny");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn root_level_conflict_uses_empty_path() {
        // When existing/incoming themselves are non-object primitives that
        // disagree, the conflict path is "" (root). This is an edge case
        // (typical inputs are always objects) but should not panic.
        let existing = json!(1);
        let incoming = json!(2);
        match merge(&existing, &incoming, &no_resolutions()) {
            MergeOutcome::NeedsResolution(cs) => {
                assert_eq!(cs.len(), 1);
                assert_eq!(cs[0].key_path, "");
            }
            other => panic!("expected NeedsResolution, got {other:?}"),
        }
    }

    #[test]
    fn resolution_custom_serde_roundtrip() {
        // Ensures the Resolution enum tags correctly for IPC.
        let r = Resolution::Custom(serde_json::json!({"x": 1}));
        let s = serde_json::to_string(&r).unwrap();
        let back: Resolution = serde_json::from_str(&s).unwrap();
        assert_eq!(r, back);

        let json = serde_json::to_value(&Resolution::KeepExisting).unwrap();
        assert_eq!(json, serde_json::json!({"kind": "keepExisting"}));
    }
}
