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

use std::sync::Arc;

use anyhow::Context;
use tokio::task::JoinSet;

use crate::api_clients::Client;
use crate::github::{Commit, Review};
use crate::remote::Remote;

#[derive(Debug)]
pub struct RepoChangeset<C: Client> {
    pub name: String,
    pub remote: Remote<C>,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Changeset>,
}

impl<C: Client + Sync + Send + 'static> RepoChangeset<C> {
    pub async fn analyze_commits(mut self) -> anyhow::Result<Self> {
        let compare_commits = self.remote.compare(&self.base_commit, &self.head_commit).await?;

        let mut join_set = JoinSet::new();
        let remote = Arc::new(self.remote);
        for commit in compare_commits {
            join_set.spawn(Self::analyze_commit(remote.clone(), commit));
        }

        let mut changesets: Vec<Changeset> = vec![];
        while let Some(res) = join_set.join_next().await {
            let changes = res?.context("while collecting change")?;
            for change in &changes {
                changesets.push(change.clone());
            }
        }

        for change in &changesets {
            if let Some(self_change) = self
                .changes
                .iter_mut()
                .find(|self_change| self_change.pr_link == change.pr_link)
            {
                for approval in &change.approvals {
                    self_change.approvals.push(approval.clone());
                }
                continue;
            }

            self.changes.push(change.clone());
        }

        self.remote = Arc::into_inner(remote).unwrap();
        Ok(self)
    }

    async fn analyze_commit(remote: Arc<Remote<C>>, commit: Commit) -> anyhow::Result<Vec<Changeset>> {
        let change_commit = CommitMetadata::new(&commit);
        let mut changes = vec![];

        let associated_prs = remote.associated_prs(commit.sha.clone()).await?;
        if associated_prs.is_empty() {
            changes.push(Changeset {
                commits: vec![change_commit],
                pr_link: None,
                approvals: Vec::new(),
            });
            return Ok(changes);
        }

        for associated_pr in &associated_prs {
            let mut changeset = Changeset {
                commits: vec![change_commit.clone()],
                pr_link: Some(associated_pr.url.clone()),
                approvals: Vec::new(),
            };

            let pr_reviews = remote.pr_reviews(associated_pr.number).await?;
            let head_sha = remote.pr_head_hash(associated_pr.number).await?;
            changeset.collect_approved_reviews(&pr_reviews, &head_sha);

            changes.push(changeset);
        }

        Ok(changes)
    }
}

#[derive(Clone, Debug, PartialEq)]
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

#[derive(Clone, Debug, PartialEq)]
pub struct CommitMetadata {
    pub headline: String,
    pub link: String,
}

