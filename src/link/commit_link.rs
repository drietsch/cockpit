//! Commit → feature correlation: explicit ids in the message first, then path ownership.

use super::{FeatureIdIndex, PathOwnership};
use crate::ids::FeatureId;
use crate::model::{
    Commit, Confidence, EntityRef, ExplicitSource, InferredSource, RelationKind, RelationOrigin,
    Relationship,
};

pub struct CommitLinks {
    pub relationships: Vec<Relationship>,
}

pub fn link_commit(
    commit: &Commit,
    idx: &FeatureIdIndex,
    ownership: &PathOwnership,
    allow_bare: bool,
) -> Vec<Relationship> {
    let mut out: Vec<Relationship> = Vec::new();
    let text = format!("{}\n{}", commit.subject, commit.body);
    let mut explicit: Vec<FeatureId> = Vec::new();
    for r in idx.find_refs(&text, allow_bare) {
        if let Some(id) = r.resolved {
            if !explicit.contains(&id) {
                explicit.push(id);
            }
        }
    }
    for id in &explicit {
        out.push(Relationship {
            from: EntityRef::Commit(commit.key.clone()),
            to: EntityRef::Feature(id.clone()),
            kind: RelationKind::CommitAffects,
            origin: RelationOrigin::Explicit(ExplicitSource::CommitMessage),
            confidence: Confidence::High,
        });
    }
    if !ownership.is_empty() {
        for f in &commit.files {
            for (id, glob) in ownership.owners_of(&f.path) {
                if out.iter().any(|r| matches!(&r.to, EntityRef::Feature(x) if x == id)) {
                    continue;
                }
                out.push(Relationship {
                    from: EntityRef::Commit(commit.key.clone()),
                    to: EntityRef::Feature(id.clone()),
                    kind: RelationKind::CommitAffects,
                    origin: RelationOrigin::Inferred(InferredSource::PathMatch {
                        glob: glob.to_string(),
                    }),
                    confidence: Confidence::Medium,
                });
            }
        }
    }
    out
}
