//! Change-freshness (§28–§29): evidence is valid if it was generated after the latest relevant
//! implementation change. Time-freshness is orthogonal and secondary.

use crate::git::CommitGraph;
use crate::ids::{CommitKey, RepoId};
use crate::model::*;
use jiff::{SignedDuration, Timestamp};
use std::collections::HashMap;

pub struct FreshnessInput<'a> {
    pub graphs: &'a HashMap<RepoId, CommitGraph>,
    pub impl_commit: Option<&'a CommitKey>,
    pub impl_at: Option<Timestamp>,
    pub max_age: SignedDuration,
    pub now: Timestamp,
}

pub fn evaluate(
    commit: Option<&CommitRef>,
    at: Timestamp,
    at_origin: crate::time::Origin,
    input: &FreshnessInput,
) -> Freshness {
    let age = input.now.duration_since(at);
    let time_stale = age > input.max_age;
    let mut note;
    let (change, confidence) = match (commit, input.impl_commit) {
        (Some(c), Some(i)) if c.resolved && c.key.repo == i.repo => {
            let g = input.graphs.get(&i.repo);
            let is_after =
                c.key.sha == i.sha || g.map(|g| g.is_ancestor(&i.sha, &c.key.sha)).unwrap_or(false);
            let is_before =
                !is_after && g.map(|g| g.is_ancestor(&c.key.sha, &i.sha)).unwrap_or(false);
            let conf = match c.origin {
                CommitOrigin::Declared | CommitOrigin::Sidecar => Confidence::High,
                _ => Confidence::Medium,
            };
            if is_after {
                note =
                    format!("generated at {} which includes {}", c.key.sha.short(), i.sha.short());
                (ChangeFreshness::Fresh, conf)
            } else if is_before {
                note = format!(
                    "generated at {}, before implementation change {}",
                    c.key.sha.short(),
                    i.sha.short()
                );
                (ChangeFreshness::Stale, conf)
            } else {
                // Diverged histories: fall back to timestamps.
                let fresh = input.impl_at.map(|t| at >= t).unwrap_or(false);
                note = "evidence commit and implementation commit are on diverged branches; compared by time".into();
                (
                    if fresh { ChangeFreshness::Fresh } else { ChangeFreshness::Stale },
                    Confidence::Low,
                )
            }
        }
        (Some(c), Some(i)) if !c.resolved => {
            let fresh = input.impl_at.map(|t| at >= t).unwrap_or(false);
            note = format!(
                "commit {} is not in local history; compared by time against {}",
                c.key.sha.short(),
                i.sha.short()
            );
            (if fresh { ChangeFreshness::Fresh } else { ChangeFreshness::Stale }, Confidence::Low)
        }
        (Some(_), Some(i)) => {
            let fresh = input.impl_at.map(|t| at >= t).unwrap_or(false);
            note = format!(
                "evidence from another repository; compared by time against {}",
                i.sha.short()
            );
            (if fresh { ChangeFreshness::Fresh } else { ChangeFreshness::Stale }, Confidence::Low)
        }
        (None, Some(i)) => {
            let fresh = input.impl_at.map(|t| at >= t).unwrap_or(false);
            let conf = if at_origin == crate::time::Origin::Declared {
                Confidence::Medium
            } else {
                Confidence::Low
            };
            note = format!("no commit recorded; compared by time against {}", i.sha.short());
            (if fresh { ChangeFreshness::Fresh } else { ChangeFreshness::Stale }, conf)
        }
        (_, None) => {
            note = "no implementation commit known for this feature".into();
            (ChangeFreshness::Unknown, Confidence::Medium)
        }
    };
    if time_stale {
        note.push_str(&format!("; {} old", crate::time::span_compact(age)));
    }
    Freshness {
        change,
        against: input.impl_commit.cloned(),
        age: Some(age),
        time_stale,
        confidence,
        note,
    }
}