impl CommitMetadata {
    pub fn new(commit: &Commit) -> Self {
        let headline = commit
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_clients::{ClientSet, MockClient};
    use crate::github::{PullRequest, Review};

    fn gen_change_review() -> (Changeset, Vec<Review>) {
        (
            Changeset {
                commits: vec![
                    CommitMetadata {
                        headline: "Commit 1".to_owned(),
                        link: "https://github.com/example/project/commit/00000000000000000000000000000001".to_owned(),
                    },
                    CommitMetadata {
                        headline: "Commit 2".to_owned(),
                        link: "https://github.com/example/project/commit/00000000000000000000000000000002".to_owned(),
                    },
                ],
                pr_link: Some("https://github.com/example/project/pulls/1".to_owned()),
                approvals: Vec::new(),
            },
            vec![
                Review {
                    approved: true,
                    commit_id: "00000000000000000000000000000001".to_owned(),
                    submitted_at: 1,
                    user: "user1".to_owned(),
                },
                Review {
                    approved: true,
                    commit_id: "00000000000000000000000000000002".to_owned(),
                    submitted_at: 2,
                    user: "user2".to_owned(),
                },
                Review {
                    approved: false,
                    commit_id: "00000000000000000000000000000003".to_owned(),
                    submitted_at: 3,
                    user: "user3".to_owned(),
                },
            ],
        )
    }

    #[test]
    fn collect_approved_reviews() {
        let (mut changeset, pr_reviews) = gen_change_review();
        changeset.collect_approved_reviews(&pr_reviews, &"00000000000000000000000000000002".to_owned());
        assert_eq!(changeset.approvals, vec!["user2"]);
    }

    #[test]
    fn collect_approved_reviews_extra_commit() {
        let (mut changeset, pr_reviews) = gen_change_review();
        changeset.collect_approved_reviews(&pr_reviews, &"00000000000000000000000000000003".to_owned());
        assert_eq!(changeset.approvals, Vec::<String>::new());
    }

    fn get_mock_remote() -> Remote<MockClient> {
        let mut api_clients = ClientSet::new();
        let mut remote = Remote::<MockClient>::parse("https://github.com/example/project.git").unwrap();
        api_clients.fill(&mut remote).unwrap();

        remote
    }

    #[tokio::test]
    async fn analyze_commit_approved() {
        let mut remote = get_mock_remote();
        let remote_client = (&mut remote.client).as_ref().unwrap();

        remote_client
            .associated_prs
            .lock()
            .unwrap()
            .insert("00000000000000000000000000000002".to_string(), vec![PullRequest {
                number: 1,
                url: "https://github.com/example/project/pulls/1".to_owned(),
            }]);

        remote_client.pr_reviews.lock().unwrap().insert(1, vec![
            Review {
                approved: false,
                commit_id: "00000000000000000000000000000001".to_owned(),
                submitted_at: 42,
                user: "user1".to_owned(),
            },
            Review {
                approved: true,
                commit_id: "00000000000000000000000000000002".to_owned(),
                submitted_at: 42,
                user: "user1".to_owned(),
            },
        ]);

        remote_client
            .pr_head_hash
            .lock()
            .unwrap()
            .insert(1, "00000000000000000000000000000002".to_owned());

        let changeset = RepoChangeset::analyze_commit(remote.into(), Commit {
            html_url: "https://github.com/example/project/commit/00000000000000000000000000000002".to_owned(),
            message: "Testing test".to_owned(),
            sha: "00000000000000000000000000000002".to_owned(),
        })
        .await
        .unwrap();

        assert_eq!(changeset.len(), 1);
        assert_eq!(changeset[0], Changeset {
            approvals: vec!["user1".to_owned()],
            commits: vec![CommitMetadata {
                headline: "Testing test".to_owned(),
                link: "https://github.com/example/project/commit/00000000000000000000000000000002".to_owned(),
            }],
            pr_link: Some("https://github.com/example/project/pulls/1".to_owned()),
        });
    }

    #[tokio::test]
    async fn analyze_commit_none() {
        let mut remote = get_mock_remote();
        let remote_client = (&mut remote.client).as_ref().unwrap();

        remote_client
            .associated_prs
            .lock()
            .unwrap()
            .insert("00000000000000000000000000000002".to_string(), vec![PullRequest {
                number: 1,
                url: "https://github.com/example/project/pulls/2".to_owned(),
            }]);

        remote_client.pr_reviews.lock().unwrap().insert(1, vec![Review {
            approved: false,
            commit_id: "00000000000000000000000000000001".to_owned(),
            submitted_at: 42,
            user: "user1".to_owned(),
        }]);

        remote_client
            .pr_head_hash
            .lock()
            .unwrap()
            .insert(1, "00000000000000000000000000000003".to_owned());

        let changeset = RepoChangeset::analyze_commit(remote.into(), Commit {
            html_url: "https://github.com/example/project/commit/00000000000000000000000000000002".to_owned(),
            message: "Testing test".to_owned(),
            sha: "00000000000000000000000000000002".to_owned(),
        })
        .await
        .unwrap();

        assert_eq!(changeset.len(), 1);
        assert_eq!(changeset[0], Changeset {
            approvals: vec![],
            commits: vec![CommitMetadata {
                headline: "Testing test".to_owned(),
                link: "https://github.com/example/project/commit/00000000000000000000000000000002".to_owned(),
            }],
            pr_link: Some("https://github.com/example/project/pulls/2".to_owned()),
        });
    }
}
