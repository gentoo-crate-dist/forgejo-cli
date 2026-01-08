use clap::{Args, Subcommand};
use eyre::{bail, eyre, Context, OptionExt};
use forgejo_api::{
    structs::{RepoCreateReleaseAttachmentQuery, RepoListReleasesQuery},
    Forgejo,
};
use futures::stream::TryStreamExt;
use tokio::io::AsyncWriteExt;

use crate::{
    keys::KeyInfo,
    repo::{RepoArg, RepoInfo, RepoName},
    SpecialRender,
};

#[derive(Args, Clone, Debug)]
pub struct ReleaseCommand {
    /// The local git remote that points to the repo to operate on
    #[clap(long, short = 'R', global = true)]
    remote: Option<String>,
    /// The name of the repository to operate on
    #[clap(long, short, global = true)]
    repo: Option<RepoArg>,
    #[clap(subcommand)]
    command: ReleaseSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ReleaseSubcommand {
    /// Create a new release
    Create {
        name: String,
        #[clap(long, short = 'T')]
        /// Create a new cooresponding tag for this release. Defaults to release's name.
        create_tag: Option<Option<String>>,
        #[clap(long, short = 't')]
        /// Pre-existing tag to use
        ///
        /// If you need to create a new tag for this release, use `--create-tag`
        tag: Option<String>,
        #[clap(
            long,
            short,
            help = "Include a file as an attachment",
            long_help = "Include a file as an attachment
        
`--attach=<FILE>` will set the attachment's name to the file name
`--attach=<FILE>:<ASSET>` will use the provided name for the attachment"
        )]
        attach: Vec<String>,
        #[clap(long, short)]
        /// Text of the release body.
        ///
        /// Using this flag without an argument will open your editor.
        body: Option<Option<String>>,
        #[clap(long, short = 'B')]
        branch: Option<String>,
        #[clap(long, short)]
        draft: bool,
        #[clap(long, short)]
        prerelease: bool,
    },
    /// Edit a release's info
    Edit {
        name: String,
        #[clap(long, short = 'n')]
        rename: Option<String>,
        #[clap(long, short = 't')]
        /// Corresponding tag for this release.
        tag: Option<String>,
        #[clap(long, short)]
        /// Text of the release body.
        ///
        /// Using this flag without an argument will open your editor.
        body: Option<Option<String>>,
        #[clap(long, short)]
        draft: Option<bool>,
        #[clap(long, short)]
        prerelease: Option<bool>,
    },
    /// Delete a release
    Delete {
        name: String,
        #[clap(long, short = 't')]
        by_tag: bool,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be deleted without actually deleting
        #[clap(long)]
        dry_run: bool,
    },
    /// List all the releases on a repo
    List {
        #[clap(long, short = 'p')]
        include_prerelease: bool,
        #[clap(long, short = 'd')]
        include_draft: bool,
    },
    /// View a release's info
    View {
        name: String,
        #[clap(long, short = 't')]
        by_tag: bool,
    },
    /// Open a release in your browser
    Browse { name: Option<String> },
    /// Commands on a release's attached files
    #[clap(subcommand)]
    Asset(AssetCommand),
}

#[derive(Subcommand, Clone, Debug)]
pub enum AssetCommand {
    /// Create a new attachment on a release
    Create {
        release: String,
        path: std::path::PathBuf,
        name: Option<String>,
    },
    /// Remove an attachment from a release
    Delete {
        release: String,
        asset: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be deleted without actually deleting
        #[clap(long)]
        dry_run: bool,
    },
    /// Download an attached file
    ///
    /// Use `source.zip` or `source.tar.gz` to download the repo archive
    Download {
        release: String,
        asset: String,
        #[clap(long, short)]
        output: Option<std::path::PathBuf>,
    },
}

