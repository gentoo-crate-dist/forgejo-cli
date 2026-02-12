use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Subcommand};
use eyre::{eyre, Context, OptionExt};
use forgejo_api::structs::{
    Comment, CreateIssueCommentOption, CreateIssueOption, EditIssueOption, IssueGetCommentsQuery,
};
use forgejo_api::Forgejo;

use crate::repo::{RepoArg, RepoInfo, RepoName};

pub mod template;

#[derive(Args, Clone, Debug)]
pub struct IssueCommand {
    /// The local git remote that points to the repo to operate on.
    #[clap(long, short = 'R', global = true)]
    remote: Option<String>,
    #[clap(subcommand)]
    command: IssueSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum IssueSubcommand {
    /// Create a new issue on a repo
    Create {
        /// Title of the issue
        title: Option<String>,
        /// The text body of the issue
        ///
        /// Leaving this out will open your editor, unless --body-file is specified.
        #[clap(long, conflicts_with = "template")]
        body: Option<String>,
        /// The text body of the issue, to read from a file
        #[clap(long, conflicts_with = "body")]
        body_file: Option<PathBuf>,
        /// The template to use when creating an issue
        ///
        /// If the repo has disabled blank issues, this flag is required.
        #[clap(long)]
        template: Option<String>,
        /// Don't use a template for this issue.
        ///
        /// If the repo has disabled blank issues, this will fail.
        #[clap(long, conflicts_with = "template")]
        no_template: bool,
        /// The repo to create this issue on
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// Open the issue creation page in your web browser
        #[clap(long)]
        web: bool,
    },
    /// Edit an issue
    Edit {
        issue: IssueId,
        #[clap(subcommand)]
        command: EditCommand,
    },
    /// Add a comment on an issue
    Comment {
        issue: IssueId,
        /// The text content of the comment.
        ///
        /// Leaving this out will open your editor, unless --body-file is specified.
        body: Option<String>,
        /// The text content of the comment, to read from a file
        #[clap(long, conflicts_with = "body")]
        body_file: Option<PathBuf>,
    },
    /// Close an issue
    Close {
        issue: IssueId,
        /// A comment to leave on the issue before closing it
        #[clap(long, short)]
        with_msg: Option<Option<String>>,
    },
    /// Search for an issue in a repo
    Search {
        /// The repo to search in
        #[clap(long, short)]
        repo: Option<RepoArg>,
        query: Option<String>,
        #[clap(long, short)]
        labels: Option<String>,
        #[clap(long, short)]
        creator: Option<String>,
        #[clap(long, short)]
        assignee: Option<String>,
        /// Filter issues by state. Default: open
        #[clap(long, short)]
        state: Option<State>,
    },
    /// View an issue's info
    View {
        id: IssueId,
        #[clap(subcommand)]
        command: Option<ViewCommand>,
    },
    /// List the issue templates in a repo.
    Templates {
        /// The repo to view the templates of.
        #[clap(long, short)]
        repo: Option<RepoArg>,
    },
    /// Open an issue in your browser
    Browse { id: IssueId },
}

#[derive(Clone, Debug)]
pub struct IssueId {
    pub repo: Option<RepoArg>,
    pub number: i64,
}

impl FromStr for IssueId {
    type Err = IssueIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (repo, number) = match s.rsplit_once("#") {
            Some((repo, number)) => (Some(repo.parse::<RepoArg>()?), number),
            None => (None, s),
        };
        Ok(Self {
            repo,
            number: number.parse::<u64>()? as i64,
        })
    }
}

#[derive(Debug, Clone)]
pub enum IssueIdError {
    Repo(crate::repo::RepoArgError),
    Number(std::num::ParseIntError),
}

impl std::fmt::Display for IssueIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueIdError::Repo(e) => e.fmt(f),
            IssueIdError::Number(e) => e.fmt(f),
        }
    }
}

impl From<crate::repo::RepoArgError> for IssueIdError {
    fn from(value: crate::repo::RepoArgError) -> Self {
        Self::Repo(value)
    }
}

