use std::sync::Arc;

use crate::util::parse_remote;
use anyhow::{anyhow, Context};
use octocrab::commits::PullRequestTarget;
use octocrab::models::commits::Commit;
use octocrab::models::pulls::Review;
use octocrab::models::pulls::ReviewState::Approved;
use octocrab::Octocrab;

#[derive(Clone, Debug)]
pub struct RepoChangeset {
    pub name: String,
    pub remote: String,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Changeset>,
}

impl RepoChangeset {
    pub async fn analyze_commits(&mut self, octocrab: &Arc<Octocrab>) -> Result<(), anyhow::Error> {
        let (repo_owner, repo_name) = parse_remote(&self.remote).context("while parsing remote")?;

        let compare = octocrab
            .commits(repo_owner.clone(), repo_name.clone())
            .compare(&self.base_commit, &self.head_commit)
            .send()
            .await
            .context(format!(
                "failed to compare {}/compare/{}...{}",
                self.remote.trim_end_matches(".git"),
                &self.base_commit,
                &self.head_commit
            ))?;

        for commit in &compare.commits {
            self.analyze_commit(octocrab, commit).await?;
        }

        Ok(())
    }

    async fn analyze_commit(&mut self, octocrab: &Arc<Octocrab>, commit: &Commit) -> Result<(), anyhow::Error> {
        // TODO: it's not nice that we have to do this each time, this should be parsed once and
        // stored inside of `self.remote`
        let (repo_owner, repo_name) = parse_remote(&self.remote).context("while parsing remote")?;

        let mut associated_prs_page = octocrab
            .commits(repo_owner.clone(), repo_name.clone())
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
            println!("pr number: {:}", associated_pr.number);

            let mut pr_reviews_page = octocrab
                .pulls(repo_owner.clone(), repo_name.clone())
                .list_reviews(associated_pr.number)
                .send()
                .await
                .context("failed to get reviews")?;
            assert!(
                pr_reviews_page.next.is_none(),
                "found more than one page for associated_prs"
            );
            let pr_reviews = pr_reviews_page.take_items();

            let associated_pr_link = Some(
                associated_pr
                    .html_url
                    .as_ref()
                    .ok_or(anyhow!("pr without an html link!?"))?
                    .to_string(),
            );

            if let Some(changeset) = self.changes.iter_mut().find(|cs| cs.pr_link == associated_pr_link) {
                changeset.commits.push(change_commit.clone());
                changeset.collect_approved_reviews(&pr_reviews);
                continue;
            }

            let mut changeset = Changeset {
                commits: vec![change_commit.clone()],
                pr_link: associated_pr_link,
                approvals: Vec::new(),
            };

            changeset.collect_approved_reviews(&pr_reviews);
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
    pub fn collect_approved_reviews(&mut self, pr_reviews: &[Review]) {
        for pr_review in pr_reviews {
            // TODO: do we need to check if this is the last review of the user?
            if pr_review.state == Some(Approved) {
                let Some(ref user) = pr_review.user else {
                    continue;
                };

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
