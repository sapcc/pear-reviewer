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

#![warn(clippy::pedantic)]

mod api_clients;
mod changes;
mod github;
mod helm_config;
mod remote;
mod repo;

use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::sync::LazyLock;
use std::{env, str};

use anyhow::{anyhow, Context};
use api_clients::{ClientSet, RealClient};
use changes::RepoChangeset;
use clap::builder::styling::Style;
use clap::builder::NonEmptyStringValueParser;
use clap::{Parser, Subcommand};
use git2::{Oid, Repository};
use helm_config::ImageRefs;
use remote::Remote;
use tokio::task::JoinSet;
use url::{Host, Url};

const BOLD_UNDERLINE: Style = Style::new().bold().underline();
static GITHUB_TOKEN_HELP: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{BOLD_UNDERLINE}Environment variables:{BOLD_UNDERLINE:#}
  GITHUB_TOKEN                 GitHub token to use for API requests
"
    )
});

/// Program to simplify PCI double approval process across repositories
#[derive(Parser)]
#[command(version, about, long_about = None, after_help = GITHUB_TOKEN_HELP.to_string(), propagate_version = true)]
// see https://docs.github.com/en/actions/writing-workflows/choosing-what-your-workflow-does/variables for environment variablesuse
struct Cli {
    /// The git base ref to compare against
    #[arg(
        long,
        env = "GITHUB_BASE_REF",
        hide_env_values = true,
        required = false,
        value_parser = NonEmptyStringValueParser::new(),
        global = true
    )]
    base: String,

    /// The git head ref or source branch of the PR to compare against
    #[arg(
        long,
        default_value = "HEAD",
        env = "GITHUB_HEAD_REF",
        hide_env_values = true,
        required = false,
        value_parser = NonEmptyStringValueParser::new(),
        global = true
    )]
    head: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Analyzes commits in a repo and finds relevant reviews
    #[command(after_help = GITHUB_TOKEN_HELP.to_string())]
    Repo {
        /// GitHub git remote to use
        remote: String,
    },

    /// Analyzes a helm-charts repo, finds sources from values.yaml files and runs repo subcommand on them
    #[command(after_help = GITHUB_TOKEN_HELP.to_string())]
    HelmChart {
        /// Git repository where to discover images.yaml files
        #[arg(env = "GITHUB_WORKSPACE", hide_env_values = true, required = false, global = true)]
        workspace: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    let cli = Cli::parse();

    let mut api_clients = ClientSet::new();

    match &cli.command {
        Commands::Repo { remote } => {
            let mut remote = Remote::parse(remote)?;
            api_clients.fill(&mut remote)?;
            let repo = RepoChangeset {
                name: remote.repository.clone(),
                remote,
                base_commit: cli.base,
                head_commit: cli.head,
                changes: Vec::new(),
            };
            let repo = repo.analyze_commits().await.context("while finding reviews")?;
            print_changes(&[repo])?;
        },
        Commands::HelmChart { workspace } => {
            let changes =
                find_values_yaml(workspace.clone(), &cli.base, &cli.head).context("while finding values.yaml files")?;

            let mut join_set = JoinSet::new();
            for mut repo in changes {
                api_clients.fill(&mut repo.remote)?;
                join_set.spawn(repo.analyze_commits());
            }

            let mut changes = Vec::new();
            while let Some(res) = join_set.join_next().await {
                let repo_changeset = res?.context("while collecting repo changes")?;
                changes.push(repo_changeset);
            }

            print_changes(&changes)?;
        },
    }

    Ok(())
}