impl ReleaseCommand {
    pub async fn run(self, keys: &mut KeyInfo, remote_name: Option<&str>) -> eyre::Result<()> {
        let repo = RepoInfo::get_current(
            remote_name,
            self.repo.as_ref(),
            self.remote.as_deref(),
            &keys,
        )?;
        let api = keys.get_api(repo.host_url()).await?;
        let repo = repo
            .name()
            .ok_or_eyre("couldn't get repo name, try specifying with --repo")?;
        match self.command {
            ReleaseSubcommand::Create {
                name,
                create_tag,
                tag,
                attach,
                body,
                branch,
                draft,
                prerelease,
            } => {
                create_release(
                    repo, &api, name, create_tag, tag, attach, body, branch, draft, prerelease,
                )
                .await?
            }
            ReleaseSubcommand::Edit {
                name,
                rename,
                tag,
                body,
                draft,
                prerelease,
            } => edit_release(repo, &api, name, rename, tag, body, draft, prerelease).await?,
            ReleaseSubcommand::Delete { name, by_tag, force, dry_run } => {
                delete_release(repo, &api, name, by_tag, force, dry_run).await?
            }
            ReleaseSubcommand::List {
                include_prerelease,
                include_draft,
            } => list_releases(repo, &api, include_prerelease, include_draft).await?,
            ReleaseSubcommand::View { name, by_tag } => {
                view_release(repo, &api, name, by_tag).await?
            }
            ReleaseSubcommand::Browse { name } => browse_release(repo, &api, name).await?,
            ReleaseSubcommand::Asset(subcommand) => match subcommand {
                AssetCommand::Create {
                    release,
                    path,
                    name,
                } => create_asset(repo, &api, release, path, name).await?,
                AssetCommand::Delete { release, asset, force, dry_run } => {
                    delete_asset(repo, &api, release, asset, force, dry_run).await?
                }
                AssetCommand::Download {
                    release,
                    asset,
                    output,
                } => download_asset(repo, &api, release, asset, output).await?,
            },
        }
        Ok(())
    }
}

async fn create_release(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    create_tag: Option<Option<String>>,
    tag: Option<String>,
    attachments: Vec<String>,
    body: Option<Option<String>>,
    branch: Option<String>,
    draft: bool,
    prerelease: bool,
) -> eyre::Result<()> {
    let tag_name = match (tag, create_tag) {
        (None, None) => bail!("must select tag with `--tag` or `--create-tag`"),
        (Some(tag), None) => tag,
        (None, Some(tag)) => {
            let tag = tag.unwrap_or_else(|| name.clone());
            let opt = forgejo_api::structs::CreateTagOption {
                message: None,
                tag_name: tag.clone(),
                target: branch,
            };
            api.repo_create_tag(repo.owner(), repo.name(), opt).await?;
            tag
        }
        (Some(_), Some(_)) => {
            bail!("`--tag` and `--create-tag` are mutually exclusive; please pick just one")
        }
    };

    let body = match body {
        Some(Some(body)) => Some(body),
        Some(None) => {
            let mut s = String::new();
            crate::editor(&mut s, Some("md")).await?;
            Some(s)
        }
        None => None,
    };

    let release_opt = forgejo_api::structs::CreateReleaseOption {
        hide_archive_links: None,
        body,
        draft: Some(draft),
        name: Some(name.clone()),
        prerelease: Some(prerelease),
        tag_name,
        target_commitish: None,
    };
    let release = api
        .repo_create_release(repo.owner(), repo.name(), release_opt)
        .await?;

    for attachment in attachments {
        let (file, asset) = match attachment.split_once(':') {
            Some((file, asset)) => (std::path::Path::new(file), asset),
            None => {
                let file = std::path::Path::new(&attachment);
                let asset = file
                    .file_name()
                    .ok_or_else(|| eyre!("{attachment} does not have a file name"))?
                    .to_str()
                    .unwrap();
                (file, asset)
            }
        };
        let query = RepoCreateReleaseAttachmentQuery {
            name: Some(asset.into()),
        };
        let id = release
            .id
            .ok_or_else(|| eyre::eyre!("release does not have id"))?;
        api.repo_create_release_attachment(
            repo.owner(),
            repo.name(),
            id,
            Some(&tokio::fs::read(file).await?),
            None,
            query,
        )
        .await?;
    }

    println!("Created release {name}");

    Ok(())
}

async fn edit_release(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    rename: Option<String>,
    tag: Option<String>,
    body: Option<Option<String>>,
    draft: Option<bool>,
    prerelease: Option<bool>,
) -> eyre::Result<()> {
    let release = find_release(repo, api, &name).await?;
    let body = match body {
        Some(Some(body)) => Some(body),
        Some(None) => {
            let mut s = release
                .body
                .clone()
                .ok_or_else(|| eyre::eyre!("release does not have body"))?;
            crate::editor(&mut s, Some("md")).await?;
            Some(s)
        }
        None => None,
    };
    let release_edit = forgejo_api::structs::EditReleaseOption {
        hide_archive_links: None,
        name: rename,
        tag_name: tag,
        body,
        draft,
        prerelease,
        target_commitish: None,
    };
    let id = release
        .id
        .ok_or_else(|| eyre::eyre!("release does not have id"))?;
    api.repo_edit_release(repo.owner(), repo.name(), id, release_edit)
        .await?;
    Ok(())
}

