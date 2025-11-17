use std::{io::Write, path::PathBuf, str::FromStr};

use clap::{Args, Subcommand};
use eyre::{eyre, Context, OptionExt, Result};
use forgejo_api::{structs::CreateRepoOption, Forgejo};
use ssh2_config::ParseRule;
use url::Url;

use crate::SpecialRender;

pub struct RepoInfo {
    url: Url,
    name: Option<RepoName>,
    remote_name: Option<String>,
}

impl RepoInfo {
    pub fn get_current(
        host: Option<&str>,
        repo: Option<&RepoArg>,
        remote: Option<&str>,
        keys: &crate::keys::KeyInfo,
    ) -> eyre::Result<Self> {
        // l = domain/owner/name
        // s = owner/name
        // x = is present
        // i = found locally by git
        //
        // | repo | host | remote | ans-host | ans-repo |
        // |------|------|--------|----------|----------|
        // | l    | x    | x      | repo     | repo     |
        // | l    | x    | i      | repo     | repo     |
        // | l    | x    |        | repo     | repo     |
        // | l    |      | x      | repo     | repo     |
        // | l    |      | i      | repo     | repo     |
        // | l    |      |        | repo     | repo     |
        // | s    | x    | x      | host     | repo     |
        // | s    | x    | i      | host     | repo     |
        // | s    | x    |        | host     | repo     |
        // | s    |      | x      | remote   | repo     |
        // | s    |      | i      | remote   | repo     |
        // | s    |      |        | err      | repo     |
        // |      | x    | x      | remote   | remote   |
        // |      | x    | i      | host     | ?remote  |
        // |      | x    |        | host     | none     |
        // |      |      | x      | remote   | remote   |
        // |      |      | i      | remote   | remote   |
        // |      |      |        | err      | remote   |

        let mut repo_url: Option<Url> = None;
        let mut repo_name: Option<RepoName> = None;

        if let Some(repo) = repo {
            if let Some(host) = &repo.host {
                repo_url = Url::parse(host)
                    .ok()
                    .filter(|x| !x.cannot_be_a_base())
                    .or_else(|| Url::parse(&format!("https://{host}/")).ok())
                    .map(|url| keys.deref_alias(url))
            }
            repo_name = Some(RepoName {
                owner: repo.owner.clone(),
                name: repo.name.clone(),
            });
        }

        let repo_url = repo_url;
        let repo_name = repo_name;

        let host_url = host.and_then(|host| {
            Url::parse(host)
                .ok()
                .filter(|x| !x.cannot_be_a_base())
                .or_else(|| Url::parse(&format!("https://{host}/")).ok())
                .map(|url| keys.deref_alias(url))
        });

        let mut final_remote_name = None;

        let (remote_url, remote_repo_name) = {
            let mut out = (None, None);
            if let Ok(local_repo) = git2::Repository::discover(".") {
                let mut name = remote.map(|s| s.to_owned());

                // if there's only one remote, use that
                if name.is_none() {
                    let all_remotes = local_repo.remotes()?;
                    if all_remotes.len() == 1 {
                        if let Some(remote_name) = all_remotes.get(0) {
                            name = Some(remote_name.to_owned());
                        }
                    }
                }

                // if the current branch is tracking a remote branch, use that remote
                if name.is_none() {
                    let head = local_repo.head()?;
                    let branch_name = head.name().ok_or_eyre("branch name not UTF-8")?;

                    if let Ok(remote_name) = local_repo.branch_upstream_remote(branch_name) {
                        let remote_name_s =
                            remote_name.as_str().ok_or_eyre("remote name invalid")?;

                        if let Some(host_url) = &host_url {
                            let remote = local_repo.find_remote(remote_name_s)?;
                            let url_s = std::str::from_utf8(remote.url_bytes())?;
                            let url = keys.deref_alias(crate::ssh_url_parse(url_s)?);

                            if crate::host_with_port(&url) == crate::host_with_port(host_url) {
                                name = Some(remote_name_s.to_owned());
                            }
                        } else {
                            name = Some(remote_name_s.to_owned());
                        }
                    }
                }

                // if there's a remote whose host url matches the one
                // specified with `--host`, use that
                //
                // This is different than using `--host` itself, since this
                // will include the repo name, which `--host` can't do.
                if name.is_none() {
                    if let Some(host_url) = &host_url {
                        let all_remotes = local_repo.remotes()?;
                        for remote_name in all_remotes.iter() {
                            let Some(remote_name) = remote_name else {
                                continue;
                            };
                            let remote = local_repo.find_remote(remote_name)?;

                            if let Some(url) = remote.url() {
                                let url = keys.deref_alias(crate::ssh_url_parse(url)?);
                                let (url, _) = url_strip_repo_name(url)?;
                                if crate::host_with_port(&url) == crate::host_with_port(&url)
                                    && url.path() == host_url.path()
                                {
                                    name = Some(remote_name.to_owned());
                                    break;
                                }
                            }
                        }
                    }
                }

                if let Some(name) = name {
                    if let Ok(remote) = local_repo.find_remote(&name) {
                        let url_s = std::str::from_utf8(remote.url_bytes())?;
                        let url = keys.deref_alias(crate::ssh_url_parse(url_s)?);
                        let (url, repo_name) = url_strip_repo_name(url)?;

                        out = (Some(url), Some(repo_name));

                        final_remote_name = Some(name);
                    }
                }
            } else {
                eyre::ensure!(remote.is_none(), "remote specified but no git repo found");
            }
            out
        };

        let same_instance = |a: &Option<Url>, b: &Option<Url>| {
            let a = a.as_ref().map(crate::host_with_port_and_path);
            let b = b.as_ref().map(crate::host_with_port_and_path);
            a == b
        };

        let (url, name) = if repo_url.is_some() {
            (repo_url, repo_name)
        } else if repo_name.is_some() {
            (host_url.or(remote_url), repo_name)
        } else if remote.is_some() {
            (remote_url, remote_repo_name)
        } else if host_url.is_none() || same_instance(&remote_url, &host_url) {
            (remote_url, remote_repo_name)
        } else {
            (host_url, None)
        };

        let url = url.or_else(fallback_host).map(|url| {
            let mut url = match url.scheme() {
                "http" | "https" => url,
                _ => url::Url::parse(&format!("https{}", &url[url::Position::AfterScheme..]))
                    .expect("should always be valid"),
            };
            url.set_username("").expect("shouldn't fail");
            url
        });

        let info = match (url, name) {
            (Some(url), name) => RepoInfo {
                url,
                name,
                remote_name: final_remote_name,
            },
            (None, Some(_)) => eyre::bail!("cannot find repo, no host specified"),
            (None, None) => eyre::bail!("no repo info specified"),
        };

        Ok(info)
    }

