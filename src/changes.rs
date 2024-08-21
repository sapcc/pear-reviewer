use anyhow::anyhow;
use octocrab::models::commits::Commit;
use octocrab::models::pulls::Review;
use octocrab::models::pulls::ReviewState::Approved;

use crate::api_clients::ClientSet;
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
    pub async fn analyze_commits(&mut self, client_set: &ClientSet) -> Result<(), anyhow::Error> {
        let compare = self
            .remote
            .compare(client_set, &self.base_commit, &self.head_commit)
            .await?;

        for commit in &compare.commits {
            self.analyze_commit(client_set, commit).await?;
        }

        Ok(())
    }

    async fn analyze_commit(&mut self, client_set: &ClientSet, commit: &Commit) -> Result<(), anyhow::Error> {
        let associated_prs = self.remote.associated_prs(client_set, commit).await?;

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
            let pr_reviews = self.remote.pr_reviews(client_set, associated_pr.number).await?;

            let associated_pr_link = Some(
                associated_pr
                    .html_url
                    .as_ref()
                    .ok_or(anyhow!("pr without an html link!?"))?
                    .to_string(),
            );

            let head_sha = self.remote.pr_head_hash(client_set, associated_pr.number).await?;

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
