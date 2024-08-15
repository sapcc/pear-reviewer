#[derive(Clone, Debug)]
pub struct RepoChangeset {
    pub name: String,
    pub remote: String,
    pub base_commit: String,
    pub head_commit: String,
    pub changes: Vec<Changeset>,
}

#[derive(Clone, Debug)]
pub struct Changeset {
    pub commits: Vec<CommitMetadata>,
    pub pr_link: Option<String>,
    pub approvals: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct CommitMetadata {
    pub headline: String,
    pub link: String,
}
