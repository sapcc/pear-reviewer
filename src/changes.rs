use octocrab::models::pulls::Review;
use octocrab::models::pulls::ReviewState::Approved;

#[derive(Clone, Debug)]
pub struct RepoChangeset {
    pub name:        String,
    pub remote:      String,
    pub base_commit: String,
    pub head_commit: String,
    pub changes:     Vec<Changeset>,
}

#[derive(Clone, Debug)]
pub struct Changeset {
    pub commits:   Vec<CommitMetadata>,
    pub pr_link:   Option<String>,
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
    pub link:     String,
}
