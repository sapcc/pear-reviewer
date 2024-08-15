use anyhow::Context;
use url::Url;

pub fn parse_remote(remote: &str) -> Result<(String, String), anyhow::Error> {
    let remote_url = Url::parse(remote).context("can't parse remote")?;
    let repo_parts: Vec<&str> = remote_url.path().trim_start_matches('/').split('/').collect();
    let repo_owner = repo_parts[0].to_string();
    let repo_name = repo_parts[1].trim_end_matches(".git").to_string();

    Ok((repo_owner, repo_name))
}
