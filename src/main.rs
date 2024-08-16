#![warn(clippy::pedantic)]

mod changes;
mod helm_config;
mod repo;
mod util;

use std::sync::LazyLock;
use std::{env, str};

use anyhow::{anyhow, Context};
use changes::RepoChangeset;
use clap::builder::styling::Style;
use clap::{Parser, Subcommand};
use git2::Repository;
use helm_config::ImageRefs;
use octocrab::Octocrab;

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

    octocrab::initialise(
        Octocrab::builder()
            .personal_token(env::var("GITHUB_TOKEN").context("missing GITHUB_TOKEN env")?)
            .build()
            .context("failed to build octocrab client")?,
    );
    let octocrab = octocrab::instance();

    match &cli.command {
        Commands::Repo { remote } => {
            let remote = util::Remote::parse(remote)?;
            let repo = &mut RepoChangeset {
                name: remote.repository.clone(),
                remote,
                base_commit: cli.base,
                head_commit: cli.head,
                changes: Vec::new(),
            };
            repo.analyze_commits(&octocrab).await.context("while finding reviews")?;
            print_changes(&[repo.clone()]);
        },
        Commands::HelmChart { workspace } => {
            let mut changes =
                find_values_yaml(workspace.clone(), &cli.base, &cli.head).context("while finding values.yaml files")?;

            for repo in &mut changes {
                repo.analyze_commits(&octocrab)
                    .await
                    .context("while collecting repo changes")?;
            }

            print_changes(&changes);
        },
    }

    Ok(())
}

fn find_values_yaml(workspace: String, base: &str, head: &str) -> Result<Vec<RepoChangeset>, anyhow::Error> {
    let repo = Repository::open(workspace).context("failed to open repository")?;

    let base_tree = repo::tree_for_commit_ref(&repo, base)?;
    let head_tree = repo::tree_for_commit_ref(&repo, head)?;
    let diff_tree = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&base_tree), None)
        .with_context(|| format!("cannot diff trees {} and {}", base_tree.id(), head_tree.id()))?;

    let mut changes = Vec::<RepoChangeset>::new();

    for diff_delta in diff_tree.deltas() {
        let new_file = diff_delta.new_file();
        if !new_file.exists() {
            continue;
        }

        let path = new_file.path().ok_or(anyhow!("failed to get file path"))?;
        if !path.ends_with("images.yaml") {
            continue;
        }

        let new_image_refs = ImageRefs::parse(&repo, &new_file)?;

        let old_file = diff_delta.old_file();
        if old_file.exists() {
            let old_image_refs = ImageRefs::parse(&repo, &old_file)?;

            for (name, image) in &new_image_refs.container_images {
                for source in &image.sources {
                    changes.push(RepoChangeset {
                        name: name.clone(),
                        remote: util::Remote::parse(&source.repo)?,
                        // TODO: iterate over sources
                        base_commit: old_image_refs.container_images[name].sources[0].commit.clone(),
                        head_commit: source.commit.clone(),
                        changes: Vec::new(),
                    });
                }
            }
        }
    }

    Ok(changes)
}

fn print_changes(changes: &[RepoChangeset]) {
    for change in changes {
        println!(
            "Name {} from {} moved from {} to {}",
            change.name, change.remote.original, change.base_commit, change.head_commit
        );
        println!("| Commit link | Pull Request link | Approvals | Reviewer's verdict |");
        println!("|-------------|-------------------|-----------|--------------------|");
        for commit_change in &change.changes {
            let pr_link = commit_change.pr_link.clone();
            println!(
                "| {} | {} | {} | <enter your decision> |",
                commit_change
                    .commits
                    .iter()
                    .map(|x| format!(
                        "[{}]({})",
                        match x.headline.char_indices().nth(45) {
                            None => x.headline.clone(),
                            Some((idx, _)) => x.headline[..idx].to_string() + "…",
                        },
                        x.link
                    ))
                    .collect::<Vec<String>>()
                    .join(" ,<br>"),
                match pr_link {
                    Some(link) => {
                        // PRs prefix number with pound
                        // https://github.com/sapcc/tenso/pull/187
                        // [tenso #187](https://github.com/sapcc/tenso/pull/187)
                        let split: Vec<&str> = link.split('/').collect();

                        if split[5] == "pull" {
                            format!("[{} #{}]({})", split[4], split[6], link)
                        } else {
                            link
                        }
                    },
                    None => String::new(),
                },
                commit_change.approvals.join("None"),
            );
        }
    }
}
