#![warn(clippy::pedantic)]

mod changes;
mod images;

use std::sync::Arc;
use std::{env, str};

use anyhow::{anyhow, Context};
use changes::{Change, ChangeCommit, RepoChange};
use clap::builder::styling::Style;
use clap::{Parser, Subcommand};
use git2::Repository;
use images::ContainerImages;
use lazy_static::lazy_static;
use octocrab::commits::PullRequestTarget;
use octocrab::models::pulls;
use octocrab::models::pulls::ReviewState;
use octocrab::Octocrab;

const BOLD_UNDERLINE: Style = Style::new().bold().underline();
lazy_static! {
    static ref GITHUB_TOKEN_HELP: String = format!(
        "{BOLD_UNDERLINE}Environment variables:{BOLD_UNDERLINE:#}
  GITHUB_TOKEN                 GitHub token to use for API requests
"
    );
}

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
            let repo = &mut RepoChange {
                name: parse_remote(remote).context("while parsing remote")?.1,
                remote: remote.clone(),
                base_commit: cli.base,
                head_commit: cli.head,
                changes: Vec::new(),
            };
            find_reviews(&octocrab, repo).await.context("while finding reviews")?;
            print_changes(&[repo.clone()]);
        },
        Commands::HelmChart { workspace } => {
            let mut changes = find_values_yaml(workspace.clone(), &cli.base, &cli.head)
                .context("while finding values.yaml files")?;

            for repo in &mut changes {
                find_reviews(&octocrab, repo)
                    .await
                    .context("while collecting repo changes")?;
            }

            print_changes(&changes);
        },
    }

    Ok(())
}

fn parse_remote(remote: &str) -> Result<(String, String), anyhow::Error> {
    let repo_parts: Vec<&str> = remote
        .strip_prefix("https://github.com/")
        .ok_or(anyhow!("can't strip https://github.com/ prefix"))?
        .split('/')
        .collect();
    let repo_owner = repo_parts[0].to_string();
    let repo_name = repo_parts[1]
        .strip_suffix(".git")
        .ok_or(anyhow!("can't strip .git suffix"))?
        .to_string();

    Ok((repo_owner, repo_name))
}

fn find_values_yaml(workspace: String, base: &str, head: &str) -> Result<Vec<RepoChange>, anyhow::Error> {
    let repo = Repository::open(workspace).context("failed to open repository")?;

    let base_ref = repo.revparse_single(base).context("can't parse base_ref")?.id();

    let head_ref = repo.revparse_single(head).context("can't parse head_ref")?.id();

    // compare base and head ref against each other and generate the diff GitHub shows in the files tab in the PR
    let base_tree = repo
        .find_commit(base_ref)
        .context("can't find commit for base_ref")?
        .tree()
        .context("can't get tree for base_ref")?;
    let head_tree = repo
        .find_commit(head_ref)
        .context("can't find commit for head_ref")?
        .tree()
        .context("can't get tree for head_ref")?;
    let diff_tree = repo
        .diff_tree_to_tree(Some(&head_tree), Some(&base_tree), None)
        .context("can't diff trees")?;

    let mut changes = Vec::<RepoChange>::new();

    for diff_delta in diff_tree.deltas() {
        let new_file = diff_delta.new_file();
        if !new_file.exists() {
            continue;
        }

        let path = new_file.path().ok_or(anyhow!("failed to get file path"))?;
        if !path.ends_with("images.yaml") {
            continue;
        }

        let new_images_content = repo.find_blob(new_file.id())?;
        let new_image_config: ContainerImages = serde_yml::from_slice(new_images_content.content())?;

        let old_file = diff_delta.old_file();
        if old_file.exists() {
            let old_images_content = repo.find_blob(old_file.id())?;
            let old_image_config: ContainerImages = serde_yml::from_slice(old_images_content.content())?;

            for (name, image) in &new_image_config.container_images {
                for source in &image.sources {
                    changes.push(RepoChange {
                        name: name.clone(),
                        remote: source.repo.clone(),
                        base_commit: old_image_config.container_images[name].sources[0].commit.clone(),
                        head_commit: source.commit.clone(),
                        changes: Vec::new(),
                    });
                }
            }

            continue;
        }
    }

    Ok(changes)
}

