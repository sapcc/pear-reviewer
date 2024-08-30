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

use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use url::Url;

use crate::api_clients::Client;
use crate::github::{Commit, PullRequest, Review};

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct Remote<C: Client> {
    pub host: url::Host,
    pub port: u16,
    pub owner: String,
    pub repository: String,
    pub original: String,
    pub client: Option<Arc<C>>,
}

impl<C: Client> Remote<C> {
    pub fn parse(url: &str) -> Result<Self, anyhow::Error> {
        let remote_url = Url::parse(url).context("can't parse remote")?;
        let path_elements: Vec<&str> = remote_url.path().trim_start_matches('/').split('/').collect();

        if path_elements.len() != 2 {
            bail!("remote URLs are expected to be in the format of https://domain.com/owner/repo.git");
        }

        Ok(Self {
            host: remote_url.host().context("remote has no host")?.to_owned(),
            port: remote_url.port_or_known_default().context("remote has no port")?,
            owner: path_elements[0].to_string(),
            repository: path_elements[1].trim_end_matches(".git").to_string(),
            original: url.into(),
            client: None,
        })
    }

    pub async fn associated_prs(&self, sha: String) -> anyhow::Result<Vec<PullRequest>> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow!("no client attached to remote"))?
            .associated_prs(&self.owner, &self.repository, sha)
            .await
    }

    pub async fn compare(&self, base_commit: &str, head_commit: &str) -> anyhow::Result<Vec<Commit>> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow!("no client attached to remote"))?
            .compare(
                &self.owner,
                &self.repository,
                &self.original,
                base_commit,
                head_commit,
            )
            .await
    }

    pub async fn pr_head_hash(&self,  pr_number: u64) -> Result<String, anyhow::Error> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow!("no client attached to remote"))?
            .pr_head_hash(&self.owner, &self.repository, pr_number)
            .await
    }

    pub async fn pr_reviews(&self, pr_number: u64) -> Result<Vec<Review>, anyhow::Error> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow!("no client attached to remote"))?
            .pr_reviews(&self.owner, &self.repository, pr_number)
            .await
    }
}

#[cfg(test)]
mod tests {
    use crate::api_clients::RealClient;

    use super::*;

    #[test]
    fn parse_remote() -> Result<(), anyhow::Error> {
        let remote = "https://github.com/sapcc/pear-reviewer.git";
        let result = Remote::<RealClient>::parse(remote)?;
        assert_eq!(result.host, url::Host::Domain("github.com"));
        assert_eq!(result.owner, "sapcc");
        assert_eq!(result.repository, "pear-reviewer");
        assert_eq!(result.original, remote);
        Ok(())
    }

    #[test]
    fn parse_remote_invalid() {
        let result = Remote::<RealClient>::parse("https://sapcc/pear-reviewer.git");
        match result {
            Err(err) => {
                assert_eq!(
                    err.to_string(),
                    "remote URLs are expected to be in the format of https://domain.com/owner/repo.git"
                );
            },
            Ok(_) => todo!(),
        }
    }
}
