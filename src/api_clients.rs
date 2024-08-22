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

use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use anyhow::Context;
use octocrab::Octocrab;
use tokio::sync::{AcquireError, Semaphore, SemaphorePermit};

use crate::remote::Remote;

#[derive(Debug)]
pub struct Client {
    semaphore: Semaphore,
    octocrab: Arc<Octocrab>,
}

impl Client {
    pub async fn lock(&self) -> Result<(SemaphorePermit<'_>, &Arc<Octocrab>), AcquireError> {
        let permit = self.semaphore.acquire().await?;
        Ok((permit, &self.octocrab))
    }
}

pub struct ClientSet {
    clients: HashMap<String, Arc<Client>>,
}

impl ClientSet {
    pub fn new() -> Self {
        ClientSet {
            clients: HashMap::new(),
        }
    }

    pub fn fill(&mut self, remote: &mut Remote) -> Result<(), anyhow::Error> {
        let host = remote.host.to_string();
        let client = self.get_client(&host)?;
        remote.client = Some(client);
        Ok(())
    }

    fn get_client(&mut self, host: &str) -> Result<Arc<Client>, anyhow::Error> {
        if let Some(client) = self.clients.get(host) {
            return Ok(client.clone());
        }

        let mut api_endpoint = "https://api.github.com".to_string();
        let mut env_name = "GITHUB_TOKEN".to_string();

        if host != "github.com" {
            api_endpoint = format!("https://{host}/api/v3");
            env_name = format!(
                "GITHUB_{}_TOKEN",
                host.replace('.', "_").to_uppercase().trim_start_matches("GITHUB_")
            );
        };

        octocrab::initialise(
            Octocrab::builder()
                .personal_token(env::var(&env_name).with_context(|| format!("missing {env_name} env"))?)
                .base_uri(&api_endpoint)
                .with_context(|| format!("failed to set base_uri to {api_endpoint}"))?
                .build()
                .context("failed to build octocrab client")?,
        );
        let client = Arc::new(Client {
            semaphore: Semaphore::new(5), // i.e. up to 5 API calls in parallel to the same GitHub instance
            octocrab: octocrab::instance(),
        });
        self.clients.insert(host.to_owned(), client.clone());
        Ok(client)
    }
}
