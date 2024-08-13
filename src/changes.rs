#[derive(Clone, Debug)]
pub struct RepoChange {
    pub name: String,
    pub remote: String,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub commits: Vec<ChangeCommit>,
    pub pr_link: Option<String>,
    pub approvals: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ChangeCommit {
    pub headline: String,
    pub link: String,
}