    pub fn name(&self) -> Option<&RepoName> {
        self.name.as_ref()
    }

    pub fn host_url(&self) -> &Url {
        &self.url
    }

    pub fn remote_name(&self) -> Option<&str> {
        self.remote_name.as_deref()
    }
}

fn fallback_host() -> Option<Url> {
    if let Some(envvar) = std::env::var_os("FJ_FALLBACK_HOST") {
        let out = envvar.to_str().and_then(|x| x.parse::<Url>().ok());
        if out.is_none() {
            println!("warn: `FJ_FALLBACK_HOST` is not set to a valid url");
        }
        out
    } else {
        None
    }
}

fn url_strip_repo_name(mut url: Url) -> eyre::Result<(Url, RepoName)> {
    let mut iter = url
        .path_segments()
        .ok_or_eyre("repo url cannot be a base")?
        .rev();

    let name = iter.next().ok_or_eyre("repo url too short")?;
    let name = name.strip_suffix(".git").unwrap_or(name).to_owned();

    let owner = iter.next().ok_or_eyre("repo url too short")?.to_owned();

    // Remove the username and repo name from the url
    url.path_segments_mut()
        .map_err(|_| eyre!("repo url cannot be a base"))?
        .pop()
        .pop();

    Ok((url, RepoName { owner, name }))
}

#[derive(Clone, Debug)]
pub struct RepoName {
    owner: String,
    name: String,
}

impl RepoName {
    pub fn new(owner: String, name: String) -> Self {
        Self { owner, name }
    }
    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug, Clone)]
pub struct RepoArg {
    host: Option<String>,
    owner: String,
    name: String,
}