impl From<std::num::ParseIntError> for IssueIdError {
    fn from(value: std::num::ParseIntError) -> Self {
        Self::Number(value)
    }
}

impl std::error::Error for IssueIdError {}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum State {
    Open,
    Closed,
    All,
}

impl From<State> for forgejo_api::structs::IssueListIssuesQueryState {
    fn from(value: State) -> Self {
        match value {
            State::Open => forgejo_api::structs::IssueListIssuesQueryState::Open,
            State::Closed => forgejo_api::structs::IssueListIssuesQueryState::Closed,
            State::All => forgejo_api::structs::IssueListIssuesQueryState::All,
        }
    }
}

#[derive(Subcommand, Clone, Debug)]
pub enum EditCommand {
    /// Edit an issue's title
    Title { new_title: Option<String> },
    /// Edit an issue's text content
    Body { new_body: Option<String> },
    /// Edit a comment on an issue
    Comment {
        idx: usize,
        new_body: Option<String>,
    },
    /// Assign a user to an issue
    Assign {
        /// Username to assign
        user: String,
    },
    /// Unassign a user from an issue
    Unassign {
        /// Username to unassign (omit to clear all)
        user: Option<String>,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ViewCommand {
    /// View an issue's title and body. The default
    Body,
    /// View a specific
    Comment { idx: usize },
    /// List every comment
    Comments,
}

impl IssueCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        use IssueSubcommand::*;
        let repo = RepoInfo::get_current(host_name, self.repo(), self.remote.as_deref(), &keys)?;
        let api = keys.get_api(repo.host_url()).await?;
        let repo = repo.name().ok_or_else(|| self.no_repo_error())?;
        match self.command {
            Create {
                repo: _,
                title,
                body,
                body_file,
                template,
                no_template,
                web,
            } => {
                create_issue(
                    repo,
                    &api,
                    title,
                    body,
                    body_file,
                    template,
                    no_template,
                    web,
                )
                .await?
            }
            View { id, command } => match command.unwrap_or(ViewCommand::Body) {
                ViewCommand::Body => view_issue(repo, &api, id.number).await?,
                ViewCommand::Comment { idx } => view_comment(repo, &api, id.number, idx).await?,
                ViewCommand::Comments => view_comments(repo, &api, id.number).await?,
            },
            Search {
                repo: _,
                query,
                labels,
                creator,
                assignee,
                state,
            } => view_issues(repo, &api, query, labels, creator, assignee, state).await?,
            Templates { .. } => view_issue_templates(repo, &api).await?,
            Edit { issue, command } => match command {
                EditCommand::Title { new_title } => {
                    edit_title(repo, &api, issue.number, new_title).await?
                }
                EditCommand::Body { new_body } => {
                    edit_body(repo, &api, issue.number, new_body).await?
                }
                EditCommand::Comment { idx, new_body } => {
                    edit_comment(repo, &api, issue.number, idx, new_body).await?
                }
                EditCommand::Assign { user } => {
                    edit_assign(repo, &api, issue.number, user).await?
                }
                EditCommand::Unassign { user } => {
                    edit_unassign(repo, &api, issue.number, user).await?
                }
            },
            Close { issue, with_msg } => close_issue(repo, &api, issue.number, with_msg).await?,
            Browse { id } => browse_issue(repo, &api, id.number).await?,
            Comment {
                issue,
                body,
                body_file,
            } => add_comment(repo, &api, issue.number, body, body_file).await?,
        }
        Ok(())
    }

    fn repo(&self) -> Option<&RepoArg> {
        use IssueSubcommand::*;
        match &self.command {
            Create { repo, .. } | Search { repo, .. } | Templates { repo } => repo.as_ref(),
            View { id: issue, .. }
            | Edit { issue, .. }
            | Close { issue, .. }
            | Comment { issue, .. }
            | Browse { id: issue, .. } => issue.repo.as_ref(),
        }
    }

