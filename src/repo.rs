// Copyright 2024 SAP SE
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