async fn list_releases(
    repo: &RepoName,
    api: &Forgejo,
    prerelease: bool,
    draft: bool,
) -> eyre::Result<()> {
    let query = forgejo_api::structs::RepoListReleasesQuery {
        q: None,
        pre_release: Some(prerelease),
        draft: Some(draft),
    };
    let (_, releases) = api
        .repo_list_releases(repo.owner(), repo.name(), query)
        .await?;
    for release in releases {
        let name = release
            .name
            .as_ref()
            .ok_or_else(|| eyre::eyre!("release does not have name"))?;
        let draft = release
            .draft
            .as_ref()
            .ok_or_else(|| eyre::eyre!("release does not have draft"))?;
        let prerelease = release
            .prerelease
            .as_ref()
            .ok_or_else(|| eyre::eyre!("release does not have prerelease"))?;
        print!("{}", name);
        match (draft, prerelease) {
            (false, false) => (),
            (true, false) => print!(" (draft)"),
            (false, true) => print!(" (prerelease)"),
            (true, true) => print!(" (draft, prerelease)"),
        }
        println!();
    }
    Ok(())
}

async fn view_release(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    by_tag: bool,
) -> eyre::Result<()> {
    let release = if by_tag {
        api.repo_get_release_by_tag(repo.owner(), repo.name(), &name)
            .await?
    } else {
        find_release(repo, api, &name).await?
    };
    let name = release
        .name
        .as_ref()
        .ok_or_else(|| eyre::eyre!("release does not have name"))?;
    let author = release
        .author
        .as_ref()
        .ok_or_else(|| eyre::eyre!("release does not have author"))?;
    let login = author
        .login
        .as_ref()
        .ok_or_else(|| eyre::eyre!("autho does not have login"))?;
    let created_at = release
        .created_at
        .ok_or_else(|| eyre::eyre!("release does not have created_at"))?;
    println!("{}", name);
    print!("By {} on ", login);
    created_at.format_into(
        &mut std::io::stdout(),
        &time::format_description::well_known::Rfc2822,
    )?;
    println!();
    let SpecialRender { bullet, .. } = crate::special_render();
    let body = release
        .body
        .as_ref()
        .ok_or_else(|| eyre::eyre!("release does not have body"))?;
    if !body.is_empty() {
        println!();
        println!("{}", crate::markdown(body));
        println!();
    }
    let assets = release
        .assets
        .as_ref()
        .ok_or_else(|| eyre::eyre!("release does not have assets"))?;
    if !assets.is_empty() {
        println!("{} assets", assets.len() + 2);
        for asset in assets {
            let name = asset
                .name
                .as_ref()
                .ok_or_else(|| eyre::eyre!("asset does not have name"))?;
            println!("{bullet} {}", name);
        }
        println!("{bullet} source.zip");
        println!("{bullet} source.tar.gz");
    }
    Ok(())
}

async fn browse_release(repo: &RepoName, api: &Forgejo, name: Option<String>) -> eyre::Result<()> {
    match name {
        Some(name) => {
            let release = find_release(repo, api, &name).await?;
            let html_url = release
                .html_url
                .as_ref()
                .ok_or_else(|| eyre::eyre!("release does not have html_url"))?;
            open::that_detached(html_url.as_str()).wrap_err("Failed to open URL")?;
        }
        None => {
            let repo_data = api.repo_get(repo.owner(), repo.name()).await?;
            let mut html_url = repo_data
                .html_url
                .clone()
                .ok_or_else(|| eyre::eyre!("repository does not have html_url"))?;
            html_url.path_segments_mut().unwrap().push("releases");
            open::that_detached(html_url.as_str()).wrap_err("Failed to open URL")?;
        }
    }
    Ok(())
}

async fn create_asset(
    repo: &RepoName,
    api: &Forgejo,
    release: String,
    file: std::path::PathBuf,
    asset: Option<String>,
) -> eyre::Result<()> {
    let (file, asset) = match asset {
        Some(ref asset) => (&*file, &**asset),
        None => {
            let asset = file
                .file_name()
                .ok_or_else(|| eyre!("{} does not have a file name", file.display()))?
                .to_str()
                .unwrap();
            (&*file, asset)
        }
    };
    let id = find_release(repo, api, &release)
        .await?
        .id
        .ok_or_else(|| eyre::eyre!("release does not have id"))?;
    let query = RepoCreateReleaseAttachmentQuery {
        name: Some(asset.to_owned()),
    };
    api.repo_create_release_attachment(
        repo.owner(),
        repo.name(),
        id,
        Some(&tokio::fs::read(file).await?),
        None,
        query,
    )
    .await?;

    println!("Added attachment `{}` to {}", asset, release);

    Ok(())
}

