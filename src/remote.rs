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

use anyhow::{anyhow, Context};
use octocrab::commits::PullRequestTarget;
use octocrab::models::commits::CommitComparison;
use octocrab::models::pulls::{PullRequest, ReviewState};
use octocrab::models::repos::RepoCommit;
use octocrab::Octocrab;
use tokio::sync::SemaphorePermit;
use url::Url;

use crate::api_clients::Client;
use crate::github::Review;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Remote {
    pub host: url::Host,
    pub port: u16,
    pub owner: String,
    pub repository: String,
    pub original: String,
    pub client: Option<Arc<Client>>,
}

impl Remote {
    pub fn parse(url: &str) -> Result<Self, anyhow::Error> {
        let remote_url = Url::parse(url).context("can't parse remote")?;
        let path_elements: Vec<&str> = remote_url.path().trim_start_matches('/').split('/').collect();
        Ok(Self {
            host: remote_url.host().context("remote has no host")?.to_owned(),
            port: remote_url.port_or_known_default().context("remote has no port")?,
            owner: path_elements[0].to_string(),
            repository: path_elements[1].trim_end_matches(".git").to_string(),
            original: url.into(),
            client: None,
        })
    }

    async fn get_client(&self) -> Result<(SemaphorePermit<'_>, &Arc<Octocrab>), anyhow::Error> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow!("no client attached to remote"))?;
        client.lock().await.context("cannot obtain semaphore for client")
    }

    pub async fn associated_prs(&self, sha: String) -> Result<Vec<PullRequest>, anyhow::Error> {
        let (_permit, octocrab) = self.get_client().await?;

        let mut associated_prs_page = octocrab
            .commits(&self.owner, &self.repository)
            .associated_pull_requests(PullRequestTarget::Sha(sha))
            .send()
            .await
            .context("failed to get associated prs")?;
        assert!(
            associated_prs_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        Ok(associated_prs_page.take_items())
    }

    pub async fn compare(&self, base_commit: &str, head_commit: &str) -> Result<CommitComparison, anyhow::Error> {
        let (_permit, octocrab) = self.get_client().await?;

        octocrab
            .commits(&self.owner, &self.repository)
            .compare(base_commit, head_commit)
            .send()
            .await
            .context(format!(
                "failed to compare {}/compare/{}...{}",
                self.original.trim_end_matches(".git"),
                &base_commit,
                &head_commit
            ))
    }

    pub async fn pr_head_hash(&self, pr_number: u64) -> Result<String, anyhow::Error> {
        Ok(self
            .pr_commits(pr_number)
            .await
            .context("failed to get pr commits")?
            .last()
            .ok_or_else(|| anyhow!("PR contains no commits?"))?
            .sha
            .clone())
    }

    pub async fn pr_commits(&self, pr_number: u64) -> Result<Vec<RepoCommit>, anyhow::Error> {
        let (_permit, octocrab) = self.get_client().await?;

        let mut pr_commits_page = octocrab
            .pulls(&self.owner, &self.repository)
            .pr_commits(pr_number)
            .await
            .context("failed to get pr commits")?;
        assert!(
            pr_commits_page.next.is_none(),
            "found more than one page for associated_prs"
        );

        let pr_commits = pr_commits_page.take_items();
        assert!(
            pr_commits.len() <= 250,
            "found more than 250 commits which requires a different api endpoint per doc"
        );

        Ok(pr_commits)
    }

    pub async fn pr_reviews(&self, pr_number: u64) -> Result<Vec<Review>, anyhow::Error> {
        let (_permit, octocrab) = self.get_client().await?;

        let mut pr_reviews_page = octocrab
            .pulls(&self.owner, &self.repository)
            .list_reviews(pr_number)
            .send()
            .await
            .context("failed to get reviews")?;
        assert!(
            pr_reviews_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        let pr_reviews = pr_reviews_page.take_items();

        let mut reviews = Vec::new();
        for pr_review in &pr_reviews {
            reviews.push(Review {
                approved: pr_review.state == Some(ReviewState::Approved),
                commit_id: pr_review.commit_id.clone().ok_or(anyhow!("review has no commit_id"))?,
                submitted_at: pr_review
                    .submitted_at
                    .ok_or_else(|| anyhow!("review has no submitted_at"))?
                    .timestamp_micros(),
                user: pr_review.user.clone().ok_or(anyhow!("review has no user"))?.login,
            });
        }

        reviews.sort_by_key(|r| r.submitted_at);
        Ok(reviews)
    }
}