    fn no_repo_error(&self) -> eyre::Error {
        use IssueSubcommand::*;
        match &self.command {
            Create { .. } | Search { .. } | Templates { .. } => {
                eyre::eyre!("can't figure what repo to access, try specifying with `--repo`")
            }
            View { id: issue, .. }
            | Edit { issue, .. }
            | Close { issue, .. }
            | Comment { issue, .. }
            | Browse { id: issue, .. } => eyre::eyre!(
                "can't figure out what repo to access, try specifying with `{{owner}}/{{repo}}#{}`",
                issue.number
            ),
        }
    }
}

pub async fn label_names_to_ids(
    repo: &RepoName,
    api: &Forgejo,
    names: Vec<String>,
) -> eyre::Result<Vec<i64>> {
    let query = forgejo_api::structs::IssueListLabelsQuery { sort: None };
    let (_, all_labels) = api
        .issue_list_labels(repo.owner(), repo.name(), query)
        .await?;
    let mut all_labels = all_labels
        .into_iter()
        .filter_map(|l| Some((l.name?, l.id?)))
        .collect::<BTreeMap<_, _>>();
    Ok(names
        .into_iter()
        .filter_map(|name| all_labels.remove(&name))
        .collect())
}

pub async fn maybe_label_names_to_ids(
    repo: &RepoName,
    api: &Forgejo,
    names: Option<Vec<String>>,
) -> eyre::Result<Option<Vec<i64>>> {
    Ok(match names {
        Some(names) => Some(crate::issues::label_names_to_ids(repo, api, names).await?),
        None => None,
    })
}

async fn create_issue(
    repo: &RepoName,
    api: &Forgejo,
    title: Option<String>,
    body: Option<String>,
    body_file: Option<PathBuf>,
    template: Option<String>,
    no_template: bool,
    web: bool,
) -> eyre::Result<()> {
    match (title, web) {
        (Some(title), false) => {
            let body_from_file = match body_file {
                None => None,
                Some(ref path) => Some(crate::read_file_or_stdin(path).await?),
            };
            let body = body.or(body_from_file);

            let has_templates = api
                .repo_get_issue_templates(repo.owner(), repo.name())
                .await
                .is_ok();
            let blank_issues_enabled = api
                .repo_get_issue_config(repo.owner(), repo.name())
                .await?
                .blank_issues_enabled
                .unwrap_or(true);

            let opts = if let Some(template_name) = template {
                eyre::ensure!(
                    has_templates,
                    "{}/{} does not have any issue templates",
                    repo.owner(),
                    repo.name()
                );
                let (template_file, is_yaml) =
                    template::get_template_file(repo, api, &template_name).await?;
                let (body, metadata) =
                    crate::issues::template::generate_from_template(body, template_file, is_yaml)
                        .await?;

                CreateIssueOption {
                    body: Some(body),
                    title,
                    assignee: None,
                    assignees: None,
                    closed: None,
                    due_date: None,
                    labels: maybe_label_names_to_ids(repo, api, metadata.labels).await?,
                    milestone: None,
                    r#ref: metadata.r#ref,
                }
            } else {
                eyre::ensure!(
                    blank_issues_enabled,
                    "{}/{} requires using a template. \
                    Please choose one with `--template <NAME>`",
                    repo.owner(),
                    repo.name()
                );
                eyre::ensure!(
                    !has_templates || no_template,
                    "{}/{} uses issue templates. \
                    Please choose one with `--template <NAME>`, \
                    or use `--no-template` to write one from scratch",
                    repo.owner(),
                    repo.name()
                );
                let body = match body {
                    Some(body) => body,
                    None => {
                        let mut body = String::new();
                        crate::editor(&mut body, Some("md")).await?;
                        body
                    }
                };
                CreateIssueOption {
                    body: Some(body),
                    title,
                    assignee: None,
                    assignees: None,
                    closed: None,
                    due_date: None,
                    labels: None,
                    milestone: None,
                    r#ref: None,
                }
            };

            let issue = api
                .issue_create_issue(repo.owner(), repo.name(), opts)
                .await?;
            let number = issue
                .number
                .ok_or_else(|| eyre::eyre!("issue does not have number"))?;
            let title = issue
                .title
                .as_ref()
                .ok_or_else(|| eyre::eyre!("issue does not have title"))?;
            eprintln!("created issue #{}: {}", number, title);
        }
        (None, true) => {
            let base_repo = api.repo_get(repo.owner(), repo.name()).await?;
            let mut issue_create_url = base_repo
                .html_url
                .clone()
                .ok_or_eyre("repo does not have html url")?;
            issue_create_url
                .path_segments_mut()
                .expect("invalid url")
                .extend(["issues", "new"]);
            open::that_detached(issue_create_url.as_str()).wrap_err("Failed to open URL")?;
        }
        (None, false) => {
            eyre::bail!("requires either issue title or --web flag")
        }
        (Some(_), true) => {
            eyre::bail!("issue title and --web flag are mutually exclusive")
        }
    }
    Ok(())
}

