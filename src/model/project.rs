use crate::ids::{ComponentId, RepoId};
use crate::model::git::RepoHead;
use crate::paths::RelPath;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
    pub repos: Vec<Repo>,
    pub components: Vec<Component>,
    pub primary_branch: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Repo {
    pub id: RepoId,
    /// Project-root-relative path; empty for the root repo.
    pub path: RelPath,
    pub primary_branch: Option<String>,
    pub head: Option<RepoHead>,
    pub commit_count: u64,
    /// Why a repo is present but not scanned ("0 commits, skipped").
    pub note: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Component {
    pub id: ComponentId,
    pub title: String,
    pub group: Option<String>,
    pub order: u32,
}
