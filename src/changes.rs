use std::sync::Arc;

use anyhow::{anyhow, Context};
use octocrab::commits::PullRequestTarget;
use octocrab::models::commits::Commit;
use octocrab::models::pulls::Review;
use octocrab::models::pulls::ReviewState::Approved;
use octocrab::Octocrab;

use crate::util::Remote;

#[derive(Clone, Debug)]
pub struct RepoChangeset {
    pub name: String,
    pub remote: Remote,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Changeset>,
}

impl RepoChangeset {
    pub async fn analyze_commits(&mut self, octocrab: &Arc<Octocrab>) -> Result<(), anyhow::Error> {
        let compare = octocrab
            .commits(&self.remote.owner, &self.remote.repository)
            .compare(&self.base_commit, &self.head_commit)
            .send()
            .await
            .context(format!(
                "failed to compare {}/compare/{}...{}",
                self.remote.original.trim_end_matches(".git"),
                &self.base_commit,
                &self.head_commit
            ))?;

        for commit in &compare.commits {
            self.analyze_commit(octocrab, commit).await?;
        }

        Ok(())
    }

    async fn analyze_commit(&mut self, octocrab: &Arc<Octocrab>, commit: &Commit) -> Result<(), anyhow::Error> {
        let mut associated_prs_page = octocrab
            .commits(&self.remote.owner, &self.remote.repository)
            .associated_pull_requests(PullRequestTarget::Sha(commit.sha.clone()))
            .send()
            .await
            .context("failed to get associated prs")?;
        assert!(
            associated_prs_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        let associated_prs = associated_prs_page.take_items();

        let change_commit = CommitMetadata::new(commit);

        if associated_prs.is_empty() {
            self.changes.push(Changeset {
                commits: vec![change_commit],
                pr_link: None,
                approvals: Vec::new(),
            });
            return Ok(());
        }

        for associated_pr in &associated_prs {
            let mut pr_reviews_page = octocrab
                .pulls(&self.remote.owner, &self.remote.repository)
                .list_reviews(associated_pr.number)
                .send()
                .await
                .context("failed to get reviews")?;
            assert!(
                pr_reviews_page.next.is_none(),
                "found more than one page for associated_prs"
            );
            let mut pr_reviews = pr_reviews_page.take_items();
            pr_reviews.sort_by_key(|r| r.submitted_at);

            let associated_pr_link = Some(
                associated_pr
                    .html_url
                    .as_ref()
                    .ok_or(anyhow!("pr without an html link!?"))?
                    .to_string(),
            );

            let mut pr_commits_page = octocrab
              .pulls(&self.remote.owner, &self.remote.repository)
              .pr_commits(associated_pr.number)
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
            let head_sha = pr_commits.last().ok_or(anyhow!("PR contains no commits?"))?.sha.clone();

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

        Ok(())
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
        let mut last_review_by: Vec<&String> = vec![];

        // reverse the order of reviews to start with the oldest
        for pr_review in pr_reviews.iter().rev() {
            let Some(ref user) = pr_review.user else {
                continue;
            };

            // Only consider the last review of any user.
            // For example a user might have requested changes early on in the PR and later approved it
            // or requested additional changes after supplying an approval first.
            if last_review_by.contains(&&user.login) {
                continue;
            }
            last_review_by.push(&user.login);

            // Only account for reviews done on the last commit of the PR.
            // We could count the PR as partly reviewed but that is to complicated to present at the moment.
            if pr_review.commit_id != Some(head_sha.to_string()) {
              continue;
            }

            // in case it isn't approve, ignore it
            if pr_review.state != Some(Approved) {
                continue;
            }

            // don't duplicate user names
            if !self.approvals.contains(&user.login) {
                self.approvals.push(user.login.clone());
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
        CommitMetadata {
            headline,
            link: commit.html_url.clone(),
        }
    }
}
