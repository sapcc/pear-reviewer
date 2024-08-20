use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use anyhow::{anyhow, Context, Ok};
use octocrab::Octocrab;

use crate::util::Remote;

pub struct ClientSet {
    octocrab: HashMap<String, Arc<Octocrab>>,
}

impl ClientSet {
    pub fn new() -> Self {
        ClientSet {
            octocrab: HashMap::new(),
        }
    }

    pub fn add(&mut self, remote: &Remote) -> Result<(), anyhow::Error> {
        let mut api_endpoint = "https://api.github.com".to_string();
        let mut env_name = "GITHUB_TOKEN".to_string();
        if remote.host.to_string() != "github.com" {
            api_endpoint = format!("https://{}/api/v3", &remote.host);
            env_name = format!(
                "GITHUB_{}_TOKEN",
                &remote
                    .host
                    .to_string()
                    .replace('.', "_")
                    .to_uppercase()
                    .trim_start_matches("GITHUB_")
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
        self.octocrab.insert(remote.host.to_string(), octocrab::instance());

        Ok(())
    }

    pub fn get(&self, remote: &Remote) -> Result<&Arc<Octocrab>, anyhow::Error> {
        self.octocrab
            .get(&remote.host.to_string())
            .ok_or(anyhow!("no api client for {}", &remote.host))
    }
}
