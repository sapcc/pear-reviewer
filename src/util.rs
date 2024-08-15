use anyhow::Context;
use url::Url;

#[derive(Clone, Debug)]
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
}
