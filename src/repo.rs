use anyhow::Context;
use git2::{Repository, Tree};

pub fn tree_for_commit_ref<'r>(repo: &'r Repository, commit_ref: &'_ str) -> Result<Tree<'r>, anyhow::Error> {
    let commit_id = repo
        .revparse_single(commit_ref)
        .with_context(|| format!("cannot revparse {commit_ref:?}"))?
        .id();
    let commit = repo
        .find_commit(commit_id)
        .with_context(|| format!("cannot find Git commit {commit_id}"))?;
    let tree = commit
        .tree()
        .with_context(|| format!("cannot find tree for Git commit {commit_id}"))?;
    Ok(tree)
}
