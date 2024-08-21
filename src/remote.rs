use anyhow::{anyhow, Context};
use octocrab::commits::PullRequestTarget;
use octocrab::models::commits::{Commit, CommitComparison};
use octocrab::models::pulls::{PullRequest, Review};
use octocrab::models::repos::RepoCommit;
use url::Url;

use crate::api_clients::ClientSet;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Remote {
    pub host: url::Host,
    pub port: u16,
    pub owner: String,
    pub repository: String,
    pub original: String,
}

impl Remote {
    pub fn parse(url: &str) -> Result<Self, anyhow::Error> {
        let remote_url = Url::parse(url).context("can't parse remote")?;
        let path_elements: Vec<&str> = remote_url.path().trim_start_matches('/').split('/').collect();
        Ok(Remote {
            host: remote_url.host().context("remote has no host")?.to_owned(),
            port: remote_url.port_or_known_default().context("remote has no port")?,
            owner: path_elements[0].to_string(),
            repository: path_elements[1].trim_end_matches(".git").to_string(),
            original: url.into(),
        })
    }

    pub async fn associated_prs(
        &self,
        client_set: &ClientSet,
        commit: &Commit,
    ) -> Result<Vec<PullRequest>, anyhow::Error> {
        let mut associated_prs_page = client_set
            .get(self)?
            .commits(&self.owner, &self.repository)
            .associated_pull_requests(PullRequestTarget::Sha(commit.clone().sha.clone()))
            .send()
            .await
            .context("failed to get associated prs")?;
        assert!(
            associated_prs_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        Ok(associated_prs_page.take_items())
    }

    pub async fn compare(
        &self,
        client_set: &ClientSet,
        base_commit: &str,
        head_commit: &str,
    ) -> Result<CommitComparison, anyhow::Error> {
        client_set
            .get(self)?
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

    pub async fn pr_head_hash(&self, client_set: &ClientSet, pr_number: u64) -> Result<String, anyhow::Error> {
        Ok(self
            .pr_commits(client_set, pr_number)
            .await
            .context("failed to get pr commits")?
            .last()
            .ok_or(anyhow!("PR contains no commits?"))?
            .sha
            .clone())
    }

    pub async fn pr_commits(&self, client_set: &ClientSet, pr_number: u64) -> Result<Vec<RepoCommit>, anyhow::Error> {
        let mut pr_commits_page = client_set
            .get(self)?
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

    pub async fn pr_reviews(&self, client_set: &ClientSet, pr_number: u64) -> Result<Vec<Review>, anyhow::Error> {
        let mut pr_reviews_page = client_set
            .get(self)?
            .pulls(&self.owner, &self.repository)
            .list_reviews(pr_number)
            .send()
            .await
            .context("failed to get reviews")?;
        assert!(
            pr_reviews_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        let mut pr_reviews = pr_reviews_page.take_items();
        pr_reviews.sort_by_key(|r| r.submitted_at);
        Ok(pr_reviews)
    }
}