fn find_values_yaml(
    workspace: String,
    base: &str,
    head: &str,
) -> Result<Vec<RepoChangeset<RealClient>>, anyhow::Error> {
    let repo = Repository::open(workspace).context("failed to open repository")?;

    let base_tree = repo::tree_for_commit_ref(&repo, base).context("while parsing base")?;
    let head_tree = repo::tree_for_commit_ref(&repo, head).context("while parsing head")?;
    let diff_tree = repo
        .diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None)
        .with_context(|| format!("cannot diff trees {} and {}", base_tree.id(), head_tree.id()))?;

    let mut changes = Vec::<RepoChangeset<RealClient>>::new();

    for diff_delta in diff_tree.deltas() {
        let new_file = diff_delta.new_file();
        let path = new_file.path().ok_or_else(|| anyhow!("failed to get file path"))?;
        if !path.ends_with("images.yaml") {
            continue;
        }

        let new_image_refs = ImageRefs::parse(&repo, &new_file).context("while parsing new file")?;

        let old_file = diff_delta.old_file();
        let mut old_image_refs = ImageRefs {
            container_images: HashMap::new(),
        };
        // only zeros means the file was newly created and there is no old file to parse
        if old_file.id() != Oid::from_str("0000000000000000000000000000000000000000")? {
            old_image_refs = ImageRefs::parse(&repo, &old_file).context("while parsing old file")?;
        }

        for (name, image) in &new_image_refs.container_images {
            for new_source in &image.sources {
                // Is this a new container image?
                if !old_image_refs.container_images.contains_key(name) {
                    changes.push(RepoChangeset::new(
                        name.clone(),
                        remote::Remote::parse(&new_source.repo)?,
                        new_source.commit.clone(),
                        String::new(),
                    ));
                    continue;
                }

                for old_source in &old_image_refs.container_images[name].sources {
                    // Did we previously have this source?
                    if new_source.repo == old_source.repo {
                        changes.push(RepoChangeset::new(
                            name.clone(),
                            remote::Remote::parse(&new_source.repo)?,
                            new_source.commit.clone(),
                            old_source.commit.clone(),
                        ));
                    } else {
                        changes.push(RepoChangeset::new(
                            name.clone(),
                            remote::Remote::parse(&new_source.repo)?,
                            new_source.commit.clone(),
                            String::new(),
                        ));
                    }
                }
            }
        }
    }

    Ok(changes)
}

fn println_or_redirect(line: String) -> Result<(), anyhow::Error> {
    if env::var("GITHUB_ACTIONS").is_ok() {
        let path = env::var("GITHUB_OUTPUT").context("cannot find GITHUB_OUTPUT")?;
        let mut file = File::create(path.clone()).with_context(|| format!("cannot write to $GITHUB_OUTPUT {path}"))?;
        file.write_all((line + "\n").as_bytes())?;
    } else {
        println!("{line}");
    }

    Ok(())
}

fn print_changes(repo_changeset: &[RepoChangeset<RealClient>]) -> Result<(), anyhow::Error> {
    for change in repo_changeset {
        println_or_redirect(format!(
            "Name {} from {} moved from {} to {}",
            change.name, change.remote.original, change.base_commit, change.head_commit,
        ))?;
        println_or_redirect("| Commit link | Pull Request link | Approvals | Reviewer's verdict |".to_string())?;
        println_or_redirect("|-------------|-------------------|-----------|--------------------|".to_string())?;
        for commit_change in &change.changes {
            let mut commit_links: Vec<String> = vec![];
            for commit in &commit_change.commits {
                commit_links.push(format!(
                    "[{}]({})",
                    match commit.headline.char_indices().nth(45) {
                        None => commit.headline.clone(),
                        Some((idx, _)) => commit.headline[..idx].to_string() + "…",
                    },
                    prepend_redirect_to_domain(&commit.link)?
                ));
            }

            let pr_link = commit_change.pr_link.clone();
            println_or_redirect(format!(
                "| {} | {} | {} | <enter your decision> |",
                commit_links.join(" ,<br>"),
                match pr_link {
                    Some(link) => {
                        // PRs prefix number with pound
                        // https://github.com/sapcc/tenso/pull/187
                        // [tenso #187](https://github.com/sapcc/tenso/pull/187)
                        let split: Vec<&str> = link.split('/').collect();
                        if split[5] == "pull" {
                            format!("[{} #{}]({})", split[4], split[6], prepend_redirect_to_domain(&link)?)
                        } else {
                            link
                        }
                    },
                    None => String::new(),
                },
                commit_change.approvals.join(", "),
            ))?;
        }
    }

    Ok(())
}

fn prepend_redirect_to_domain(link: &str) -> Result<String, anyhow::Error> {
    let mut parsed_link = Url::parse(link).with_context(|| "failed to parse link {link}")?;
    if parsed_link.host() == Some(Host::Domain("github.com")) {
        parsed_link.set_host(Some("redirect.github.com"))?;
    }

    Ok(parsed_link.to_string())
}