pub async fn view_issue(repo: &RepoName, api: &Forgejo, id: i64) -> eyre::Result<()> {
    let crate::SpecialRender {
        dash,

        bright_red,
        bright_green,
        yellow,
        dark_grey,
        white,
        reset,
        ..
    } = crate::special_render();

    let issue = api.issue_get_issue(repo.owner(), repo.name(), id).await?;

    // if it's a pull request, display it as one instead
    if issue.pull_request.is_some() {
        crate::prs::view_pr(repo, api, Some(id)).await?;
        return Ok(());
    }

    let title = issue
        .title
        .as_ref()
        .ok_or_else(|| eyre::eyre!("issue does not have title"))?;
    let user = issue
        .user
        .as_ref()
        .ok_or_else(|| eyre::eyre!("issue does not have creator"))?;
    let username = user
        .login
        .as_ref()
        .ok_or_else(|| eyre::eyre!("user does not have login"))?;
    let state = issue
        .state
        .ok_or_else(|| eyre::eyre!("pr does not have state"))?;
    let comments = issue.comments.unwrap_or_default();

    println!("{yellow}{title} {dark_grey}#{id}{reset}");
    print!("By {white}{username}{reset} {dash} ");

    use forgejo_api::structs::StateType;
    match state {
        StateType::Open => println!("{bright_green}Open{reset}"),
        StateType::Closed => println!("{bright_red}Closed{reset}"),
    };

    if let Some(body) = &issue.body {
        if !body.is_empty() {
            println!();
            println!("{}", crate::markdown(body));
        }
    }
    println!();

    if comments == 1 {
        println!("1 comment");
    } else {
        println!("{comments} comments");
    }
    Ok(())
}
async fn view_issues(
    repo: &RepoName,
    api: &Forgejo,
    query_str: Option<String>,
    labels: Option<String>,
    creator: Option<String>,
    assignee: Option<String>,
    state: Option<State>,
) -> eyre::Result<()> {
    let labels = labels
        .map(|s| s.split(',').map(|s| s.to_string()).collect::<Vec<_>>())
        .unwrap_or_default();
    let query = forgejo_api::structs::IssueListIssuesQuery {
        q: query_str,
        labels: Some(labels.join(",")),
        created_by: creator,
        assigned_by: assignee,
        state: state.map(|s| s.into()),
        r#type: None,
        milestones: None,
        since: None,
        before: None,
        mentioned_by: None,
        sort: None,
    };
    let issues = api
        .issue_list_issues(repo.owner(), repo.name(), query)
        .all()
        .await?;
    if issues.len() == 1 {
        println!("1 issue");
    } else {
        println!("{} issues", issues.len());
    }
    for issue in issues {
        let number = issue
            .number
            .ok_or_else(|| eyre::eyre!("issue does not have number"))?;
        let title = issue
            .title
            .as_ref()
            .ok_or_else(|| eyre::eyre!("issue does not have title"))?;
        let user = issue
            .user
            .as_ref()
            .ok_or_else(|| eyre::eyre!("issue does not have creator"))?;
        let username = user
            .login
            .as_ref()
            .ok_or_else(|| eyre::eyre!("user does not have login"))?;
        println!("#{}: {} (by {})", number, title, username);
    }
    Ok(())
}

