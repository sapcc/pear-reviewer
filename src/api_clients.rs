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

use std::collections::HashMap;
use std::env;
use std::future::Future;
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context};
use octocrab::commits::PullRequestTarget;
use octocrab::models::pulls::ReviewState;
use octocrab::models::repos::RepoCommit;
use octocrab::Octocrab;
use tokio::sync::Semaphore;

use crate::github::{Commit, PullRequest, Review};
use crate::remote::Remote;

#[derive(Debug)]
pub struct RealClient {
    semaphore: Semaphore,
    octocrab: Arc<Octocrab>,
}

pub trait Client {
    fn new(env_name: String, api_endpoint: String) -> anyhow::Result<Arc<Self>>;

    fn associated_prs(
        &self,
        owner: &str,
        repo: &str,
        sha: String,
    ) -> impl Future<Output = anyhow::Result<Vec<PullRequest>>> + Send;

    async fn compare(
        &self,
        owner: &str,
        repo: &str,
        original: &str,
        base_commit: &str,
        head_commit: &str,
    ) -> anyhow::Result<Vec<Commit>>;

    async fn pr_commits(&self, owner: &str, repo: &str, pr_number: u64) -> anyhow::Result<Vec<RepoCommit>>;

    fn pr_head_hash(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> impl Future<Output = anyhow::Result<String>> + Send;

    fn pr_reviews(
        &self,
        owner: &str,
        repo: &str,
        pr_number: u64,
    ) -> impl Future<Output = anyhow::Result<Vec<Review>>> + Send;
}

impl Client for RealClient {
    fn new(env_name: String, api_endpoint: String) -> anyhow::Result<Arc<RealClient>> {
        octocrab::initialise(
            Octocrab::builder()
                .personal_token(env::var(&env_name).with_context(|| format!("missing {env_name} env"))?)
                .base_uri(&api_endpoint)
                .with_context(|| format!("failed to set base_uri to {api_endpoint}"))?
                .build()
                .context("failed to build octocrab client")?,
        );
        Ok(Arc::new(Self {
            semaphore: Semaphore::new(5), // i.e. up to 5 API calls in parallel to the same GitHub instance
            octocrab: octocrab::instance(),
        }))
    }

    async fn associated_prs(&self, owner: &str, repo: &str, sha: String) -> anyhow::Result<Vec<PullRequest>> {
        let _permit = self.semaphore.acquire().await?;

        let mut associated_prs_page = self
            .octocrab
            .commits(owner, repo)
            .associated_pull_requests(PullRequestTarget::Sha(sha))
            .send()
            .await
            .context("failed to get associated prs")?;
        assert!(
            associated_prs_page.next.is_none(),
            "found more than one page for associated_prs"
        );

        let associated_prs = associated_prs_page.take_items();

        let mut prs: Vec<PullRequest> = Vec::new();
        for associated_pr in associated_prs {
            let associated_pr_url = associated_pr
                .html_url
                .as_ref()
                .ok_or_else(|| anyhow!("pr without an html link!?"))?
                .to_string();

            prs.push(PullRequest {
                number: associated_pr.number,
                url: associated_pr_url,
            });
        }

        Ok(prs)
    }

    async fn compare(
        &self,
        owner: &str,
        repo: &str,
        original: &str,
        base_commit: &str,
        head_commit: &str,
    ) -> anyhow::Result<Vec<Commit>> {
        let _permit = self.semaphore.acquire().await?;

        let compare = self
            .octocrab
            .commits(owner, repo)
            .compare(base_commit, head_commit)
            .send()
            .await
            .context(format!(
                "failed to compare {}/compare/{}...{}",
                original.trim_end_matches(".git"),
                &base_commit,
                &head_commit
            ))?;

        let mut commits: Vec<Commit> = vec![];
        for commit in compare.commits {
            commits.push(Commit {
                html_url: commit.html_url,
                message: commit.commit.message,
                sha: commit.sha,
            });
        }

        Ok(commits)
    }

    async fn pr_head_hash(&self, owner: &str, repo: &str, pr_number: u64) -> Result<String, anyhow::Error> {
        Ok(self
            .pr_commits(owner, repo, pr_number)
            .await
            .context("failed to get pr commits")?
            .last()
            .ok_or_else(|| anyhow!("PR {owner}/{repo}/pull/{pr_number} contains no commits?"))?
            .sha
            .clone())
    }