async fn find_reviews(octocrab: &Arc<Octocrab>, repo: &mut RepoChange) -> Result<(), anyhow::Error> {
    let (repo_owner, repo_name) = parse_remote(&repo.remote).context("while parsing remote")?;

    let link_prefix = format!("https://github.com/{repo_owner}/{repo_name}");

    let compare = octocrab
        .commits(repo_owner.clone(), repo_name.clone())
        .compare(&repo.base_commit, &repo.head_commit)
        .send()
        .await
        .context(format!(
            "failed to compare {link_prefix}/compare/{}...{}",
            &repo.base_commit, &repo.head_commit
        ))?;

    for commit in &compare.commits {
        let mut associated_prs_page = octocrab
            .commits(repo_owner.clone(), repo_name.clone())
            .associated_pull_requests(PullRequestTarget::Sha(commit.sha.clone()))
            .send()
            .await
            .context("failed to get associated prs")?;
        assert!(
            associated_prs_page.next.is_none(),
            "found more than one page for associated_prs"
        );
        let associated_prs = associated_prs_page.take_items();

        let change_commit = ChangeCommit {
            headline: commit.commit.message.split('\n').collect::<Vec<&str>>()[0].to_string(),
            link: commit.html_url.clone(),
        };

        if associated_prs.is_empty() {
            repo.changes.push(Change {
                commits: vec![change_commit],
                pr_link: None,
                approvals: Vec::new(),
            });
            continue;
        }

        for associated_pr in &associated_prs {
            println!("pr number: {:}", associated_pr.number);

            let mut pr_reviews_page = octocrab
                .pulls(repo_owner.clone(), repo_name.clone())
                .list_reviews(associated_pr.number)
                .send()
                .await
                .context("failed to get reviews")?;
            assert!(
                pr_reviews_page.next.is_none(),
                "found more than one page for associated_prs"
            );
            let pr_reviews = pr_reviews_page.take_items();

            let mut found_existing_change = false;
            let associated_pr_link = associated_pr
                .html_url
                .as_ref()
                .ok_or(anyhow!("pr without an html link!?"))?
                .to_string();

            for review in &mut repo.changes {
                if review.pr_link.as_ref() == Some(&associated_pr_link) {
                    found_existing_change = true;
                    review.commits.push(change_commit.clone());
                    collect_approved_reviews(&pr_reviews, review)?;
                }
            }

            if found_existing_change {
                continue;
            }

            let mut review = Change {
                commits: vec![change_commit.clone()],
                pr_link: Some(
                    associated_pr
                        .html_url
                        .as_ref()
                        .ok_or(anyhow!("pr without an html link!?"))?
                        .to_string(),
                ),
                approvals: Vec::new(),
            };

            collect_approved_reviews(&pr_reviews, &mut review)?;
            repo.changes.push(review);
        }
    }

    Ok(())
}

fn print_changes(changes: &[RepoChange]) {
    for change in changes {
        println!(
            "Name {} from {} moved from {} to {}",
            change.name, change.remote, change.base_commit, change.head_commit
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
                    Some(pr_link) => short_md_link(pr_link),
                    None => String::new(),
                },
                commit_change.approvals.join("None"),
            );
        }
    }
}

fn collect_approved_reviews(pr_reviews: &[pulls::Review], review: &mut Change) -> Result<(), anyhow::Error> {
    for pr_review in pr_reviews {
        // TODO: do we need to check if this is the last review of the user?
        if pr_review.state.ok_or(anyhow!("review has no state"))? == ReviewState::Approved {
            review.approvals.push(
                pr_review
                    .user
                    .as_ref()
                    .ok_or(anyhow!("review without an user!?"))?
                    .login
                    .clone(),
            );
        }
    }

    Ok(())
}

// for commits take the first 6 chars
// https://github.com/sapcc/tenso/commit/39241382cc5de6ab54eb34f6ac09dfb740cbee701
// -> [tenso 392413](https://github.com/sapcc/tenso/commit/39241382cc5de6ab54eb34f6ac09dfb740cbee701)
//
// or for PRs prefix number with pound
// https://github.com/sapcc/tenso/pull/187
// [tenso #187](https://github.com/sapcc/tenso/pull/187)
fn short_md_link(link: String) -> String {
    let split: Vec<&str> = link.split('/').collect();

    // TODO: drop commit path
    if split[5] == "commit" {
        return format!("[{} {}]({})", split[4], &split[6][..8], link);
    } else if split[5] == "pull" {
        return format!("[{} #{}]({})", split[4], split[6], link);
    }

    link
}