async fn delete_asset(
    repo: &RepoName,
    api: &Forgejo,
    release_name: String,
    asset_name: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if dry_run {
        println!("Would delete asset `{}` from release {}", asset_name, release_name);
        println!("\nRun with --force to actually delete.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Delete asset `{}` from release {}?", asset_name, release_name);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    let release = find_release(repo, api, &release_name).await?;
    let assets = release
        .assets
        .as_ref()
        .ok_or_else(|| eyre::eyre!("release does not have assets"))?;
    let asset = assets
        .iter()
        .find(|a| a.name.as_ref() == Some(&asset_name))
        .ok_or_else(|| eyre!("asset not found"))?;
    let release_id = release
        .id
        .ok_or_else(|| eyre::eyre!("release does not have id"))?;
    let asset_id = asset
        .id
        .ok_or_else(|| eyre::eyre!("asset does not have id"))?;

    crate::verbose_log!("Deleting asset {} from release {}", asset_name, release_name);
    api.repo_delete_release_attachment(repo.owner(), repo.name(), release_id, asset_id)
        .await?;
    println!("Removed attachment `{}` from {}", asset_name, release_name);
    Ok(())
}

async fn download_asset(
    repo: &RepoName,
    api: &Forgejo,
    release: String,
    asset: String,
    output: Option<std::path::PathBuf>,
) -> eyre::Result<()> {
    let release = find_release(repo, api, &release).await?;
    let file = match &*asset {
        "source.zip" => {
            let tag_name = release
                .tag_name
                .as_ref()
                .ok_or_else(|| eyre::eyre!("release does not have tag_name"))?;
            api.repo_get_archive(repo.owner(), repo.name(), &format!("{}.zip", tag_name))
                .await?
        }
        "source.tar.gz" => {
            let tag_name = release
                .tag_name
                .as_ref()
                .ok_or_else(|| eyre::eyre!("release does not have tag_name"))?;
            api.repo_get_archive(repo.owner(), repo.name(), &format!("{}.tar.gz", tag_name))
                .await?
        }
        name => {
            let assets = release
                .assets
                .as_ref()
                .ok_or_else(|| eyre::eyre!("release does not have assets"))?;
            let asset = assets
                .iter()
                .find(|a| a.name.as_deref() == Some(name))
                .ok_or_else(|| eyre!("asset not found"))?;
            let release_id = release
                .id
                .ok_or_else(|| eyre::eyre!("release does not have id"))?;
            let asset_id = asset
                .id
                .ok_or_else(|| eyre::eyre!("asset does not have id"))?;
            api.download_release_attachment(repo.owner(), repo.name(), release_id, asset_id)
                .await?
        }
    };
    let real_output = output
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new(&asset));
    tokio::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(real_output)
        .await?
        .write_all(file.as_ref())
        .await?;

    if output.is_some() {
        println!("Downloaded {asset} into {}", real_output.display());
    } else {
        println!("Downloaded {asset}");
    }

    Ok(())
}

async fn find_release(
    repo: &RepoName,
    api: &Forgejo,
    name: &str,
) -> eyre::Result<forgejo_api::structs::Release> {
    let query = RepoListReleasesQuery {
        q: None,
        draft: None,
        pre_release: None,
    };
    api.repo_list_releases(repo.owner(), repo.name(), query)
        .stream()
        .try_filter(|r| {
            futures::future::ready(
                r.name
                    .as_deref()
                    .is_some_and(|release_name| release_name == name),
            )
        })
        .try_next()
        .await?
        .ok_or_else(|| eyre::eyre!("could not find release {name}"))
}

async fn delete_release(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    by_tag: bool,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    let identifier = if by_tag { "tag" } else { "release" };

    if dry_run {
        println!("Would delete {} `{}`", identifier, name);
        println!("\nRun with --force to actually delete.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Delete {} `{}`?", identifier, name);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Deleting {} {}", identifier, name);
    if by_tag {
        api.repo_delete_release_by_tag(repo.owner(), repo.name(), &name)
            .await?;
    } else {
        let id = find_release(repo, api, &name)
            .await?
            .id
            .ok_or_else(|| eyre::eyre!("release does not have id"))?;
        api.repo_delete_release(repo.owner(), repo.name(), id)
            .await?;
    }
    println!("Deleted {} `{}`", identifier, name);
    Ok(())
}
