use std::path::PathBuf;

use base64ct::Encoding;
use clap::{Args, Subcommand};
use eyre::{Context, OptionExt};
use forgejo_api::Forgejo;

use crate::{
    repo::{RepoArg, RepoInfo, RepoName},
    SpecialRender,
};

#[derive(Args, Clone, Debug)]
pub struct WikiCommand {
    /// The local git remote that points to the repo to operate on
    #[clap(long, short = 'R', global = true)]
    remote: Option<String>,

    /// The repo to operate on
    #[clap(long, short, global = true)]
    repo: Option<RepoArg>,

    #[clap(subcommand)]
    command: WikiSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum WikiSubcommand {
    Contents,
    View {
        page: String,
    },
    Clone {
        #[clap(long, short)]
        path: Option<PathBuf>,
    },
    Browse {
        page: String,
    },
}

impl WikiCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        use WikiSubcommand::*;

        let repo =
            RepoInfo::get_current(host_name, self.repo.as_ref(), self.remote.as_deref(), &keys)?;
        let api = keys.get_api(repo.host_url()).await?;
        let repo = repo
            .name()
            .ok_or_else(|| eyre::eyre!("couldn't guess repo"))?;

        match self.command {
            Contents => wiki_contents(repo, &api).await?,
            View { page } => view_wiki_page(repo, &api, &page).await?,
            Clone { path } => clone_wiki(repo, &api, path).await?,
            Browse { page } => browse_wiki_page(repo, &api, &page).await?,
        }
        Ok(())
    }
}

async fn wiki_contents(repo: &RepoName, api: &Forgejo) -> eyre::Result<()> {
    let SpecialRender { bullet, .. } = *crate::special_render();

    let query = forgejo_api::structs::RepoGetWikiPagesQuery {
        page: None,
        limit: None,
    };
    let (_, pages) = api
        .repo_get_wiki_pages(repo.owner(), repo.name(), query)
        .await?;
    for page in pages {
        let title = page
            .title
            .as_deref()
            .ok_or_eyre("page does not have title")?;
        println!("{bullet} {title}");
    }

    Ok(())
}

async fn view_wiki_page(repo: &RepoName, api: &Forgejo, page: &str) -> eyre::Result<()> {
    let SpecialRender { bold, reset, .. } = *crate::special_render();

    let page = api
        .repo_get_wiki_page(repo.owner(), repo.name(), page)
        .await?;

    let title = page
        .title
        .as_deref()
        .ok_or_eyre("page does not have title")?;
    println!("{bold}{title}{reset}");
    println!();

    let contents_b64 = page
        .content_base64
        .as_deref()
        .ok_or_eyre("page does not have content")?;
    let contents = String::from_utf8(base64ct::Base64::decode_vec(contents_b64)?)
        .wrap_err("page content is not utf-8")?;

    println!("{}", crate::markdown(&contents));
    Ok(())
}

async fn browse_wiki_page(repo: &RepoName, api: &Forgejo, page: &str) -> eyre::Result<()> {
    let page = api
        .repo_get_wiki_page(repo.owner(), repo.name(), page)
        .await?;
    let html_url = page
        .html_url
        .as_ref()
        .ok_or_eyre("page does not have html url")?;
    open::that_detached(html_url.as_str()).wrap_err("Failed to open URL")?;
    Ok(())
}

async fn clone_wiki(repo: &RepoName, api: &Forgejo, path: Option<PathBuf>) -> eyre::Result<()> {
    let repo_data = api.repo_get(repo.owner(), repo.name()).await?;
    let clone_url = repo_data
        .clone_url
        .as_ref()
        .ok_or_eyre("repo does not have clone url")?;
    let git_stripped = clone_url
        .as_str()
        .strip_suffix(".git")
        .unwrap_or(clone_url.as_str());
    let clone_url = url::Url::parse(&format!("{}.wiki.git", git_stripped))?;

    let repo_name = repo_data
        .name
        .as_deref()
        .ok_or_eyre("repo does not have name")?;
    let repo_full_name = repo_data
        .full_name
        .as_deref()
        .ok_or_eyre("repo does not have full name")?;
    let name = format!("{}'s wiki", repo_full_name);

    let path = path.unwrap_or_else(|| PathBuf::from(format!("./{repo_name}-wiki")));

    crate::repo::clone_repo(&name, &clone_url, &path)?;

    Ok(())
}