pub async fn view_issue_templates(repo: &RepoName, api: &Forgejo) -> eyre::Result<()> {
    let crate::SpecialRender {
        dash, bold, reset, ..
    } = crate::special_render();

    let mut total_count = 0;

    if let Ok(templates) = api
        .repo_get_issue_templates(repo.owner(), repo.name())
        .await
    {
        total_count += templates.len();
        for template in templates {
            let filename = template
                .file_name
                .as_deref()
                .ok_or_eyre("template does not have filename")?;
            let name = filename
                .rsplit_once(".")
                .map(|(s, _)| s)
                .unwrap_or(filename);
            let name = name.rsplit_once("/").map(|(_, s)| s).unwrap_or(name);
            let display_name = template.name.as_deref().filter(|s| !s.is_empty());
            print!("{bold}{name}{reset}");
            if let Some(display_name) = display_name {
                print!(" {dash} {display_name}");
            }
            println!();
            let desc = template.about.as_deref().filter(|s| !s.is_empty());
            if let Some(desc) = desc {
                println!("{}", crate::render_text(desc));
            }
            println!();
        }
    }

    let config = api.repo_get_issue_config(repo.owner(), repo.name()).await?;
    let contact_links = config.contact_links.unwrap_or_default();
    total_count += contact_links.len();
    for contact in contact_links {
        let url = contact.url.ok_or_eyre("contact info does not have url")?;
        let name = contact.name.ok_or_eyre("contact info does not have name")?;
        println!("{bold}{url}{reset} {dash} {name}");
        let desc = contact.about.as_deref().filter(|s| !s.is_empty());
        if let Some(desc) = desc {
            println!("{}", crate::render_text(desc));
        }
        println!();
    }

    if total_count == 0 {
        eprintln!("No issue templates or contact info.");
    }
    if config.blank_issues_enabled.unwrap_or(true) {
        println!("'--no-template' is allowed");
    } else {
        println!("'--no-template' is not allowed");
    }
    Ok(())
}

pub async fn view_comment(repo: &RepoName, api: &Forgejo, id: i64, idx: usize) -> eyre::Result<()> {
    let query = IssueGetCommentsQuery {
        since: None,
        before: None,
    };
    let (_, comments) = api
        .issue_get_comments(repo.owner(), repo.name(), id, query)
        .await?;
    let comment = comments
        .get(idx)
        .ok_or_else(|| eyre!("comment {idx} doesn't exist"))?;
    print_comment(comment)?;
    Ok(())
}

pub async fn view_comments(repo: &RepoName, api: &Forgejo, id: i64) -> eyre::Result<()> {
    let query = IssueGetCommentsQuery {
        since: None,
        before: None,
    };
    let (_, comments) = api
        .issue_get_comments(repo.owner(), repo.name(), id, query)
        .await?;
    for comment in comments {
        print_comment(&comment)?;
        println!();
    }
    Ok(())
}

fn print_comment(comment: &Comment) -> eyre::Result<()> {
    let body = comment
        .body
        .as_ref()
        .ok_or_else(|| eyre::eyre!("comment does not have body"))?;
    let user = comment
        .user
        .as_ref()
        .ok_or_else(|| eyre::eyre!("comment does not have user"))?;
    let name = user.full_name.as_deref().filter(|name| !name.is_empty());
    let username = user
        .login
        .as_ref()
        .ok_or_else(|| eyre::eyre!("user does not have login"))?;

    let crate::SpecialRender {
        bold,
        bright_cyan,
        dark_grey,
        reset,
        ..
    } = crate::special_render();
    if let Some(name) = name {
        println!("{bold}{bright_cyan}{name}{reset} {dark_grey}({username}){reset} said:");
    } else {
        println!("{bold}{bright_cyan}{username}{reset} said:");
    }
    println!("{}", crate::markdown(body));
    let assets = comment
        .assets
        .as_ref()
        .ok_or_else(|| eyre::eyre!("comment does not have assets"))?;
    if !assets.is_empty() {
        println!("({} attachments)", assets.len());
    }
    Ok(())
}

