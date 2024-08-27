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

use anyhow::{anyhow, Context};
use octocrab::models::commits::Commit;
use tokio::task::JoinSet;

use crate::github::Review;
use crate::remote::Remote;

#[derive(Clone, Debug)]
pub struct RepoChangeset {
    pub name: String,
    pub remote: Remote,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Changeset>,
}

impl RepoChangeset {
    pub async fn analyze_commits(mut self) -> Result<Self, anyhow::Error> {
        let compare = self.remote.compare(&self.base_commit, &self.head_commit).await?;

        let mut join_set = JoinSet::new();
        for commit in compare.commits {
            join_set.spawn(self.clone().analyze_commit(commit));
        }

        while let Some(res) = join_set.join_next().await {
            let changes = res?.context("while collecting change")?;
            for change in changes {
                self.changes.push(change);
            }
        }

        Ok(self)
    }

    async fn analyze_commit(mut self, commit: Commit) -> Result<Vec<Changeset>, anyhow::Error> {
        let change_commit = CommitMetadata::new(&commit);

        let associated_prs = self.remote.associated_prs(&commit).await?;
        if associated_prs.is_empty() {
            self.changes.push(Changeset {
                commits: vec![change_commit],
                pr_link: None,
                approvals: Vec::new(),
            });
            return Ok(self.changes);
        }

        for associated_pr in &associated_prs {
            let pr_reviews = self.remote.pr_reviews(associated_pr.number).await?;

            let associated_pr_link = Some(
                associated_pr
                    .html_url
                    .as_ref()
                    .ok_or_else(|| anyhow!("pr without an html link!?"))?
                    .to_string(),
            );

            let head_sha = self.remote.pr_head_hash(associated_pr.number).await?;

            if let Some(changeset) = self.changes.iter_mut().find(|cs| cs.pr_link == associated_pr_link) {
                changeset.commits.push(change_commit.clone());
                changeset.collect_approved_reviews(&pr_reviews, &head_sha);
                continue;
            }

            let mut changeset = Changeset {
                commits: vec![change_commit.clone()],
                pr_link: associated_pr_link,
                approvals: Vec::new(),
            };

            changeset.collect_approved_reviews(&pr_reviews, &head_sha);
            self.changes.push(changeset);
        }

        Ok(self.changes)
    }
}

#[derive(Clone, Debug)]
pub struct Changeset {
    pub commits: Vec<CommitMetadata>,
    pub pr_link: Option<String>,
    pub approvals: Vec<String>,
}

impl Changeset {
    // pr_reviews must be sorted by key submitted_at!
    pub fn collect_approved_reviews(&mut self, pr_reviews: &[Review], head_sha: &String) {
        let mut last_review_by: Vec<String> = vec![];

        // reverse the order of reviews to start with the oldest
        for pr_review in pr_reviews.iter().rev() {
            // Only consider the last review of any user.
            // For example a user might have requested changes early on in the PR and later approved it
            // or requested additional changes after supplying an approval first.
            if last_review_by.contains(&pr_review.user) {
                continue;
            }
            last_review_by.push(pr_review.user.clone());

            // Only account for reviews done on the last commit of the PR.
            // We could count the PR as partly reviewed but that is to complicated to present at the moment.
            if pr_review.commit_id != *head_sha {
                continue;
            }

            // in case it isn't approve, ignore it
            if !pr_review.approved {
                continue;
            }

            // don't duplicate user names
            if !self.approvals.contains(&pr_review.user) {
                self.approvals.push(pr_review.user.clone());
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct CommitMetadata {
    pub headline: String,
    pub link: String,
}

impl CommitMetadata {
    pub fn new(commit: &Commit) -> Self {
        let headline = commit
            .commit
            .message
            .split('\n')
            .next()
            .unwrap_or("<empty commit message>")
            .to_string();
        Self {
            headline,
            link: commit.html_url.clone(),
        }
    }
}