impl std::fmt::Display for RepoArg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.host {
            Some(host) => write!(f, "{host}/{}/{}", self.owner, self.name),
            None => write!(f, "{}/{}", self.owner, self.name),
        }
    }
}

impl FromStr for RepoArg {
    type Err = RepoArgError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (head, name) = s.rsplit_once("/").ok_or(RepoArgError::NoOwner)?;
        let name = name.strip_suffix(".git").unwrap_or(name);
        let (host, owner) = match head.rsplit_once("/") {
            Some((host, owner)) => (Some(host), owner),
            None => (None, head),
        };
        Ok(Self {
            host: host.map(|s| s.to_owned()),
            owner: owner.to_owned(),
            name: name.to_owned(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoArgError {
    NoOwner,
}

impl std::error::Error for RepoArgError {}

impl std::fmt::Display for RepoArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepoArgError::NoOwner => {
                write!(f, "repo name should be in the format [HOST/]OWNER/NAME")
            }
        }
    }
}

#[derive(Args, Clone, Debug)]
pub struct RepoCreateArgs {
    pub repo: String,

    // flags
    #[clap(long, short)]
    pub description: Option<String>,
    #[clap(long, short = 'P')]
    pub private: bool,
    /// Creates a new remote with the given name for the new repo
    #[clap(long, short)]
    pub remote: Option<String>,
    /// Pushes the current branch to the default branch on the new repo.
    /// Implies `--remote=origin` (setting remote manually overrides this)
    #[clap(long, short)]
    pub push: bool,
    /// Use SSH for the new remote instead of HTTP(S)
    #[clap(long, short = 'S')]
    pub ssh: Option<Option<bool>>,
}

#[derive(Subcommand, Clone, Debug)]
pub enum RepoCommand {
    /// Creates a new repository
    Create {
        #[clap(flatten)]
        args: RepoCreateArgs,
    },
    /// Fork a repository onto your account
    Fork {
        repo: RepoArg,
        #[clap(long)]
        name: Option<String>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
    Migrate {
        /// URL of the repo to migrate
        repo: String,
        /// Name of the new mirror
        name: String,
        /// Whether to mirror the repo instead of migrating it
        #[clap(long, short)]
        mirror: bool,
        /// Whether the new migration should be private
        #[clap(long, short)]
        private: bool,
        /// Comma-separated list of items to include. Defaults to nothing but git data.
        ///
        /// These are `lfs`, `wiki`, `issues`, `prs`, `milestones`, `labels`, and `releases`.
        /// You can use `all` to include everything.
        #[clap(long, short)]
        include: Option<MigrateInclude>,
        /// The URL to fetch LFS files from
        #[clap(long, short = 'L')]
        lfs_endpoint: Option<url::Url>,
        /// The type of Git service the original repo is on. Defaults to `git`
        #[clap(long, short)]
        service: Option<MigrateService>,
        /// If enabled, will read an access token in from stdin to use for fetching.
        ///
        /// Mutually exclusive with `--login`
        #[clap(long, short)]
        token: bool,
        /// If enabled, will read a username and password from stdin to use for fetching.
        ///
        /// Mutually exclusive with `--token`.
        ///
        /// This is not recommended, `--token` should be used instead whenever possible.
        #[clap(long, short)]
        login: bool,
    },
    /// View a repo's info
    View {
        name: Option<RepoArg>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
    /// View a repo's README
    Readme {
        name: Option<RepoArg>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
    /// Clone a repo's code locally
    Clone {
        repo: RepoArg,
        path: Option<PathBuf>,
        /// Clone the repo over SSH instead of HTTP(S)
        #[clap(long, short = 'S')]
        ssh: Option<Option<bool>>,
    },
    /// Add a star to a repo
    Star {
        repo: Option<RepoArg>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
    /// Take away a star from a repo
    Unstar {
        repo: Option<RepoArg>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
    /// Delete a repository
    ///
    /// This cannot be undone!
    Delete { repo: RepoArg },
    /// Open a repository's page in your browser
    Browse {
        name: Option<RepoArg>,
        #[clap(long, short = 'R')]
        remote: Option<String>,
    },
}

impl RepoCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        match self {
            RepoCommand::Create {
                args:
                    RepoCreateArgs {
                        repo,

                        description,
                        private,
                        remote,
                        push,
                        ssh,
                    },
            } => {
                let host = RepoInfo::get_current(host_name, None, None, &keys)?;
                let api = keys.get_api(host.host_url()).await?;
                let url_host = crate::host_with_port(&host.host_url());
                let ssh = ssh
                    .unwrap_or(Some(keys.default_ssh.contains(url_host)))
                    .unwrap_or(true);
                create_repo(&api, None, repo, description, private, remote, push, ssh).await?;
            }
            RepoCommand::Fork { repo, name, remote } => {
                fn strip(s: &str) -> &str {
                    let no_scheme = s
                        .strip_prefix("https://")
                        .or_else(|| s.strip_prefix("http://"))
                        .unwrap_or(s);
                    let no_trailing_slash = no_scheme.strip_suffix("/").unwrap_or(no_scheme);
                    no_trailing_slash
                }
                if let (Some(a), Some(b)) = (repo.host.as_deref(), host_name) {
                    if strip(a) != strip(b) {
                        eyre::bail!("conflicting hosts {a} and {b}. please only specify one");
                    }
                }

                let repo_info =
                    RepoInfo::get_current(host_name, Some(&repo), remote.as_deref(), &keys)?;
                let api = keys.get_api(repo_info.host_url()).await?;
                let repo = repo_info
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                fork_repo(&api, repo, name).await?
            }
            RepoCommand::Migrate {
                repo,
                name,
                mirror,
                private,
                include,
                lfs_endpoint,
                service,
                token,
                login,
            } => {
                let current_repo = RepoInfo::get_current(host_name, None, None, &keys)?;
                let api = keys.get_api(current_repo.host_url()).await?;
                migrate_repo(
                    &api,
                    repo,
                    name,
                    mirror,
                    private,
                    include,
                    lfs_endpoint,
                    service,
                    token,
                    login,
                )
                .await?
            }
            RepoCommand::View { name, remote } => {
                let repo =
                    RepoInfo::get_current(host_name, name.as_ref(), remote.as_deref(), &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let repo = repo
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                view_repo(&api, repo).await?
            }
            RepoCommand::Readme { name, remote } => {
                let repo =
                    RepoInfo::get_current(host_name, name.as_ref(), remote.as_deref(), &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let repo = repo
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                view_repo_readme(&api, repo).await?
            }
            RepoCommand::Clone { repo, path, ssh } => {
                let repo = RepoInfo::get_current(host_name, Some(&repo), None, &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let name = repo.name().unwrap();
                let url_host = crate::host_with_port(&repo.host_url());
                let ssh = ssh
                    .unwrap_or(Some(keys.default_ssh.contains(url_host)))
                    .unwrap_or(true);
                cmd_clone_repo(&api, name, path, ssh).await?;
            }
            RepoCommand::Star { repo, remote } => {
                let repo =
                    RepoInfo::get_current(host_name, repo.as_ref(), remote.as_deref(), &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let name = repo
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                api.user_current_put_star(name.owner(), name.name()).await?;
                println!("Starred {}/{}!", name.owner(), name.name());
            }
            RepoCommand::Unstar { repo, remote } => {
                let repo =
                    RepoInfo::get_current(host_name, repo.as_ref(), remote.as_deref(), &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let name = repo
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                api.user_current_delete_star(name.owner(), name.name())
                    .await?;
                println!("Removed star from {}/{}", name.owner(), name.name());
            }
            RepoCommand::Delete { repo } => {
                let repo = RepoInfo::get_current(host_name, Some(&repo), None, &keys)?;
                let api = keys.get_api(repo.host_url()).await?;
                let name = repo.name().unwrap();
                delete_repo(&api, name).await?;
            }
            RepoCommand::Browse { name, remote } => {
                let repo =
                    RepoInfo::get_current(host_name, name.as_ref(), remote.as_deref(), &keys)?;
                let mut url = repo.host_url().clone();
                let repo = repo
                    .name()
                    .ok_or_eyre("couldn't get repo name, please specify")?;
                url.path_segments_mut()
                    .map_err(|_| eyre!("url invalid"))?
                    .extend([repo.owner(), repo.name()]);

                open::that_detached(url.as_str()).wrap_err("Failed to open URL")?;
            }
        };
        Ok(())
    }
}

pub async fn create_repo(
    api: &Forgejo,
    org: Option<String>,
    repo: String,
    description: Option<String>,
    private: bool,
    remote: Option<String>,
    push: bool,
    ssh: bool,
) -> eyre::Result<()> {
    if remote.is_some() || push {
        let repo = git2::Repository::discover(".")?;

        let upstream = remote.as_deref().unwrap_or("origin");
        if repo.find_remote(upstream).is_ok() {
            eyre::bail!("A remote named \"{upstream}\" already exists");
        }
    }
    let repo_spec = CreateRepoOption {
        auto_init: Some(false),
        default_branch: Some("main".into()),
        description,
        gitignores: None,
        issue_labels: None,
        license: None,
        name: repo,
        object_format_name: None,
        private: Some(private),
        readme: Some(String::new()),
        template: Some(false),
        trust_model: Some(forgejo_api::structs::CreateRepoOptionTrustModel::Default),
    };
    let new_repo = if let Some(org) = org {
        api.create_org_repo(&org, repo_spec).await?
    } else {
        api.create_current_user_repo(repo_spec).await?
    };
    let html_url = new_repo
        .html_url
        .as_ref()
        .ok_or_else(|| eyre::eyre!("new_repo does not have html_url"))?;
    println!("created new repo at {}", html_url);

    if remote.is_some() || push {
        let repo = git2::Repository::discover(".")?;

        let upstream = remote.as_deref().unwrap_or("origin");
        let remote_url = git_url(&new_repo, ssh)?;
        let mut remote = repo.remote(upstream, remote_url.as_str())?;

        if push {
            let head = repo.head()?;
            if !head.is_branch() {
                eyre::bail!("HEAD is not on a branch; cannot push to remote");
            }
            let branch_shorthand = head
                .shorthand()
                .ok_or_else(|| eyre!("branch name invalid utf-8"))?
                .to_owned();
            let branch_name = std::str::from_utf8(head.name_bytes())?.to_owned();

            let auth = auth_git2::GitAuthenticator::new();
            auth.push(&repo, &mut remote, &[&branch_name])?;

            remote.fetch(&[&branch_shorthand], None, None)?;

            let mut current_branch = git2::Branch::wrap(head);
            current_branch.set_upstream(Some(&format!("{upstream}/{branch_shorthand}")))?;
        }
    }

    Ok(())
}

async fn fork_repo(api: &Forgejo, repo: &RepoName, name: Option<String>) -> eyre::Result<()> {
    let opt = forgejo_api::structs::CreateForkOption {
        name,
        organization: None,
    };
    let new_fork = api.create_fork(repo.owner(), repo.name(), opt).await?;
    let fork_full_name = new_fork
        .full_name
        .as_deref()
        .ok_or_eyre("fork does not have name")?;
    println!(
        "Forked {}/{} into {}",
        repo.owner(),
        repo.name(),
        fork_full_name
    );

    Ok(())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum, Default)]
pub enum MigrateService {
    #[default]
    Git,
    Github,
    Gitlab,
    Forgejo,
    Gitea,
    Gogs,
    Onedev,
    Gitbucket,
    Codebase,
}

impl FromStr for MigrateService {
    type Err = MigrateServiceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "git" => Ok(Self::Git),
            "github" => Ok(Self::Github),
            "gitlab" => Ok(Self::Gitlab),
            "forgejo" => Ok(Self::Forgejo),
            "gitea" => Ok(Self::Gitea),
            "gogs" => Ok(Self::Gogs),
            "onedev" | "one-dev" => Ok(Self::Onedev),
            "gitbucket" | "git-bucket" => Ok(Self::Gitbucket),
            "codebase" => Ok(Self::Codebase),
            _ => Err(MigrateServiceParseError),
        }
    }
}

impl MigrateService {
    fn to_api_type(self) -> forgejo_api::structs::MigrateRepoOptionsService {
        use forgejo_api::structs::MigrateRepoOptionsService as Api;
        use MigrateService as Cli;
        match self {
            Cli::Git => Api::Git,
            Cli::Github => Api::Github,
            Cli::Gitlab => Api::Gitlab,
            Cli::Forgejo => Api::Gitea,
            Cli::Gitea => Api::Gitea,
            Cli::Gogs => Api::Gogs,
            Cli::Onedev => Api::Onedev,
            Cli::Gitbucket => Api::Gitbucket,
            Cli::Codebase => Api::Codebase,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MigrateServiceParseError;

impl std::error::Error for MigrateServiceParseError {}

impl std::fmt::Display for MigrateServiceParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown service")
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct MigrateInclude {
    lfs: bool,
    wiki: bool,
    issues: bool,
    prs: bool,
    milestones: bool,
    labels: bool,
    releases: bool,
}

impl MigrateInclude {
    /// if the selection includes anything other than LFS (which is supported by base git)
    fn non_base_git(self) -> bool {
        self.wiki | self.issues | self.prs | self.milestones | self.labels | self.releases
    }
}

impl FromStr for MigrateInclude {
    type Err = MigrateIncludeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "all" {
            Ok(Self {
                lfs: true,
                wiki: true,
                issues: true,
                prs: true,
                milestones: true,
                labels: true,
                releases: true,
            })
        } else {
            let mut out = Self::default();
            for opt in s.split(",") {
                match opt {
                    "lfs" => out.lfs = true,
                    "wiki" => out.wiki = true,
                    "issues" => out.issues = true,
                    "prs" => out.prs = true,
                    "milestones" => out.milestones = true,
                    "labels" => out.labels = true,
                    "releases" => out.releases = true,
                    _ => return Err(MigrateIncludeParseError),
                }
            }
            Ok(out)
        }
    }
}

#[derive(Clone, Debug)]
pub struct MigrateIncludeParseError;

impl std::error::Error for MigrateIncludeParseError {}

impl std::fmt::Display for MigrateIncludeParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("unknown include option")
    }
}

async fn migrate_repo(
    api: &Forgejo,
    mut repo: String,
    name: String,
    mirror: bool,
    private: bool,
    include: Option<MigrateInclude>,
    lfs_endpoint: Option<url::Url>,
    service: Option<MigrateService>,
    token: bool,
    login: bool,
) -> eyre::Result<()> {
    let include = include.unwrap_or_default();
    let service = service.unwrap_or_default();

    if service == MigrateService::Git && include.non_base_git() {
        eyre::bail!("Migrating from a `git` service doesn't support migration items other than LFS. Please specify a different service or remove the included items");
    }

    if repo.ends_with("/") {
        let _ = repo.pop();
    }
    if !repo.ends_with(".git") {
        repo.push_str(".git");
    }
    let clone_url =
        url::Url::parse(&repo).or_else(|_| url::Url::parse(&format!("https://{repo}")))?;

    let (username, password) = if login {
        let username = crate::readline("Username: ").await?.trim().to_owned();
        let password = crate::readline("Password: ").await?.trim().to_owned();
        (Some(username), Some(password))
    } else {
        (None, None)
    };

    let auth_token = if token {
        let auth_token = crate::readline("Token: ").await?.trim().to_owned();
        Some(auth_token.trim().to_owned())
    } else {
        None
    };

    let migrate_options = forgejo_api::structs::MigrateRepoOptions {
        auth_password: password,
        auth_username: username,
        auth_token,
        clone_addr: clone_url.as_str().to_owned(),
        description: None,
        issues: Some(include.issues),
        labels: Some(include.labels),
        lfs: Some(include.lfs),
        lfs_endpoint: lfs_endpoint.map(|url| url.to_string()),
        milestones: Some(include.milestones),
        mirror: Some(mirror),
        mirror_interval: None,
        private: Some(private),
        pull_requests: Some(include.prs),
        releases: Some(include.releases),
        repo_name: name,
        repo_owner: None,
        service: Some(service.to_api_type()),
        uid: None,
        wiki: Some(include.wiki),
    };

    println!("Migrating...");
    let new_repo = api.repo_migrate(migrate_options).await?;
    let new_repo_url = new_repo
        .html_url
        .as_ref()
        .ok_or_eyre("new repo doesnt have url")?;
    println!("Done! View online at {new_repo_url}");

    Ok(())
}

async fn view_repo(api: &Forgejo, repo: &RepoName) -> eyre::Result<()> {
    let repo = api.repo_get(repo.owner(), repo.name()).await?;

    let SpecialRender {
        dash,
        body_prefix,
        dark_grey,
        reset,
        ..
    } = crate::special_render();

    println!("{}", repo.full_name.ok_or_eyre("no full name")?);

    if let Some(parent) = &repo.parent {
        println!(
            "Fork of {}",
            parent.full_name.as_ref().ok_or_eyre("no full name")?
        );
    }
    if repo.mirror == Some(true) {
        if let Some(original) = &repo.original_url {
            println!("Mirror of {original}")
        }
    }
    let desc = repo.description.as_deref().unwrap_or_default();
    // Don't use body::markdown, this is plain text.
    if !desc.is_empty() {
        if desc.lines().count() > 1 {
            println!();
        }
        for line in desc.lines() {
            println!("{dark_grey}{body_prefix}{reset} {line}");
        }
    }
    println!();

    let lang = repo.language.as_deref().unwrap_or_default();
    if !lang.is_empty() {
        println!("Primary language is {lang}");
    }

    let stars = repo.stars_count.unwrap_or_default();
    if stars == 1 {
        print!("{stars} star {dash} ");
    } else {
        print!("{stars} stars {dash} ");
    }

    let watchers = repo.watchers_count.unwrap_or_default();
    print!("{watchers} watching {dash} ");

    let forks = repo.forks_count.unwrap_or_default();
    if forks == 1 {
        print!("{forks} fork");
    } else {
        print!("{forks} forks");
    }
    println!();

    let mut first = true;
    if repo.has_issues.unwrap_or_default() && repo.external_tracker.is_none() {
        let issues = repo.open_issues_count.unwrap_or_default();
        if issues == 1 {
            print!("{issues} issue");
        } else {
            print!("{issues} issues");
        }
        first = false;
    }
    if repo.has_pull_requests.unwrap_or_default() {
        if !first {
            print!(" {dash} ");
        }
        let pulls = repo.open_pr_counter.unwrap_or_default();
        if pulls == 1 {
            print!("{pulls} PR");
        } else {
            print!("{pulls} PRs");
        }
        first = false;
    }
    if repo.has_releases.unwrap_or_default() {
        if !first {
            print!(" {dash} ");
        }
        let releases = repo.release_counter.unwrap_or_default();
        if releases == 1 {
            print!("{releases} release");
        } else {
            print!("{releases} releases");
        }
        first = false;
    }
    if !first {
        println!();
    }
    if let Some(external_tracker) = &repo.external_tracker {
        if let Some(tracker_url) = &external_tracker.external_tracker_url {
            println!("Issue tracker is at {tracker_url}");
        }
    }

    if let Some(html_url) = &repo.html_url {
        println!();
        println!("View online at {html_url}");
    }

    Ok(())
}

async fn view_repo_readme(api: &Forgejo, repo: &RepoName) -> eyre::Result<()> {
    let query = forgejo_api::structs::RepoGetRawFileQuery { r#ref: None };
    let file = api
        .repo_get_raw_file(repo.owner(), repo.name(), "README.md", query)
        .await;
    if let Ok(readme) = file {
        let readme_str = String::from_utf8_lossy(&readme);
        println!("{}", crate::markdown(&readme_str));
        return Ok(());
    } else {
        let query = forgejo_api::structs::RepoGetRawFileQuery { r#ref: None };
        let file = api
            .repo_get_raw_file(repo.owner(), repo.name(), "README.txt", query)
            .await;
        if let Ok(readme) = file {
            let readme_str = String::from_utf8_lossy(&readme);
            println!("{}", crate::render_text(&readme_str));
            return Ok(());
        }
    }
    eyre::bail!("Repo does not have README.md or README.txt");
}

async fn cmd_clone_repo(
    api: &Forgejo,
    name: &RepoName,
    path: Option<std::path::PathBuf>,
    ssh: bool,
) -> eyre::Result<()> {
    let repo_data = api.repo_get(name.owner(), name.name()).await?;
    let clone_url = git_url(&repo_data, ssh)?;

    let repo_name = repo_data
        .name
        .as_deref()
        .ok_or_eyre("repo does not have name")?;
    let repo_full_name = repo_data
        .full_name
        .as_deref()
        .ok_or_eyre("repo does not have full name")?;

    let path = path.unwrap_or_else(|| PathBuf::from(format!("./{repo_name}")));

    let local_repo = clone_repo(repo_full_name, clone_url, &path)?;

    if let Some(parent) = repo_data.parent.as_deref() {
        local_repo.remote("upstream", git_url(&parent, ssh)?.as_str())?;
    }

    Ok(())
}

pub fn git_url(repo: &forgejo_api::structs::Repository, ssh: bool) -> eyre::Result<&Url> {
    if ssh {
        repo.ssh_url
            .as_ref()
            .ok_or_eyre("repo does not have ssh url")
    } else {
        repo.clone_url
            .as_ref()
            .ok_or_eyre("repo does not have clone url")
    }
}

pub fn clone_repo(
    repo_name: &str,
    url: &url::Url,
    path: &std::path::Path,
) -> eyre::Result<git2::Repository> {
    let SpecialRender {
        fancy,
        hide_cursor,
        show_cursor,
        clear_line,
        ..
    } = *crate::special_render();

    let mut auth = auth_git2::GitAuthenticator::new();
    // I find it surprising that auth_git2 just hardcodes what key files to look for instead of
    // looking in .ssh/config
    if url.scheme() == "ssh" {
        let ssh_config =
            ssh2_config::SshConfig::parse_default_file(ParseRule::ALLOW_UNKNOWN_FIELDS)?;
        let params = ssh_config.query(url.host_str().ok_or_eyre("url does not have host")?);
        if let Some(identity_file) = params.identity_file.as_deref() {
            for path in identity_file {
                auth = auth.add_ssh_key_from_file(path, None);
            }
        }
    }

    let git_config = git2::Config::open_default()?;

    let mut options = git2::FetchOptions::new();
    let mut callbacks = git2::RemoteCallbacks::new();
    callbacks.credentials(auth.credentials(&git_config));

    if fancy {
        print!("{hide_cursor}");
        print!("   Preparing...");
        let _ = std::io::stdout().flush();

        callbacks.transfer_progress(|progress| {
            print!("{clear_line}\r");
            if progress.received_objects() == progress.total_objects() {
                if progress.indexed_deltas() == progress.total_deltas() {
                    print!("Finishing up...");
                } else {
                    let percent = 100.0 * (progress.indexed_deltas() as f64)
                        / (progress.total_deltas() as f64);
                    print!("   Resolving... {percent:.01}%");
                }
            } else {
                let bytes = progress.received_bytes();
                let percent = 100.0 * (progress.received_objects() as f64)
                    / (progress.total_objects() as f64);
                print!(" Downloading... {percent:.01}%");
                match bytes {
                    0..=1023 => print!(" ({}b)", bytes),
                    1024..=1048575 => print!(" ({:.01}kb)", (bytes as f64) / 1024.0),
                    1048576..=1073741823 => {
                        print!(" ({:.01}mb)", (bytes as f64) / 1048576.0)
                    }
                    1073741824.. => {
                        print!(" ({:.01}gb)", (bytes as f64) / 1073741824.0)
                    }
                }
            }
            let _ = std::io::stdout().flush();
            true
        });
    }
    options.remote_callbacks(callbacks);

    let local_repo = git2::build::RepoBuilder::new()
        .fetch_options(options)
        .clone(url.as_str(), path)?;
    if fancy {
        print!("{clear_line}{show_cursor}\r");
    }
    println!("Cloned {} into {}", repo_name, path.display());
    Ok(local_repo)
}

async fn delete_repo(api: &Forgejo, name: &RepoName) -> eyre::Result<()> {
    print!(
        "Are you sure you want to delete {}/{}? (y/N) ",
        name.owner(),
        name.name()
    );
    let user_response = crate::readline("").await?;
    let yes = matches!(user_response.trim(), "y" | "Y" | "yes" | "Yes");
    if yes {
        api.repo_delete(name.owner(), name.name()).await?;
        println!("Deleted {}/{}", name.owner(), name.name());
    } else {
        println!("Did not delete");
    }
    Ok(())
}