    async fn pr_commits(&self, owner: &str, repo: &str, pr_number: u64) -> anyhow::Result<Vec<RepoCommit>> {
        let _permit = self.semaphore.acquire().await?;

        let mut pr_commits_page = self
            .octocrab
            .pulls(owner, repo)
            .pr_commits(pr_number)
            .per_page(250)
            .page(1u32)
            .send()
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

    async fn pr_reviews(&self, owner: &str, repo: &str, pr_number: u64) -> anyhow::Result<Vec<Review>> {
        let _permit = self.semaphore.acquire().await?;

        let mut pr_reviews_page = self
            .octocrab
            .pulls(owner, repo)
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

#[derive(Debug)]
pub struct MockClient {
    pub associated_prs: Mutex<HashMap<String, Vec<PullRequest>>>,
    pub pr_commits: Mutex<HashMap<u64, Vec<RepoCommit>>>,
    pub pr_head_hash: Mutex<HashMap<u64, String>>,
    pub pr_reviews: Mutex<HashMap<u64, Vec<Review>>>,
}

impl Client for MockClient {
    fn new(_env_name: String, _api_endpoint: String) -> anyhow::Result<Arc<Self>> {
        Ok(Arc::new(Self {
            associated_prs: Mutex::new(HashMap::new()),
            pr_commits: Mutex::new(HashMap::new()),
            pr_head_hash: Mutex::new(HashMap::new()),
            pr_reviews: Mutex::new(HashMap::new()),
        }))
    }

    async fn associated_prs(&self, _owner: &str, _repo: &str, sha: String) -> anyhow::Result<Vec<PullRequest>> {
        Ok(self
            .associated_prs
            .lock()
            .unwrap()
            .get(&sha)
            .ok_or_else(|| anyhow!("MockClient associated_prs contains no {}", sha))?
            .clone())
    }

    async fn compare(
        &self,
        _owner: &str,
        _repo: &str,
        _original: &str,
        _base_commit: &str,
        _head_commit: &str,
    ) -> anyhow::Result<Vec<Commit>> {
        todo!()
    }

    async fn pr_head_hash(&self, _owner: &str, _repo: &str, pr_number: u64) -> anyhow::Result<String> {
        Ok(self
            .pr_head_hash
            .lock()
            .unwrap()
            .get(&pr_number)
            .ok_or_else(|| anyhow!("MockClient pr_head_hash contains no {}", pr_number))?
            .to_string())
    }

    async fn pr_commits(&self, _owner: &str, _repo: &str, pr_number: u64) -> anyhow::Result<Vec<RepoCommit>> {
        Ok(self
            .pr_commits
            .lock()
            .unwrap()
            .get(&pr_number)
            .ok_or_else(|| anyhow!("MockClient pr_commits contains no {}", pr_number))?
            .clone())
    }

    async fn pr_reviews(&self, _owner: &str, _repo: &str, pr_number: u64) -> anyhow::Result<Vec<Review>> {
        Ok(self
            .pr_reviews
            .lock()
            .unwrap()
            .get(&pr_number)
            .ok_or_else(|| anyhow!("MockClient pr_reviews contains no {}", pr_number))?
            .clone())
    }
}

pub struct ClientSet<C: Client> {
    clients: HashMap<String, Arc<C>>,
}

impl<C: Client> ClientSet<C> {
    pub fn new() -> Self {
        Self {
            clients: HashMap::new(),
        }
    }

    pub fn fill(&mut self, remote: &mut Remote<C>) -> Result<(), anyhow::Error> {
        let host = remote.host.to_string();
        let client = self.get_client(&host)?;
        remote.client = Some(client);
        Ok(())
    }

    fn get_client(&mut self, host: &str) -> Result<Arc<C>, anyhow::Error> {
        if let Some(client) = self.clients.get(host) {
            return Ok(client.clone());
        }

        let (env_name, api_endpoint) = get_env_name_api_endpoint_for_host(host);
        let client = C::new(env_name, api_endpoint)?;
        self.clients.insert(host.to_owned(), client.clone());

        Ok(client)
    }
}

fn get_env_name_api_endpoint_for_host(host: &str) -> (String, String) {
    let mut env_name = "GITHUB_TOKEN".to_string();
    let mut api_endpoint = "https://api.github.com".to_string();

    if host != "github.com" {
        api_endpoint = format!("https://{host}/api/v3");
        env_name = format!(
            "GITHUB_{}_TOKEN",
            host.replace('.', "_").to_uppercase().trim_start_matches("GITHUB_")
        );
    };

    (env_name, api_endpoint)
}

#[cfg(test)]
mod tests {
    use crate::api_clients;

    #[test]
    fn get_env_name_api_endpoint_for_host() {
        let (env_name, api_endpoint) = api_clients::get_env_name_api_endpoint_for_host("github.com");
        assert_eq!(env_name, "GITHUB_TOKEN");
        assert_eq!(api_endpoint, "https://api.github.com");

        let (env_name, api_endpoint) = api_clients::get_env_name_api_endpoint_for_host("github.example.com");
        assert_eq!(env_name, "GITHUB_EXAMPLE_COM_TOKEN");
        assert_eq!(api_endpoint, "https://github.example.com/api/v3");
    }
}