pub async fn browse_issue(repo: &RepoName, api: &Forgejo, id: i64) -> eyre::Result<()> {
    let issue = api.issue_get_issue(repo.owner(), repo.name(), id).await?;
    let html_url = issue
        .html_url
        .as_ref()
        .ok_or_else(|| eyre::eyre!("issue does not have html_url"))?;
    open::that_detached(html_url.as_str()).wrap_err("Failed to open URL")?;
    Ok(())
}

pub async fn add_comment(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    body: Option<String>,
    body_file: Option<PathBuf>,
) -> eyre::Result<()> {
    let body_from_file = match body_file {
        None => None,
        Some(ref path) => Some(crate::read_file_or_stdin(path).await?),
    };
    let body = match body.or(body_from_file) {
        Some(body) => body,
        None => {
            let mut body = String::new();
            crate::editor(&mut body, Some("md")).await?;
            body
        }
    };
    api.issue_create_comment(
        repo.owner(),
        repo.name(),
        issue,
        forgejo_api::structs::CreateIssueCommentOption {
            body,
            updated_at: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn edit_title(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    new_title: Option<String>,
) -> eyre::Result<()> {
    let new_title = match new_title {
        Some(s) => s,
        None => {
            let issue_info = api
                .issue_get_issue(repo.owner(), repo.name(), issue)
                .await?;
            let mut title = issue_info
                .title
                .ok_or_else(|| eyre::eyre!("issue does not have title"))?;
            crate::editor(&mut title, Some("md")).await?;
            title
        }
    };
    let new_title = new_title.trim();
    if new_title.is_empty() {
        eyre::bail!("title cannot be empty");
    }
    if new_title.contains('\n') {
        eyre::bail!("title cannot contain newlines");
    }
    api.issue_edit_issue(
        repo.owner(),
        repo.name(),
        issue,
        forgejo_api::structs::EditIssueOption {
            title: Some(new_title.to_owned()),
            assignee: None,
            assignees: None,
            body: None,
            due_date: None,
            milestone: None,
            r#ref: None,
            state: None,
            unset_due_date: None,
            updated_at: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn edit_body(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    new_body: Option<String>,
) -> eyre::Result<()> {
    let new_body = match new_body {
        Some(s) => s,
        None => {
            let issue_info = api
                .issue_get_issue(repo.owner(), repo.name(), issue)
                .await?;
            let mut body = issue_info
                .body
                .ok_or_else(|| eyre::eyre!("issue does not have body"))?;
            crate::editor(&mut body, Some("md")).await?;
            body
        }
    };
    api.issue_edit_issue(
        repo.owner(),
        repo.name(),
        issue,
        forgejo_api::structs::EditIssueOption {
            body: Some(new_body),
            assignee: None,
            assignees: None,
            due_date: None,
            milestone: None,
            r#ref: None,
            state: None,
            title: None,
            unset_due_date: None,
            updated_at: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn edit_comment(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    idx: usize,
    new_body: Option<String>,
) -> eyre::Result<()> {
    let (_, comments) = api
        .issue_get_comments(
            repo.owner(),
            repo.name(),
            issue,
            IssueGetCommentsQuery {
                since: None,
                before: None,
            },
        )
        .await?;
    let comment = comments
        .get(idx)
        .ok_or_else(|| eyre!("comment not found"))?;
    let new_body = match new_body {
        Some(s) => s,
        None => {
            let mut body = comment
                .body
                .clone()
                .ok_or_else(|| eyre::eyre!("issue does not have body"))?;
            crate::editor(&mut body, Some("md")).await?;
            body
        }
    };
    let id = comment
        .id
        .ok_or_else(|| eyre::eyre!("comment does not have id"))?;
    api.issue_edit_comment(
        repo.owner(),
        repo.name(),
        id,
        forgejo_api::structs::EditIssueCommentOption {
            body: new_body,
            updated_at: None,
        },
    )
    .await?;
    Ok(())
}

pub async fn edit_assign(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    user: String,
) -> eyre::Result<()> {
    let issue_data = api.issue_get_issue(repo.owner(), repo.name(), issue).await?;
    let mut assignees: Vec<String> = issue_data
        .assignees
        .unwrap_or_default()
        .iter()
        .filter_map(|u| u.login.clone())
        .collect();
    if assignees.iter().any(|a| a.eq_ignore_ascii_case(&user)) {
        eprintln!("{user} is already assigned to #{issue}");
        return Ok(());
    }
    assignees.push(user.clone());
    api.issue_edit_issue(
        repo.owner(),
        repo.name(),
        issue,
        EditIssueOption {
            assignees: Some(assignees),
            assignee: None,
            body: None,
            due_date: None,
            milestone: None,
            r#ref: None,
            state: None,
            title: None,
            unset_due_date: None,
            updated_at: None,
        },
    )
    .await?;
    eprintln!("assigned {user} to #{issue}");
    Ok(())
}

pub async fn edit_unassign(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    user: Option<String>,
) -> eyre::Result<()> {
    let assignees = match user {
        Some(ref user) => {
            let issue_data = api.issue_get_issue(repo.owner(), repo.name(), issue).await?;
            let current: Vec<String> = issue_data
                .assignees
                .unwrap_or_default()
                .iter()
                .filter_map(|u| u.login.clone())
                .collect();
            if !current.iter().any(|a| a.eq_ignore_ascii_case(user)) {
                eprintln!("{user} is not assigned to #{issue}");
                return Ok(());
            }
            let filtered: Vec<String> = current
                .into_iter()
                .filter(|a| !a.eq_ignore_ascii_case(user))
                .collect();
            filtered
        }
        None => Vec::new(),
    };
    api.issue_edit_issue(
        repo.owner(),
        repo.name(),
        issue,
        EditIssueOption {
            assignees: Some(assignees),
            assignee: None,
            body: None,
            due_date: None,
            milestone: None,
            r#ref: None,
            state: None,
            title: None,
            unset_due_date: None,
            updated_at: None,
        },
    )
    .await?;
    match user {
        Some(user) => eprintln!("unassigned {user} from #{issue}"),
        None => eprintln!("cleared all assignees from #{issue}"),
    }
    Ok(())
}

pub async fn close_issue(
    repo: &RepoName,
    api: &Forgejo,
    issue: i64,
    message: Option<Option<String>>,
) -> eyre::Result<()> {
    if let Some(message) = message {
        let body = match message {
            Some(m) => m,
            None => {
                let mut s = String::new();
                crate::editor(&mut s, Some("md")).await?;
                s
            }
        };

        let opt = CreateIssueCommentOption {
            body,
            updated_at: None,
        };
        api.issue_create_comment(repo.owner(), repo.name(), issue, opt)
            .await?;
    }

    let edit = EditIssueOption {
        state: Some("closed".into()),
        assignee: None,
        assignees: None,
        body: None,
        due_date: None,
        milestone: None,
        r#ref: None,
        title: None,
        unset_due_date: None,
        updated_at: None,
    };
    let issue_data = api
        .issue_edit_issue(repo.owner(), repo.name(), issue, edit)
        .await?;

    let issue_title = issue_data
        .title
        .as_deref()
        .ok_or_eyre("issue does not have title")?;

    println!("Closed issue {issue}: \"{issue_title}\"");

    Ok(())
}
