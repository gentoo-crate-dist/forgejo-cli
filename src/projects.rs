use std::str::FromStr;

use clap::{Args, Subcommand};
use eyre::{Context, OptionExt};
use url::Url;
use forgejo_api::structs::{
    CreateProjectOption, EditProjectOption, RepoListProjectsQuery, RepoListProjectsQueryState,
    CreateProjectColumnOption, EditProjectColumnOption, AddCardToColumnOption,
    OrgListProjectsQuery, OrgListProjectsQueryState, Issue, StateType,
    ColumnPosition, MoveProjectColumnsOption,
};
use forgejo_api::{Forgejo, ForgejoError};

use crate::repo::{RepoArg, RepoInfo, RepoName};

/// Manage Forgejo projects (kanban boards).
///
/// Projects can be associated with repositories or organizations.
/// Use project ID (number) or title to identify projects.
#[derive(Args, Clone, Debug)]
pub struct ProjectCommand {
    /// The local git remote that points to the repo to operate on.
    #[clap(long, short = 'R')]
    remote: Option<String>,
    #[clap(subcommand)]
    command: ProjectSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ProjectSubcommand {
    // ========== SIMPLIFIED COMMANDS (preferred) ==========

    /// Add an issue to a project column
    #[clap(visible_alias = "a")]
    Add {
        /// Issue number to add
        issue: u64,
        /// Column name to add the issue to (case-insensitive)
        column: String,
        /// Project name (optional if only one project exists)
        #[clap(long, short)]
        project: Option<String>,
        /// The repo to operate on (e.g., owner/repo or host/owner/repo)
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// Position in the column (1-based, default: end)
        #[clap(long)]
        position: Option<u64>,
    },
    /// Move an issue to a different column
    #[clap(visible_alias = "mv")]
    Move {
        /// Issue number to move
        issue: u64,
        /// Target column name (case-insensitive)
        column: String,
        /// Project name (optional if only one project exists)
        #[clap(long, short)]
        project: Option<String>,
        /// The repo to operate on (e.g., owner/repo or host/owner/repo)
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// Position in the target column (1-based, default: end)
        #[clap(long)]
        position: Option<u64>,
    },
    /// Remove an issue from a project
    #[clap(visible_alias = "rm")]
    Remove {
        /// Issue number to remove
        issue: u64,
        /// Project name (optional if only one project exists)
        #[clap(long, short)]
        project: Option<String>,
        /// The repo to operate on (e.g., owner/repo or host/owner/repo)
        #[clap(long, short)]
        repo: Option<RepoArg>,
    },
    /// Show which column an issue is in
    Status {
        /// Issue number to check
        issue: u64,
        /// Project name (optional if only one project exists)
        #[clap(long, short)]
        project: Option<String>,
        /// The repo to operate on (e.g., owner/repo or host/owner/repo)
        #[clap(long, short)]
        repo: Option<RepoArg>,
    },
    /// Show the project board
    Board {
        /// Project name (optional if only one project exists)
        project: Option<String>,
        /// The repo to operate on (e.g., owner/repo or host/owner/repo)
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// Show detailed information (assignees, labels, due dates)
        #[clap(long, short)]
        verbose: bool,
        /// Filter to show only a specific column (case-insensitive)
        #[clap(long, short = 'c')]
        column: Option<String>,
    },

    // ========== EXISTING COMMANDS ==========

    /// List projects in a repository or organization
    List {
        /// The repo to list projects from
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// The organization to list projects from
        #[clap(long, short)]
        org: Option<String>,
        /// Filter by project state (open, closed, all)
        #[clap(long, short)]
        state: Option<StateFilter>,
    },
    /// Create a new project
    Create {
        /// Title of the project
        title: Option<String>,
        /// Description of the project
        #[clap(long)]
        body: Option<String>,
        /// Project template type (none, basic-kanban, bug-triage)
        #[clap(long)]
        template: Option<String>,
        /// The repo to create this project in
        #[clap(long, short)]
        repo: Option<RepoArg>,
        /// Open the project creation page in your web browser
        #[clap(long)]
        web: bool,
    },
    /// View a project's details
    View {
        /// Project ID or title (supports owner/repo:id format)
        id: ProjectId,
        #[clap(subcommand)]
        command: Option<ViewCommand>,
    },
    /// Edit a project
    #[clap(name = "edit-project")]
    EditProject {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        #[clap(subcommand)]
        command: EditCommand,
    },
    /// Delete a project
    #[clap(name = "delete-project")]
    DeleteProject {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        /// Skip confirmation prompt
        #[clap(long)]
        force: bool,
    },
    /// Close a project
    Close {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
    },
    /// Open a project
    Open {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
    },
    /// Open a project in your browser
    Browse {
        /// Project ID or title (supports owner/repo:id format)
        id: ProjectId,
    },
    /// Manage project columns (advanced)
    #[clap(subcommand)]
    Column(ColumnCommand),
    /// Manage project cards (advanced - use 'add', 'move', 'remove' instead)
    #[clap(subcommand, hide = true)]
    Card(CardCommand),
}

#[derive(Subcommand, Clone, Debug)]
pub enum ViewCommand {
    /// View project details (default)
    Info,
    /// View project columns
    Columns,
    /// View project board (kanban view)
    Board,
}

#[derive(Subcommand, Clone, Debug)]
pub enum EditCommand {
    /// Edit project title
    Title { new_title: Option<String> },
    /// Edit project description
    Body { new_body: Option<String> },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ColumnCommand {
    /// List columns in a project
    List {
        project: Option<String>,
    },
    /// Create a new column
    Create {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        /// Column title
        title: Option<String>,
        /// Column color (hex color code)
        #[clap(long)]
        color: Option<String>,
    },
    /// Edit a column
    Edit {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        column: u64,
        #[clap(subcommand)]
        command: EditColumnCommand,
    },
    /// Delete a column
    Delete {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        column: u64,
        /// Skip confirmation prompt
        #[clap(long)]
        force: bool,
    },
    /// Move a column to a different position
    Move {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        column: u64,
        /// New position (1-based)
        position: u8,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum EditColumnCommand {
    /// Edit column title
    Title { new_title: Option<String> },
    /// Edit column color
    Color { new_color: Option<String> },
}

#[derive(Subcommand, Clone, Debug)]
pub enum CardCommand {
    /// List cards in a project or column
    List {
        project: Option<String>,
        /// Filter by specific column (ID or name, case-insensitive)
        #[clap(long)]
        column: Option<ColumnIdentifier>,
    },
    /// Add an issue as a card to a project
    Add {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        /// Issue number to add
        issue: u64,
        /// Column ID to add the card to
        #[clap(long)]
        column: Option<u64>,
        /// Position in the column (1-based)
        #[clap(long)]
        position: Option<u64>,
    },
    /// Remove a card from a project
    Remove {
        /// Project ID or title (supports owner/repo:id format)
        project: ProjectId,
        /// Issue number to remove
        issue: u64,
    },
    /// Move a card between columns
    Move {
        /// Project to move card in (optional if default is set)
        #[clap(long, short)]
        project: Option<ProjectId>,
        /// Issue number to move (supports #1 or 1 syntax)
        issue: IssueIdentifier,
        /// Target column (ID or name)
        column: ColumnIdentifier,
        /// Position in the target column (1-based)
        #[clap(long)]
        position: Option<u64>,
    },
}

/// State filter for CLI usage
#[derive(Clone, Debug)]
pub enum StateFilter {
    Open,
    Closed,
    All,
}

impl FromStr for StateFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(StateFilter::Open),
            "closed" => Ok(StateFilter::Closed),
            "all" => Ok(StateFilter::All),
            _ => Err(format!("Invalid state filter: {}. Use 'open', 'closed', or 'all'", s)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ProjectIdentifier {
    Name(String),
    Number(u64),
}

#[derive(Clone, Debug)]
pub enum ColumnIdentifier {
    Name(String),
    Number(u64),
}

impl FromStr for ColumnIdentifier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.parse::<u64>() {
            Ok(num) => Ok(ColumnIdentifier::Number(num)),
            Err(_) => Ok(ColumnIdentifier::Name(s.to_string())),
        }
    }
}

impl std::fmt::Display for ColumnIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ColumnIdentifier::Name(name) => write!(f, "{}", name),
            ColumnIdentifier::Number(num) => write!(f, "{}", num),
        }
    }
}

#[derive(Clone, Debug)]
pub enum IssueIdentifier {
    Number(u64),
}

impl FromStr for IssueIdentifier {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Support both "1" and "#1" syntax
        let s = s.strip_prefix('#').unwrap_or(s);
        match s.parse::<u64>() {
            Ok(num) => Ok(IssueIdentifier::Number(num)),
            Err(_) => Err(format!("Invalid issue identifier: {}", s)),
        }
    }
}

impl std::fmt::Display for IssueIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IssueIdentifier::Number(num) => write!(f, "#{}", num),
        }
    }
}

impl IssueIdentifier {
    pub fn number(&self) -> u64 {
        match self {
            IssueIdentifier::Number(num) => *num,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ProjectId {
    pub repo: Option<RepoArg>,
    pub identifier: ProjectIdentifier,
}

impl ProjectId {
    pub async fn resolve(&self, repo: &RepoName, api: &Forgejo) -> eyre::Result<u64> {
        match &self.identifier {
            ProjectIdentifier::Name(name) => {
                // Check if this is an organization project
                if self.is_org_project() {
                    if let Some(org_name) = self.org_name() {
                        let projects = api.org_list_projects(
                            &org_name,
                            OrgListProjectsQuery {
                                state: None,
                                q: None,
                                sort: None,
                                since: None,
                                before: None,
                                created_by: None,
                            },
                        ).await?;

                        if let Some(project) = projects.iter().find(|p| p.title.as_deref() == Some(name)) {
                            return Ok(project.id.unwrap_or(0) as u64);
                        }

                        // If name not found and it's a valid number, try as ID
                        if let Ok(id) = name.parse::<u64>() {
                            return Ok(id);
                        }

                        eyre::bail!("Project '{}' not found in organization '{}'", name, org_name);
                    }
                }

                // Repository project (default behavior)
                let projects = api.repo_list_projects(
                    repo.owner(),
                    repo.name(),
                    RepoListProjectsQuery {
                        state: None,
                        q: None,
                        sort: None,
                        since: None,
                        before: None,
                        created_by: None,
                    },
                ).await?;

                if let Some(project) = projects.iter().find(|p| p.title.as_deref() == Some(name)) {
                    return Ok(project.id.unwrap_or(0) as u64);
                }

                // If name not found and it's a valid number, try as ID
                if let Ok(id) = name.parse::<u64>() {
                    return Ok(id);
                }

                eyre::bail!("Project '{}' not found", name)
            }
            ProjectIdentifier::Number(id) => Ok(*id),
        }
    }

    /// Resolve project ID from argument or git config default
    pub async fn from_args_or_config(
        arg: Option<String>,
        repo: &RepoName,
        api: &Forgejo,
    ) -> eyre::Result<u64> {
        // First check if explicit argument provided
        if let Some(project_arg) = arg {
            return ProjectId::from_str(&project_arg)?.resolve(repo, api).await;
        }

        // Check git config for default
        if let Ok(Some(default)) = crate::config::get_default_project() {
            // Parse config format: "owner/repo:id" or "org/-:id"
            return parse_config_project_id(&default, repo).await;
        }

        eyre::bail!("No project specified and no default configured. Set a default with: git config fj.defaultProject owner/repo:id")
    }

    /// Check if this project ID refers to an organization project
    pub fn is_org_project(&self) -> bool {
        if let Some(repo_arg) = &self.repo {
            let repo_arg_str = format!("{}", repo_arg);
            // Organization projects either use the legacy "org/-" format or just "org" (no slash)
            repo_arg_str.ends_with("/-") || !repo_arg_str.contains('/')
        } else {
            false
        }
    }

    /// Get the organization name if this is an org project
    pub fn org_name(&self) -> Option<String> {
        if let Some(repo_arg) = &self.repo {
            let repo_arg_str = format!("{}", repo_arg);
            if repo_arg_str.ends_with("/-") {
                // Legacy format: "org/-"
                Some(repo_arg_str.trim_end_matches("/-").to_string())
            } else if !repo_arg_str.contains('/') {
                // New format: just "org"
                Some(repo_arg_str)
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Resolve organization project ID without needing repo context
    pub async fn resolve_org(&self, api: &Forgejo) -> eyre::Result<u64> {
        if !self.is_org_project() {
            return Err(eyre::eyre!("ProjectId is not an organization project"));
        }

        let org_name = self.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;

        match &self.identifier {
            ProjectIdentifier::Name(name) => {
                let projects = api.org_list_projects(
                    &org_name,
                    OrgListProjectsQuery {
                        state: None,
                        q: None,
                        sort: None,
                        since: None,
                        before: None,
                        created_by: None,
                    },
                ).await?;

                if let Some(project) = projects.iter().find(|p| p.title.as_deref() == Some(name)) {
                    return Ok(project.id.unwrap_or(0) as u64);
                }

                // If name not found and it's a valid number, try as ID
                if let Ok(id) = name.parse::<u64>() {
                    return Ok(id);
                }

                eyre::bail!("Project '{}' not found in organization '{}'", name, org_name);
            }
            ProjectIdentifier::Number(id) => Ok(*id),
        }
    }
}

impl FromStr for ProjectId {
    type Err = ProjectIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (repo, identifier_str) = match s.rsplit_once(":") {
            Some((repo_part, identifier)) => {
                // Check if this looks like an organization project (no slash in repo_part)
                if !repo_part.contains('/') {
                    // Organization project: create a synthetic RepoArg with org/-
                    let synthetic_repo_str = format!("{}/-", repo_part);
                    let repo_arg = synthetic_repo_str.parse::<RepoArg>().map_err(ProjectIdError::InvalidRepo)?;
                    (Some(repo_arg), identifier)
                } else {
                    // Regular repository project
                    let repo_arg = repo_part.parse::<RepoArg>().map_err(ProjectIdError::InvalidRepo)?;
                    (Some(repo_arg), identifier)
                }
            },
            None => (None, s),
        };

        // Validate not empty
        if identifier_str.is_empty() {
            return Err(ProjectIdError::EmptyIdentifier);
        }

        // Try to parse as number first, then as name
        let identifier = if let Ok(number) = identifier_str.parse::<u64>() {
            ProjectIdentifier::Number(number)
        } else {
            ProjectIdentifier::Name(identifier_str.to_string())
        };

        Ok(Self {
            repo,
            identifier,
        })
    }
}

#[derive(Debug)]
pub enum ProjectIdError {
    InvalidNumber(std::num::ParseIntError),
    InvalidRepo(crate::repo::RepoArgError),
    EmptyIdentifier,
}

impl std::fmt::Display for ProjectIdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProjectIdError::InvalidNumber(e) => write!(f, "Invalid project number: {}", e),
            ProjectIdError::InvalidRepo(e) => write!(f, "Invalid repository: {}", e),
            ProjectIdError::EmptyIdentifier => write!(f, "Project identifier cannot be empty"),
        }
    }
}

impl std::error::Error for ProjectIdError {}

/// Parse config project ID format (owner/repo:id or org/-:id)
async fn parse_config_project_id(config_value: &str, _repo: &RepoName) -> eyre::Result<u64> {
    let (repo_part, id_str) = config_value
        .rsplit_once(":")
        .ok_or_else(|| eyre::eyre!("Invalid config format. Expected owner/repo:id or org/-:id, got: {}", config_value))?;

    let id = id_str.parse::<u64>()
        .map_err(|_| eyre::eyre!("Invalid project ID in config: {}", id_str))?;

    // Check if this is an organization project (ends with "/-")
    if repo_part.ends_with("/-") {
        // Organization project - now supported!
        Ok(id)
    } else {
        // Repository project - just return the ID since we already validated it
        Ok(id)
    }
}

/// Helper to resolve repo from either explicit --repo arg or repo_info from git remote
fn resolve_repo<'a>(
    repo_info: &'a Option<RepoInfo>,
    repo_arg: Option<&RepoArg>,
    cmd: &ProjectCommand,
) -> eyre::Result<&'a RepoName> {
    // If repo_arg is provided, we should have created a synthetic RepoInfo
    // If not, use the detected repo_info
    if let Some(info) = repo_info {
        info.name().ok_or_else(|| cmd.no_repo_error())
    } else {
        Err(cmd.no_repo_error())
    }
}

impl ProjectCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {

        // Check if we need repo context for this operation
        let needs_repo = self.needs_repo_context();
        let explicit_repo = self.repo();

        let (repo_info, api) = if needs_repo || explicit_repo.is_some() {
            // Either we need repo from git remote, or explicit --repo was provided
            let repo_info = RepoInfo::get_current(host_name, explicit_repo, self.remote.as_deref(), &keys)?;
            let api = keys.get_api(repo_info.host_url()).await?;
            (Some(repo_info), api)
        } else {
            // For org-only operations, get the host from current repo context
            let repo_info = RepoInfo::get_current(host_name, None, self.remote.as_deref(), &keys)?;
            let api = keys.get_api(repo_info.host_url()).await?;
            (None, api)
        };

        // Clone the command to avoid borrowing issues
        let command = self.command.clone();

        match command {
            // ========== SIMPLIFIED COMMANDS ==========
            ProjectSubcommand::Add { issue, column, project, repo: repo_arg, position } => {
                let repo = resolve_repo(&repo_info, repo_arg.as_ref(), &self)?;

                // Auto-select project
                let (project_id, project_name) = auto_select_project(repo, &api, project).await?;

                // Resolve column name to ID
                let (column_id, column_name) = find_column_by_name(repo, &api, project_id, &column).await?;

                // Add the issue as a card
                let _card = api.repo_create_card(
                    repo.owner(),
                    repo.name(),
                    &project_id.to_string(),
                    column_id as i64,
                    AddCardToColumnOption {
                        issue_id: issue as i64,
                        position: position.map(|p| p as i64),
                    },
                ).await?;

                println!("Added issue #{} to \"{}\" in project \"{}\"", issue, column_name, project_name);
            },
            ProjectSubcommand::Move { issue, column, project, repo: repo_arg, position } => {
                let repo = resolve_repo(&repo_info, repo_arg.as_ref(), &self)?;

                // Auto-select project
                let (project_id, _project_name) = auto_select_project(repo, &api, project).await?;

                // Find the card for this issue
                let (card_id, _old_col_id, old_col_name) = find_card_by_issue(repo, &api, project_id, issue).await?;

                // Resolve target column name to ID
                let (column_id, column_name) = find_column_by_name(repo, &api, project_id, &column).await?;

                // Move the card
                use forgejo_api::structs::MoveProjectCardOption;
                api.repo_move_card(
                    repo.owner(),
                    repo.name(),
                    &project_id.to_string(),
                    card_id as i64,
                    MoveProjectCardOption {
                        column_id: Some(column_id as i64),
                        position: position.map(|p| p as i64),
                    },
                ).await?;

                println!("Moved issue #{} from \"{}\" to \"{}\"", issue, old_col_name, column_name);
            },
            ProjectSubcommand::Remove { issue, project, repo: repo_arg } => {
                let repo = resolve_repo(&repo_info, repo_arg.as_ref(), &self)?;

                // Auto-select project
                let (project_id, project_name) = auto_select_project(repo, &api, project).await?;

                // Find the card for this issue
                let (card_id, _col_id, col_name) = find_card_by_issue(repo, &api, project_id, issue).await?;

                // Delete the card
                api.repo_delete_card(
                    repo.owner(),
                    repo.name(),
                    &project_id.to_string(),
                    card_id as i64,
                ).await?;

                println!("Removed issue #{} from project \"{}\" (was in \"{}\")", issue, project_name, col_name);
            },
            ProjectSubcommand::Status { issue, project, repo: repo_arg } => {
                let repo = resolve_repo(&repo_info, repo_arg.as_ref(), &self)?;

                // Auto-select project
                let (project_id, project_name) = auto_select_project(repo, &api, project).await?;

                // Find the card for this issue
                match find_card_by_issue(repo, &api, project_id, issue).await {
                    Ok((_card_id, _col_id, col_name)) => {
                        println!("Issue #{} is in \"{}\" column", issue, col_name);
                    },
                    Err(_) => {
                        println!("Issue #{} is not in project \"{}\"", issue, project_name);
                    }
                }
            },
            ProjectSubcommand::Board { project, repo: repo_arg, verbose, column } => {
                let repo = resolve_repo(&repo_info, repo_arg.as_ref(), &self)?;

                // Auto-select project
                let (project_id, project_name) = auto_select_project(repo, &api, project).await?;

                // Display the board
                display_board(repo, &api, project_id, &project_name, verbose, column.as_deref()).await?;
            },

            // ========== EXISTING COMMANDS ==========
            ProjectSubcommand::List { repo: _, org, state } => {
                if let Some(org_name) = org {
                    list_org_projects(&org_name, &api, state).await?
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    list_projects(repo, &api, state).await?
                }
            },
            ProjectSubcommand::Create { repo: _, title, body, template, web } => {
                let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                create_project(repo, &api, title, body, template, web).await?
            },
            ProjectSubcommand::View { id, command } => {
                if id.is_org_project() {
                    let project_id = id.resolve_org(&api).await?;
                    let org_name = id.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    match command.unwrap_or(ViewCommand::Info) {
                        ViewCommand::Info => view_org_project(&org_name, &api, project_id).await?,
                        ViewCommand::Columns => view_org_project_columns(&org_name, &api, project_id).await?,
                        ViewCommand::Board => view_org_project_board(&org_name, &api, project_id).await?,
                    }
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = id.resolve(repo, &api).await?;
                    match command.unwrap_or(ViewCommand::Info) {
                        ViewCommand::Info => view_project(repo, &api, project_id).await?,
                        ViewCommand::Columns => view_project_columns(repo, &api, project_id).await?,
                        ViewCommand::Board => view_project_board(repo, &api, project_id).await?,
                    }
                }
            },
            ProjectSubcommand::EditProject { project, command } => {
                if project.is_org_project() {
                    let project_id = project.resolve_org(&api).await?;
                    let org_name = project.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    match command {
                        EditCommand::Title { new_title } => {
                            edit_org_project_title(&org_name, &api, project_id, new_title).await?
                        }
                        EditCommand::Body { new_body } => {
                            edit_org_project_body(&org_name, &api, project_id, new_body).await?
                        }
                    }
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = project.resolve(repo, &api).await?;
                    match command {
                        EditCommand::Title { new_title } => {
                            edit_project_title(repo, &api, project_id, new_title).await?
                        }
                        EditCommand::Body { new_body } => {
                            edit_project_body(repo, &api, project_id, new_body).await?
                        }
                    }
                }
            },
            ProjectSubcommand::DeleteProject { project, force } => {
                if project.is_org_project() {
                    let project_id = project.resolve_org(&api).await?;
                    let org_name = project.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    delete_org_project(&org_name, &api, project_id, force).await?
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = project.resolve(repo, &api).await?;
                    delete_project(repo, &api, project_id, force).await?
                }
            },
            ProjectSubcommand::Close { project } => {
                if project.is_org_project() {
                    let project_id = project.resolve_org(&api).await?;
                    let org_name = project.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    close_org_project(&org_name, &api, project_id).await?
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = project.resolve(repo, &api).await?;
                    close_project(repo, &api, project_id).await?
                }
            },
            ProjectSubcommand::Open { project } => {
                if project.is_org_project() {
                    let project_id = project.resolve_org(&api).await?;
                    let org_name = project.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    open_org_project(&org_name, &api, project_id).await?
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = project.resolve(repo, &api).await?;
                    open_project(repo, &api, project_id).await?
                }
            },
            ProjectSubcommand::Browse { id } => {
                if id.is_org_project() {
                    let project_id = id.resolve_org(&api).await?;
                    let org_name = id.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                    browse_org_project(&org_name, &api, project_id).await?
                } else {
                    let repo_info = repo_info.ok_or_else(|| self.no_repo_error())?;
                    let repo = repo_info.name().ok_or_else(|| self.no_repo_error())?;
                    let project_id = id.resolve(repo, &api).await?;
                    browse_project(repo, &api, project_id).await?
                }
            },
            ProjectSubcommand::Column(cmd) => {
                let repo = if let Some(repo_info) = &repo_info {
                    repo_info.name()
                } else {
                    None
                };
                cmd.run(repo, &api).await?
            },
            ProjectSubcommand::Card(cmd) => {
                if let Some(repo_info) = repo_info {
                    if let Some(repo) = repo_info.name() {
                        cmd.run(repo, &api).await?
                    } else {
                        return Err(self.no_repo_error());
                    }
                } else {
                    // Check if the card command has explicit project specification
                    match &cmd {
                        CardCommand::List { project: Some(project_arg), .. } => {
                            // Parse the project ID to extract repo info
                            let parsed_project = ProjectId::from_str(project_arg)?;
                            if let Some(repo_arg) = &parsed_project.repo {
                                let repo_string = repo_arg.to_string();
                                let (owner, name) = repo_string.split_once('/')
                                    .ok_or_else(|| eyre::eyre!("Invalid repository format"))?;
                                let synthetic_repo = RepoName::new(owner.to_string(), name.to_string());
                                cmd.run(&synthetic_repo, &api).await?
                            } else {
                                return Err(eyre::eyre!("Card operations require repository context or explicit project specification"));
                            }
                        },
                        CardCommand::Add { project, .. }
                        | CardCommand::Remove { project, .. } => {
                            if let Some(repo_arg) = &project.repo {
                                let repo_string = repo_arg.to_string();
                                let (owner, name) = repo_string.split_once('/')
                                    .ok_or_else(|| eyre::eyre!("Invalid repository format"))?;
                                let synthetic_repo = RepoName::new(owner.to_string(), name.to_string());
                                cmd.run(&synthetic_repo, &api).await?
                            } else {
                                return Err(eyre::eyre!("Card operations require repository context or explicit project specification"));
                            }
                        },
                        CardCommand::Move { project, .. } => {
                            match project {
                                Some(proj) => {
                                    if let Some(repo_arg) = &proj.repo {
                                        let repo_string = repo_arg.to_string();
                                        let (owner, name) = repo_string.split_once('/')
                                            .ok_or_else(|| eyre::eyre!("Invalid repository format"))?;
                                        let synthetic_repo = RepoName::new(owner.to_string(), name.to_string());
                                        cmd.run(&synthetic_repo, &api).await?
                                    } else {
                                        return Err(eyre::eyre!("Card operations require repository context or explicit project specification"));
                                    }
                                },
                                None => {
                                    return Err(eyre::eyre!("Card operations require repository context or explicit project specification"));
                                }
                            }
                        },
                        _ => {
                            return Err(eyre::eyre!("Card operations require repository context or explicit project specification"));
                        }
                    }
                }
            },
        }
        Ok(())
    }

    /// Check if this command needs repository context
    fn needs_repo_context(&self) -> bool {
        use ProjectSubcommand::*;
        match &self.command {
            // Simplified commands need repo context only if --repo not specified
            Add { repo, .. } | Move { repo, .. } | Remove { repo, .. }
            | Status { repo, .. } | Board { repo, .. } => repo.is_none(),
            List { org, .. } => org.is_none(), // Only need repo if not listing org projects
            Create { .. } => true, // Creating always needs repo for now
            View { id, .. } | EditProject { project: id, .. } | DeleteProject { project: id, .. }
            | Close { project: id } | Open { project: id } | Browse { id } => {
                // Only need repo context if it's not an org project AND has no explicit repo info
                !id.is_org_project() && id.repo.is_none()
            },
            Column(_) => true, // For now, column commands need repo context
            Card(card_cmd) => {
                // Card commands need repo context only if no explicit project is provided
                match card_cmd {
                    CardCommand::List { project, .. } => project.is_none(),
                    CardCommand::Add { project, .. }
                    | CardCommand::Remove { project, .. } => {
                        // These have required ProjectId, check if it has repo info
                        !project.is_org_project() && project.repo.is_none()
                    }
                    CardCommand::Move { project, .. } => {
                        // Move has optional ProjectId, need repo context if no project or project has no repo info
                        match project {
                            Some(proj) => !proj.is_org_project() && proj.repo.is_none(),
                            None => true, // No project specified, need repo context for default resolution
                        }
                    }
                }
            },
        }
    }

    fn repo(&self) -> Option<&RepoArg> {
        use ProjectSubcommand::*;
        match &self.command {
            // Simplified commands have optional repo
            Add { repo, .. } | Move { repo, .. } | Remove { repo, .. }
            | Status { repo, .. } | Board { repo, .. } => repo.as_ref(),
            List { repo, .. } | Create { repo, .. } => repo.as_ref(),
            View { id: project, .. }
            | EditProject { project, .. }
            | DeleteProject { project, .. }
            | Close { project }
            | Open { project }
            | Browse { id: project, .. } => project.repo.as_ref(),
            Column(cmd) => cmd.repo(),
            Card(cmd) => cmd.repo(),
        }
    }

    fn no_repo_error(&self) -> eyre::Error {
        use ProjectSubcommand::*;
        match &self.command {
            // Simplified commands need repo context from git remote
            Add { .. } | Move { .. } | Remove { .. } | Status { .. } | Board { .. } => {
                eyre::eyre!("can't figure out what repo to access - run this command from within a git repository with a remote")
            }
            List { .. } | Create { .. } => {
                eyre::eyre!("can't figure out what repo to access, try specifying with `--repo`")
            }
            View { id: project, .. }
            | EditProject { project, .. }
            | DeleteProject { project, .. }
            | Close { project }
            | Open { project }
            | Browse { id: project, .. } => eyre::eyre!(
                "can't figure out what repo to access, try specifying with `--repo` or `{{owner}}/{{repo}}:{{project}}`"
            ),
            Column(cmd) => cmd.no_repo_error(),
            Card(cmd) => cmd.no_repo_error(),
        }
    }
}

impl ColumnCommand {
    async fn run(self, repo: Option<&RepoName>, api: &Forgejo) -> eyre::Result<()> {
        use ColumnCommand::*;
        match self {
            List { project } => {
                if let Some(project_arg) = project {
                    let project_id = ProjectId::from_str(&project_arg)?;
                    if project_id.is_org_project() {
                        let resolved_id = project_id.resolve_org(api).await?;
                        let org_name = project_id.org_name().ok_or_else(|| eyre::eyre!("Failed to get org name"))?;
                        list_org_columns(&org_name, api, resolved_id).await?
                    } else {
                        // Regular repository project - extract repo info from ProjectId if available
                        if let Some(repo_arg) = &project_id.repo {
                            // Convert RepoArg to string and parse owner/name
                            let repo_str = repo_arg.to_string();
                            let (owner, name) = repo_str.split_once('/').ok_or_else(|| eyre::eyre!("Invalid repo format"))?;
                            let repo_name = crate::repo::RepoName::new(owner.to_string(), name.to_string());
                            let resolved_id = project_id.resolve(&repo_name, api).await?;
                            list_columns(&repo_name, api, resolved_id).await?
                        } else {
                            let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required for repo projects without explicit repo specification"))?;
                            let resolved_id = project_id.resolve(repo, api).await?;
                            list_columns(repo, api, resolved_id).await?
                        }
                    }
                } else {
                    // No project specified, need repo context and config
                    let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required when no project specified"))?;
                    let project_id = ProjectId::from_args_or_config(None, repo, api).await?;
                    list_columns(repo, api, project_id).await?
                }
            },
            Create { project, title, color } => {
                let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required for creating columns"))?;
                let project_id = project.resolve(repo, api).await?;
                create_column(repo, api, project_id, title, color).await?
            },
            Edit { project, column, command } => {
                let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required for editing columns"))?;
                let project_id = project.resolve(repo, api).await?;
                match command {
                    EditColumnCommand::Title { new_title } => {
                        edit_column_title(repo, api, project_id, column, new_title).await?
                    }
                    EditColumnCommand::Color { new_color } => {
                        edit_column_color(repo, api, project_id, column, new_color).await?
                    }
                }
            },
            Delete { project, column, force } => {
                let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required for deleting columns"))?;
                let project_id = project.resolve(repo, api).await?;
                delete_column(repo, api, project_id, column, force).await?
            },
            Move { project, column, position } => {
                let repo = repo.ok_or_else(|| eyre::eyre!("Repository context required for moving columns"))?;
                let project_id = project.resolve(repo, api).await?;
                move_column(repo, api, project_id, column, position).await?
            },
        }
        Ok(())
    }

    fn repo(&self) -> Option<&RepoArg> {
        use ColumnCommand::*;
        match self {
            List { project: _ } => None, // List uses optional String, no repo info
            Create { project, .. }
            | Edit { project, .. }
            | Delete { project, .. }
            | Move { project, .. } => project.repo.as_ref(),
        }
    }

    fn no_repo_error(&self) -> eyre::Error {
        eyre::eyre!("can't figure out what repo to access")
    }
}

impl CardCommand {
    async fn run(self, repo: &RepoName, api: &Forgejo) -> eyre::Result<()> {
        use CardCommand::*;
        match self {
            List { project, column } => {
                let project_id = match project {
                    Some(project_arg) => {
                        let parsed_project = ProjectId::from_str(&project_arg)?;
                        parsed_project.resolve(repo, api).await?
                    },
                    None => ProjectId::from_args_or_config(None, repo, api).await?
                };
                list_cards(repo, api, project_id, column).await?
            },
            Add { project, issue, column, position } => {
                let project_id = project.resolve(repo, api).await?;
                add_card(repo, api, project_id, issue, column, position).await?
            },
            Remove { project, issue } => {
                let project_id = project.resolve(repo, api).await?;
                remove_card(repo, api, project_id, issue).await?
            },
            Move { project, issue, column, position } => {
                let project_id = match project {
                    Some(proj) => proj.resolve(repo, api).await?,
                    None => ProjectId::from_args_or_config(None, repo, api).await?,
                };
                move_card(repo, api, project_id, issue.number(), column, position).await?
            },
        }
        Ok(())
    }

    fn repo(&self) -> Option<&RepoArg> {
        use CardCommand::*;
        match self {
            List { project: _, .. } => None, // List uses optional String, no repo info
            Add { project, .. }
            | Remove { project, .. } => project.repo.as_ref(),
            Move { project, .. } => project.as_ref().and_then(|p| p.repo.as_ref()),
        }
    }

    fn no_repo_error(&self) -> eyre::Error {
        eyre::eyre!("can't figure out what repo to access")
    }
}

async fn list_org_projects(org: &str, api: &Forgejo, state: Option<StateFilter>) -> eyre::Result<()> {
    let projects = api.org_list_projects(
        org,
        OrgListProjectsQuery {
            state: match state {
                Some(StateFilter::Open) => Some(OrgListProjectsQueryState::Open),
                Some(StateFilter::Closed) => Some(OrgListProjectsQueryState::Closed),
                Some(StateFilter::All) => Some(OrgListProjectsQueryState::All),
                None => None,
            },
            q: None,
            sort: None,
            since: None,
            before: None,
            created_by: None,
        },
    ).await?;

    if projects.is_empty() {
        println!("No projects found in organization {}", org);
        return Ok(());
    }

    for project in projects {
        let state = match project.state {
            Some(StateType::Open) => "open",
            Some(StateType::Closed) => "closed",
            Some(_) => "unknown",
            None => "unknown",
        };
        println!(
            "#{} {} [{}] ({})",
            project.id.unwrap_or(0),
            project.title.as_deref().unwrap_or("<no title>"),
            state,
            project.updated_at.as_ref().map(|t| t.date().to_string()).unwrap_or_default()
        );
        if let Some(body) = &project.body {
            if !body.trim().is_empty() {
                println!("  {}", body.lines().next().unwrap_or("").trim());
            }
        }
    }
    Ok(())
}

async fn list_projects(repo: &RepoName, api: &Forgejo, state: Option<StateFilter>) -> eyre::Result<()> {
    let projects = api.repo_list_projects(
        repo.owner(),
        repo.name(),
        RepoListProjectsQuery {
            state: match state {
                Some(StateFilter::Open) => Some(RepoListProjectsQueryState::Open),
                Some(StateFilter::Closed) => Some(RepoListProjectsQueryState::Closed),
                Some(StateFilter::All) => Some(RepoListProjectsQueryState::All),
                None => None,
            },
            q: None,
            sort: None,
            since: None,
            before: None,
            created_by: None,
        },
    ).await?;

    if projects.is_empty() {
        println!("No projects found");
        return Ok(());
    }

    for project in projects {
        let state = match project.state {
            Some(StateType::Open) => "open",
            Some(StateType::Closed) => "closed",
            Some(_) => "unknown",
            None => "unknown",
        };
        println!(
            "#{} {} [{}] ({})",
            project.id.unwrap_or(0),
            project.title.as_deref().unwrap_or("<no title>"),
            state,
            project.updated_at.as_ref().map(|t| t.date().to_string()).unwrap_or_default()
        );
        if let Some(body) = &project.body {
            if !body.trim().is_empty() {
                println!("  {}", body.lines().next().unwrap_or("").trim());
            }
        }
    }
    Ok(())
}

async fn create_project(
    repo: &RepoName,
    api: &Forgejo,
    title: Option<String>,
    body: Option<String>,
    template: Option<String>,
    web: bool,
) -> eyre::Result<()> {
    match (title, web) {
        (Some(title), false) => {
            let body = match body {
                Some(body) => body,
                None => {
                    let mut body = String::new();
                    crate::editor(&mut body, Some("md")).await?;
                    body
                }
            };

            let project = api.repo_create_project(
                repo.owner(),
                repo.name(),
                CreateProjectOption {
                    title: title.clone(),
                    body: Some(body.clone()),
                    template_type: template.as_ref().and_then(|t| t.parse().ok()),
                },
            ).await?;

            let id = project.id.unwrap_or(0);
            println!("Created project #{}: {}", id, title);
        }
        (None, true) => {
            let base_repo = api.repo_get(repo.owner(), repo.name()).await?;
            let mut project_create_url = base_repo
                .html_url
                .clone()
                .ok_or_eyre("repo does not have html url")?;
            project_create_url
                .path_segments_mut()
                .expect("invalid url")
                .extend(["projects", "new"]);
            open::that_detached(project_create_url.as_str()).wrap_err("Failed to open URL")?;
        }
        (None, false) => {
            eyre::bail!("requires either project title or --web flag")
        }
        (Some(_), true) => {
            eyre::bail!("cannot specify both title and --web flag")
        }
    }
    Ok(())
}

async fn view_project(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    let project = api.repo_get_project(repo.owner(), repo.name(), &id.to_string()).await?;

    let state = match project.state {
        Some(StateType::Open) => "open",
        Some(StateType::Closed) => "closed",
        Some(_) => "unknown",
        None => "unknown",
    };

    println!("Project #{}: {}", project.id.unwrap_or(0), project.title.as_deref().unwrap_or("<no title>"));
    println!("State: {}", state);
    if let Some(created_at) = &project.created_at {
        println!("Created: {}", created_at);
    }
    if let Some(updated_at) = &project.updated_at {
        println!("Updated: {}", updated_at);
    }
    if let Some(body) = &project.body {
        if !body.trim().is_empty() {
            println!();
            println!("{}", crate::markdown(body));
        }
    }
    Ok(())
}

async fn view_project_columns(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &id.to_string(),
    ).await?;

    if columns.is_empty() {
        println!("No columns found in project #{}", id);
        return Ok(());
    }

    println!("Columns for project #{}:", id);
    for (i, column) in columns.iter().enumerate() {
        println!(
            "  {}. {} (id: {})",
            i + 1,
            column.title.as_deref().unwrap_or("<no title>"),
            column.id.unwrap_or(0)
        );
        if let Some(color) = &column.color {
            println!("     Color: {}", color);
        }
    }
    Ok(())
}

async fn view_project_board(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    let project = api.repo_get_project(repo.owner(), repo.name(), &id.to_string()).await?;
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &id.to_string(),
    ).await?;

    println!("Board for project: {}", project.title.as_deref().unwrap_or("<no title>"));
    println!();

    if columns.is_empty() {
        println!("No columns in this project.");
        return Ok(());
    }

    for column in columns {
        println!("Column: {} (id: {})",
            column.title.as_deref().unwrap_or("<no title>"),
            column.id.unwrap_or(0)
        );
        println!("─────────────────────────");

        // Note: To display cards, we'd need to implement the card listing functionality
        // For now, show placeholder
        println!("  [Cards will be shown here when card functionality is implemented]");
        println!();
    }
    Ok(())
}

async fn edit_project_title(repo: &RepoName, api: &Forgejo, id: u64, new_title: Option<String>) -> eyre::Result<()> {
    let new_title = match new_title {
        Some(title) => title,
        None => {
            let title = crate::readline("Enter new title: ").await?;
            title.trim().to_string()
        }
    };

    if new_title.is_empty() {
        eyre::bail!("Title cannot be empty");
    }

    api.repo_edit_project(
        repo.owner(),
        repo.name(),
        &id.to_string(),
        EditProjectOption {
            title: Some(new_title.clone()),
            body: None,
            state: None,
        },
    ).await?;

    println!("Updated project #{} title to: {}", id, new_title);
    Ok(())
}

async fn edit_project_body(repo: &RepoName, api: &Forgejo, id: u64, new_body: Option<String>) -> eyre::Result<()> {
    let new_body = match new_body {
        Some(body) => body,
        None => {
            let current = api.repo_get_project(repo.owner(), repo.name(), &id.to_string()).await?;
            let mut body = current.body.unwrap_or_default();
            crate::editor(&mut body, Some("md")).await?;
            body
        }
    };

    api.repo_edit_project(
        repo.owner(),
        repo.name(),
        &id.to_string(),
        EditProjectOption {
            title: None,
            body: Some(new_body),
            state: None,
        },
    ).await?;

    println!("Updated project #{} body", id);
    Ok(())
}

async fn delete_project(repo: &RepoName, api: &Forgejo, id: u64, force: bool) -> eyre::Result<()> {
    if !force {
        let project = api.repo_get_project(repo.owner(), repo.name(), &id.to_string()).await?;
        let title = project.title.as_deref().unwrap_or("<no title>");
        let input = crate::readline(&format!("Are you sure you want to delete project #{} '{}'? (y/N): ", id, title)).await?;
        let confirmation = input.trim().to_lowercase();

        if confirmation != "y" && confirmation != "yes" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    api.repo_delete_project(repo.owner(), repo.name(), &id.to_string()).await?;
    println!("Deleted project #{}", id);
    Ok(())
}

async fn close_project(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    api.repo_edit_project(
        repo.owner(),
        repo.name(),
        &id.to_string(),
        EditProjectOption {
            title: None,
            body: None,
            state: Some(StateType::Closed),
        },
    ).await?;
    println!("Closed project #{}", id);
    Ok(())
}

async fn open_project(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    api.repo_edit_project(
        repo.owner(),
        repo.name(),
        &id.to_string(),
        EditProjectOption {
            title: None,
            body: None,
            state: Some(StateType::Open),
        },
    ).await?;
    println!("Opened project #{}", id);
    Ok(())
}

async fn browse_project(repo: &RepoName, api: &Forgejo, id: u64) -> eyre::Result<()> {
    let base_repo = api.repo_get(repo.owner(), repo.name()).await?;
    let mut project_url = base_repo
        .html_url
        .clone()
        .ok_or_eyre("repo does not have html url")?;
    project_url
        .path_segments_mut()
        .expect("invalid url")
        .extend(["projects", &id.to_string()]);
    open::that_detached(project_url.as_str()).wrap_err("Failed to open URL")?;
    println!("Opened project #{} in browser", id);
    Ok(())
}

// Column operations
async fn list_columns(repo: &RepoName, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    if columns.is_empty() {
        println!("No columns found in project #{}", project_id);
        return Ok(());
    }

    println!("Columns for project #{}:", project_id);
    for (i, column) in columns.iter().enumerate() {
        println!(
            "  {}. {} (id: {})",
            i + 1,
            column.title.as_deref().unwrap_or("<no title>"),
            column.id.unwrap_or(0)
        );
        if let Some(color) = &column.color {
            println!("     Color: {}", color);
        }
    }
    Ok(())
}

async fn list_org_columns(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    let columns = api.org_list_project_columns(org, &project_id.to_string()).await?;

    if columns.is_empty() {
        println!("No columns found in organization project #{}", project_id);
        return Ok(());
    }

    println!("Columns for organization project #{}:", project_id);
    for (i, column) in columns.iter().enumerate() {
        println!(
            "  {}. {} (id: {})",
            i + 1,
            column.title.as_deref().unwrap_or("<no title>"),
            column.id.unwrap_or(0)
        );
        if let Some(color) = &column.color {
            println!("     Color: {}", color);
        }
    }
    Ok(())
}

async fn create_column(repo: &RepoName, api: &Forgejo, project_id: u64, title: Option<String>, color: Option<String>) -> eyre::Result<()> {
    let title = match title {
        Some(title) => title,
        None => {
            let title = crate::readline("Enter column title: ").await?;
            title.trim().to_string()
        }
    };

    if title.is_empty() {
        eyre::bail!("Title cannot be empty");
    }

    let column = api.repo_create_project_column(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        CreateProjectColumnOption {
            title: title.clone(),
            color,
        },
    ).await?;

    let id = column.id.unwrap_or(0);
    println!("Created column #{}: {}", id, title);
    Ok(())
}

async fn edit_column_title(repo: &RepoName, api: &Forgejo, project_id: u64, column_id: u64, new_title: Option<String>) -> eyre::Result<()> {
    let new_title = match new_title {
        Some(title) => title,
        None => {
            let title = crate::readline("Enter new title: ").await?;
            title.trim().to_string()
        }
    };

    if new_title.is_empty() {
        eyre::bail!("Title cannot be empty");
    }

    api.repo_edit_project_column(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        column_id as i64,
        EditProjectColumnOption {
            title: Some(new_title.clone()),
            color: None,
        },
    ).await?;

    println!("Updated column #{} title to: {}", column_id, new_title);
    Ok(())
}

async fn edit_column_color(repo: &RepoName, api: &Forgejo, project_id: u64, column_id: u64, new_color: Option<String>) -> eyre::Result<()> {
    let new_color = match new_color {
        Some(color) => color,
        None => {
            let color = crate::readline("Enter new color (hex format, e.g. #ff0000): ").await?;
            color.trim().to_string()
        }
    };

    api.repo_edit_project_column(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        column_id as i64,
        EditProjectColumnOption {
            title: None,
            color: Some(new_color.clone()),
        },
    ).await?;

    println!("Updated column #{} color to: {}", column_id, new_color);
    Ok(())
}

async fn delete_column(repo: &RepoName, api: &Forgejo, project_id: u64, column_id: u64, force: bool) -> eyre::Result<()> {
    if !force {
        let input = crate::readline(&format!("Are you sure you want to delete column #{}? (y/N): ", column_id)).await?;
        let confirmation = input.trim().to_lowercase();

        if confirmation != "y" && confirmation != "yes" {
            println!("Cancelled.");
            return Ok(());
        }
    }

    api.repo_delete_project_column(repo.owner(), repo.name(), &project_id.to_string(), column_id as i64).await?;
    println!("Deleted column #{}", column_id);
    Ok(())
}

async fn move_column(repo: &RepoName, api: &Forgejo, project_id: u64, column_id: u64, position: u8) -> eyre::Result<()> {
    // Get current columns
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    // Verify the column exists
    let column = columns.iter()
        .find(|c| c.id == Some(column_id as i64))
        .ok_or_else(|| eyre::eyre!("Column {} not found in project", column_id))?;
    let column_name = column.title.clone().unwrap_or_default();

    // Build list of column IDs in current order
    let mut column_ids: Vec<i64> = columns.iter()
        .filter_map(|c| c.id)
        .collect();

    // Remove column from current position
    if let Some(pos) = column_ids.iter().position(|&id| id == column_id as i64) {
        column_ids.remove(pos);
    } else {
        return Err(eyre::eyre!("Column {} not found in project", column_id));
    }

    // Insert at new position (0-based internally, but user sees 1-based)
    let insert_pos = ((position as usize).saturating_sub(1)).min(column_ids.len());
    column_ids.insert(insert_pos, column_id as i64);

    // Build the move request with new positions
    let columns_with_positions: Vec<ColumnPosition> = column_ids.iter()
        .enumerate()
        .map(|(pos, &col_id)| ColumnPosition {
            column_id: col_id,
            position: pos as i64,
        })
        .collect();

    api.repo_move_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        MoveProjectColumnsOption { columns: columns_with_positions },
    ).await?;

    println!("Moved column \"{}\" to position {}", column_name, position);
    Ok(())
}

// Card operations
async fn list_cards(repo: &RepoName, api: &Forgejo, project_id: u64, column_id: Option<ColumnIdentifier>) -> eyre::Result<()> {
    match column_id {
        Some(col_id) => {
            // Resolve column identifier to numeric ID
            let column_number = match &col_id {
                ColumnIdentifier::Number(num) => *num,
                ColumnIdentifier::Name(name) => {
                    // Get all columns to find the one with matching name
                    let columns = api.repo_list_project_columns(
                        repo.owner(),
                        repo.name(),
                        &project_id.to_string(),
                    ).await?;

                    let matching_column = columns.iter()
                        .find(|col| col.title.as_deref() == Some(name))
                        .ok_or_else(|| eyre::eyre!("Column '{}' not found in project", name))?;

                    matching_column.id.ok_or_else(|| eyre::eyre!("Column has no ID"))? as u64
                }
            };

            // Note: For now, assume repository projects
            // TODO: Add support for organization project cards when we have org context
            let cards = api.repo_list_column_cards(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
                column_number as i64,
            ).await?;

            if cards.is_empty() {
                println!("No cards found in column '{}'", col_id);
                return Ok(());
            }

            println!("Cards in column '{}':", col_id);
            for card in cards {
                if let Some(issue) = &card.issue {
                    println!(
                        "  Card #{}: Issue #{} - {}",
                        card.id.unwrap_or(0),
                        issue.number.unwrap_or(0),
                        issue.title.as_deref().unwrap_or("<no title>")
                    );
                }
            }
        }
        None => {
            // Show all cards organized by columns
            let columns = api.repo_list_project_columns(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
            ).await?;

            if columns.is_empty() {
                println!("No columns found in project");
                return Ok(());
            }

            let mut has_any_cards = false;

            for column in &columns {
                if let Some(column_id) = column.id {
                    let cards = api.repo_list_column_cards(
                        repo.owner(),
                        repo.name(),
                        &project_id.to_string(),
                        column_id,
                    ).await?;

                    let column_title = column.title.as_deref().unwrap_or("<no title>");
                    println!("\n## {} (id: {})", column_title, column_id);

                    if cards.is_empty() {
                        println!("  (no cards)");
                    } else {
                        has_any_cards = true;
                        for card in &cards {
                            if let Some(issue) = &card.issue {
                                println!(
                                    "  Card #{}: Issue #{} - {}",
                                    card.id.unwrap_or(0),
                                    issue.number.unwrap_or(0),
                                    issue.title.as_deref().unwrap_or("<no title>")
                                );
                            }
                        }
                    }
                }
            }

            if !has_any_cards {
                println!("No cards found in any column");
            }
        }
    }
    Ok(())
}

async fn add_card(repo: &RepoName, api: &Forgejo, project_id: u64, issue: u64, column: Option<u64>, position: Option<u64>) -> eyre::Result<()> {
    let column_id = match column {
        Some(col) => col,
        None => {
            eyre::bail!("Column ID is required to add a card")
        }
    };

    let card = api.repo_create_card(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        column_id as i64,
        AddCardToColumnOption {
            issue_id: issue as i64,
            position: position.map(|p| p as i64),
        },
    ).await?;

    println!(
        "Added issue #{} as card #{} to column #{} in project #{}",
        issue,
        card.id.unwrap_or(0),
        column_id,
        project_id
    );
    Ok(())
}

async fn remove_card(repo: &RepoName, api: &Forgejo, project_id: u64, issue: u64) -> eyre::Result<()> {
    // Find the card by issue number
    let (card_id, _col_id, col_name) = find_card_by_issue(repo, api, project_id, issue).await?;

    // Delete the card
    api.repo_delete_card(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        card_id as i64,
    ).await?;

    println!("Removed issue #{} from column \"{}\"", issue, col_name);
    Ok(())
}

async fn move_card(repo: &RepoName, api: &Forgejo, project_id: u64, issue: u64, column: ColumnIdentifier, position: Option<u64>) -> eyre::Result<()> {
    // Resolve column identifier to numeric ID
    let column_number = match &column {
        ColumnIdentifier::Number(num) => *num,
        ColumnIdentifier::Name(name) => {
            // Get all columns to find the one with matching name
            let columns = api.repo_list_project_columns(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
            ).await?;

            let matching_column = columns.iter()
                .find(|col| col.title.as_deref() == Some(name))
                .ok_or_else(|| eyre::eyre!("Column '{}' not found in project", name))?;

            matching_column.id.ok_or_else(|| eyre::eyre!("Column has no ID"))? as u64
        }
    };

    // Note: The API requires card ID, not issue ID. We need to find the card for this issue.
    // Get all cards in all columns to find the card with the matching issue
    let all_columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    let mut card_id: Option<u64> = None;
    for col in &all_columns {
        if let Some(col_id) = col.id {
            let cards = api.repo_list_column_cards(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
                col_id,
            ).await?;

            for card in &cards {
                if let Some(card_issue) = &card.issue {
                    if card_issue.number == Some(issue as i64) {
                        card_id = card.id.map(|id| id as u64);
                        break;
                    }
                }
            }
            if card_id.is_some() {
                break;
            }
        }
    }

    let card_id = card_id.ok_or_else(|| eyre::eyre!("Issue #{} is not found in any project column", issue))?;

    // Move the card
    println!("Moving card #{} (issue #{}) to column '{}'", card_id, issue, column);

    use forgejo_api::structs::MoveProjectCardOption;
    let move_option = MoveProjectCardOption {
        column_id: Some(column_number as i64),
        position: position.map(|p| p as i64),
    };

    let result = api.repo_move_card(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
        card_id as i64,
        move_option,
    ).await;

    match result {
        Ok(_) => {
            println!("✅ Successfully moved card to column '{}'", column);
            if let Some(pos) = position {
                println!("   Position: {}", pos);
            }
        }
        Err(ForgejoError::ApiError(ref e)) if matches!(e.kind, forgejo_api::ApiErrorKind::NotFound { .. }) => {
            println!("❌ Card move API not available in this Forgejo instance");
            println!("   This feature requires a newer version of Forgejo that supports project card management");
            println!("   Card #{} (issue #{}) was found but could not be moved to column '{}'", card_id, issue, column);
        }
        Err(e) => return Err(e.into()),
    }

    Ok(())
}

// Organization project functions

async fn view_org_project(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    let project = api.org_get_project(org, &project_id.to_string()).await?;

    let state = match project.state {
        Some(StateType::Open) => "open",
        Some(StateType::Closed) => "closed",
        _ => "unknown",
    };

    println!("Project #{}: {}", project_id, project.title.as_deref().unwrap_or(""));
    println!("State: {}", state);
    if let Some(created_at) = project.created_at {
        println!("Created: {}", created_at);
    }
    if let Some(updated_at) = project.updated_at {
        println!("Updated: {}", updated_at);
    }
    if let Some(body) = project.body {
        if !body.trim().is_empty() {
            println!("Description: {}", body);
        }
    }

    Ok(())
}

async fn view_org_project_columns(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    let columns = api.org_list_project_columns(org, &project_id.to_string()).await?;

    println!("Columns in organization project #{}:", project_id);
    for column in columns {
        let card_count = column.card_count.unwrap_or(0);
        println!("  #{}: {} ({} cards)", column.id.unwrap_or(0), column.title.as_deref().unwrap_or(""), card_count);
    }

    Ok(())
}

async fn view_org_project_board(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    let columns = api.org_list_project_columns(org, &project_id.to_string()).await?;

    println!("Organization Project #{} Board:", project_id);
    for column in columns {
        let column_id = column.id.unwrap_or(0);
        let title = column.title.as_deref().unwrap_or("");
        println!("\n=== {} ===", title);

        // Get cards for this column
        let cards = api.org_list_column_cards(org, &project_id.to_string(), column_id).await?;

        if cards.is_empty() {
            println!("  (no cards)");
        } else {
            for card in cards {
                if let Some(issue) = card.issue {
                    println!("  • #{}: {}", issue.number.unwrap_or(0), issue.title.as_deref().unwrap_or(""));
                }
            }
        }
    }

    Ok(())
}

async fn edit_org_project_title(org: &str, api: &Forgejo, project_id: u64, new_title: Option<String>) -> eyre::Result<()> {
    let title = match new_title {
        Some(title) => title,
        None => crate::readline("new title: ").await?.trim().to_string(),
    };

    api.org_edit_project(org, &project_id.to_string(), EditProjectOption {
        title: Some(title),
        body: None,
        state: None,
    }).await?;

    println!("Project title updated");
    Ok(())
}

async fn edit_org_project_body(org: &str, api: &Forgejo, project_id: u64, new_body: Option<String>) -> eyre::Result<()> {
    let body = match new_body {
        Some(body) => body,
        None => crate::readline("new description: ").await?.trim().to_string(),
    };

    api.org_edit_project(org, &project_id.to_string(), EditProjectOption {
        title: None,
        body: Some(body),
        state: None,
    }).await?;

    println!("Project description updated");
    Ok(())
}

async fn delete_org_project(org: &str, api: &Forgejo, project_id: u64, force: bool) -> eyre::Result<()> {
    if !force {
        let confirm = crate::readline(&format!("Delete organization project #{}? [y/N]: ", project_id)).await?;
        if !confirm.trim().to_lowercase().starts_with('y') {
            println!("Cancelled");
            return Ok(());
        }
    }

    api.org_delete_project(org, &project_id.to_string()).await?;
    println!("Organization project #{} deleted", project_id);
    Ok(())
}

async fn close_org_project(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    api.org_edit_project(org, &project_id.to_string(), EditProjectOption {
        title: None,
        body: None,
        state: Some(StateType::Closed),
    }).await?;

    println!("Organization project #{} closed", project_id);
    Ok(())
}

async fn open_org_project(org: &str, api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    api.org_edit_project(org, &project_id.to_string(), EditProjectOption {
        title: None,
        body: None,
        state: Some(StateType::Open),
    }).await?;

    println!("Organization project #{} opened", project_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_project_id_parsing_org_format() {
        // Test simplified org format: "goats:3"
        let id = ProjectId::from_str("goats:3").unwrap();
        assert!(id.is_org_project());
        assert_eq!(id.org_name(), Some("goats".to_string()));
        match id.identifier {
            ProjectIdentifier::Number(3) => {},
            _ => panic!("Expected Number(3), got {:?}", id.identifier),
        }
    }

    #[test]
    fn test_project_id_parsing_repo_format() {
        // Test repo format: "myers/myers:1"
        let id = ProjectId::from_str("myers/myers:1").unwrap();
        assert!(!id.is_org_project());
        assert_eq!(id.org_name(), None);
        match id.identifier {
            ProjectIdentifier::Number(1) => {},
            _ => panic!("Expected Number(1), got {:?}", id.identifier),
        }
    }

    #[test]
    fn test_project_id_parsing_legacy_org_format() {
        // Test legacy org format: "goats/-:3"
        let id = ProjectId::from_str("goats/-:3").unwrap();
        assert!(id.is_org_project());
        assert_eq!(id.org_name(), Some("goats".to_string()));
        match id.identifier {
            ProjectIdentifier::Number(3) => {},
            _ => panic!("Expected Number(3), got {:?}", id.identifier),
        }
    }

    #[test]
    fn test_project_id_parsing_name_identifier() {
        // Test org format with name: "goats:my-project"
        let id = ProjectId::from_str("goats:my-project").unwrap();
        assert!(id.is_org_project());
        assert_eq!(id.org_name(), Some("goats".to_string()));
        match &id.identifier {
            ProjectIdentifier::Name(name) => assert_eq!(name, "my-project"),
            _ => panic!("Expected Name identifier, got {:?}", id.identifier),
        }
    }

    #[test]
    fn test_needs_repo_context_org_list() {
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::List {
                repo: None,
                org: Some("goats".to_string()),
                state: None
            }
        };
        assert!(!cmd.needs_repo_context());
    }

    #[test]
    fn test_needs_repo_context_repo_list() {
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::List {
                repo: None,
                org: None,
                state: None
            }
        };
        assert!(cmd.needs_repo_context());
    }

    #[test]
    fn test_org_name_extraction() {
        // Test different org name formats
        let test_cases = vec![
            ("goats:3", Some("goats")),
            ("my-org:project", Some("my-org")),
            ("org_with_underscores:1", Some("org_with_underscores")),
            ("goats/-:3", Some("goats")), // Legacy format
            ("myers/myers:1", None), // Repo format
        ];

        for (input, expected) in test_cases {
            let id = ProjectId::from_str(input).unwrap();
            assert_eq!(id.org_name(), expected.map(|s| s.to_string()),
                      "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_is_org_project_detection() {
        let test_cases = vec![
            ("goats:3", true),
            ("my-org:project", true),
            ("goats/-:3", true), // Legacy format
            ("myers/myers:1", false), // Repo format
        ];

        for (input, expected) in test_cases {
            let id = ProjectId::from_str(input).unwrap();
            assert_eq!(id.is_org_project(), expected,
                      "Failed for input: {}", input);
        }
    }

    #[test]
    fn test_project_id_error_cases() {
        // Test empty string - this actually succeeds and creates a Name("") identifier
        let id = ProjectId::from_str("").unwrap();
        match &id.identifier {
            ProjectIdentifier::Name(name) => assert_eq!(name, ""),
            _ => panic!("Expected empty name"),
        }

        // Test invalid repo format - this actually succeeds because RepoArg parsing is lenient
        // The RepoArg parser takes "owner/" and treats it as owner="owner", name=""
        let id = ProjectId::from_str("owner/:1").unwrap();
        assert!(!id.is_org_project()); // Should be a repo project

        // Test colon but no identifier - this actually creates an empty name too
        let id = ProjectId::from_str("goats:").unwrap();
        assert!(id.is_org_project());
        match &id.identifier {
            ProjectIdentifier::Name(name) => assert_eq!(name, ""),
            _ => panic!("Expected empty name"),
        }
    }

    #[test]
    fn test_project_id_edge_cases() {
        // Test numeric org name (should still work)
        let id = ProjectId::from_str("123:project").unwrap();
        assert!(id.is_org_project());
        assert_eq!(id.org_name(), Some("123".to_string()));

        // Test org name with hyphens and underscores
        let id = ProjectId::from_str("my-org_2023:1").unwrap();
        assert!(id.is_org_project());
        assert_eq!(id.org_name(), Some("my-org_2023".to_string()));

        // Test repo with host
        let id = ProjectId::from_str("github.com/owner/repo:1").unwrap();
        assert!(!id.is_org_project());
        assert_eq!(id.org_name(), None);
    }

    #[test]
    fn test_needs_repo_context_view_commands() {
        // Org project view should not need repo context
        let org_project = ProjectId::from_str("goats:3").unwrap();
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::View {
                id: org_project,
                command: None
            }
        };
        assert!(!cmd.needs_repo_context());

        // Repo project with full spec should not need repo context
        let repo_project = ProjectId::from_str("myers/myers:1").unwrap();
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::View {
                id: repo_project,
                command: None
            }
        };
        assert!(!cmd.needs_repo_context());

        // Project with just ID should need repo context
        let bare_project = ProjectId::from_str("1").unwrap();
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::View {
                id: bare_project,
                command: None
            }
        };
        assert!(cmd.needs_repo_context());
    }

    #[test]
    fn test_needs_repo_context_create_commands() {
        // Create should always need repo context (for now)
        let cmd = ProjectCommand {
            remote: None,
            command: ProjectSubcommand::Create {
                repo: None,
                title: Some("Test".to_string()),
                body: None,
                template: None,
                web: false
            }
        };
        assert!(cmd.needs_repo_context());
    }

    #[test]
    fn test_project_identifier_types() {
        // Test numeric identifier
        let id = ProjectId::from_str("goats:42").unwrap();
        match id.identifier {
            ProjectIdentifier::Number(42) => {},
            _ => panic!("Expected numeric identifier"),
        }

        // Test name identifier
        let id = ProjectId::from_str("goats:my-project-name").unwrap();
        match &id.identifier {
            ProjectIdentifier::Name(name) => assert_eq!(name, "my-project-name"),
            _ => panic!("Expected name identifier"),
        }

        // Test that numbers are parsed as numbers, not names
        let id = ProjectId::from_str("goats:0").unwrap();
        match id.identifier {
            ProjectIdentifier::Number(0) => {},
            _ => panic!("Expected zero to be parsed as number"),
        }
    }
}

async fn browse_org_project(org: &str, _api: &Forgejo, project_id: u64) -> eyre::Result<()> {
    // For now, assume HTTPS and localhost - we could make this configurable later
    let url = format!("https://localhost/{}/projects/{}", org, project_id);

    if let Err(e) = open::that(&url) {
        eprintln!("Failed to open browser: {}", e);
        println!("URL: {}", url);
    }

    Ok(())
}

// ========== SIMPLIFIED UX HELPER FUNCTIONS ==========

/// Auto-select a project when only one exists, or use git config default
async fn auto_select_project(
    repo: &RepoName,
    api: &Forgejo,
    specified: Option<String>,
) -> eyre::Result<(u64, String)> {
    // First check if explicit argument provided
    if let Some(project_name) = specified {
        return find_project_by_name(repo, api, &project_name).await;
    }

    // Check git config for default
    if let Ok(Some(default)) = crate::config::get_default_project() {
        // Try to parse as project name first
        return find_project_by_name(repo, api, &default).await;
    }

    // Auto-select if only one project exists
    let projects = check_project_api_available(api.repo_list_projects(
        repo.owner(),
        repo.name(),
        RepoListProjectsQuery {
            state: Some(RepoListProjectsQueryState::Open),
            q: None,
            sort: None,
            since: None,
            before: None,
            created_by: None,
        },
    ).await)?;

    match projects.len() {
        0 => Err(eyre::eyre!("No projects found in this repository")),
        1 => {
            let project = &projects[0];
            let id = project.id.ok_or_else(|| eyre::eyre!("Project has no ID"))? as u64;
            let title = project.title.clone().unwrap_or_default();
            Ok((id, title))
        }
        _ => {
            let mut msg = String::from("Multiple projects found in this repository:\n");
            for p in &projects {
                msg.push_str(&format!("  - {}\n", p.title.as_deref().unwrap_or("<no title>")));
            }
            msg.push_str("\nSpecify which project with --project, or set a default:\n");
            msg.push_str("  git config fj.defaultProject \"Project Name\"");
            Err(eyre::eyre!(msg))
        }
    }
}

/// Find a project by name
async fn find_project_by_name(
    repo: &RepoName,
    api: &Forgejo,
    name: &str,
) -> eyre::Result<(u64, String)> {
    let projects = check_project_api_available(api.repo_list_projects(
        repo.owner(),
        repo.name(),
        RepoListProjectsQuery {
            state: Some(RepoListProjectsQueryState::Open),
            q: None,
            sort: None,
            since: None,
            before: None,
            created_by: None,
        },
    ).await)?;

    // Case-insensitive search
    for project in &projects {
        if let Some(title) = &project.title {
            if title.eq_ignore_ascii_case(name) {
                let id = project.id.ok_or_else(|| eyre::eyre!("Project has no ID"))? as u64;
                return Ok((id, title.clone()));
            }
        }
    }

    // Provide helpful error with available projects
    let mut msg = format!("Project '{}' not found\n\nAvailable projects:\n", name);
    for p in &projects {
        msg.push_str(&format!("  - {}\n", p.title.as_deref().unwrap_or("<no title>")));
    }
    Err(eyre::eyre!(msg))
}

/// Find a column by name (case-insensitive)
async fn find_column_by_name(
    repo: &RepoName,
    api: &Forgejo,
    project_id: u64,
    column_name: &str,
) -> eyre::Result<(u64, String)> {
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    // Case-insensitive search
    for column in &columns {
        if let Some(title) = &column.title {
            if title.eq_ignore_ascii_case(column_name) {
                let id = column.id.ok_or_else(|| eyre::eyre!("Column has no ID"))? as u64;
                return Ok((id, title.clone()));
            }
        }
    }

    // Provide helpful error with available columns and fuzzy match suggestion
    let mut msg = format!("Column '{}' not found in project\n\nAvailable columns:\n", column_name);
    let mut best_match: Option<(&str, usize)> = None;

    for col in &columns {
        let title = col.title.as_deref().unwrap_or("<no title>");
        msg.push_str(&format!("  - {}\n", title));

        // Simple fuzzy match: count matching characters
        let similarity = column_name.chars()
            .filter(|c| title.to_lowercase().contains(c.to_ascii_lowercase()))
            .count();
        if let Some((_, best_score)) = best_match {
            if similarity > best_score {
                best_match = Some((title, similarity));
            }
        } else if similarity > 0 {
            best_match = Some((title, similarity));
        }
    }

    if let Some((suggested, _)) = best_match {
        msg.push_str(&format!("\nDid you mean \"{}\"?", suggested));
    }

    Err(eyre::eyre!(msg))
}

/// Find a card by issue number in any column of a project
async fn find_card_by_issue(
    repo: &RepoName,
    api: &Forgejo,
    project_id: u64,
    issue_number: u64,
) -> eyre::Result<(u64, u64, String)> {  // Returns (card_id, column_id, column_name)
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    for column in &columns {
        if let Some(col_id) = column.id {
            let cards = api.repo_list_column_cards(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
                col_id,
            ).await?;

            for card in &cards {
                if let Some(issue) = &card.issue {
                    if issue.number == Some(issue_number as i64) {
                        let card_id = card.id.ok_or_else(|| eyre::eyre!("Card has no ID"))? as u64;
                        let col_name = column.title.clone().unwrap_or_default();
                        return Ok((card_id, col_id as u64, col_name));
                    }
                }
            }
        }
    }

    Err(eyre::eyre!("Issue #{} is not in project\n\nTo add it: fj project add {} \"<column>\"", issue_number, issue_number))
}

/// Display the project board with clean output (no card IDs)
async fn display_board(
    repo: &RepoName,
    api: &Forgejo,
    project_id: u64,
    project_name: &str,
    verbose: bool,
    filter_column: Option<&str>,
) -> eyre::Result<()> {
    let columns = api.repo_list_project_columns(
        repo.owner(),
        repo.name(),
        &project_id.to_string(),
    ).await?;

    println!("Project: {}\n", project_name);

    if columns.is_empty() {
        println!("No columns in this project.");
        return Ok(());
    }

    // Filter columns if requested
    let columns_to_show: Vec<_> = if let Some(filter) = filter_column {
        let filtered: Vec<_> = columns.into_iter()
            .filter(|c| c.title.as_deref()
                .map(|t| t.eq_ignore_ascii_case(filter))
                .unwrap_or(false))
            .collect();

        if filtered.is_empty() {
            // Collect all column names for error message
            let all_columns = api.repo_list_project_columns(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
            ).await?;
            let available: Vec<_> = all_columns.iter()
                .filter_map(|c| c.title.as_deref())
                .collect();
            return Err(eyre::eyre!("Column '{}' not found in project\n\nAvailable columns:\n  - {}", filter, available.join("\n  - ")));
        }
        filtered
    } else {
        columns
    };

    for column in &columns_to_show {
        let column_title = column.title.as_deref().unwrap_or("<no title>");

        if let Some(col_id) = column.id {
            let cards = api.repo_list_column_cards(
                repo.owner(),
                repo.name(),
                &project_id.to_string(),
                col_id,
            ).await?;

            // Show column header with count if verbose
            if verbose {
                let count = cards.len();
                let label = if count == 1 { "issue" } else { "issues" };
                println!("## {} ({} {})", column_title, count, label);
            } else {
                println!("## {}", column_title);
            }

            if cards.is_empty() {
                println!("  (empty)\n");
            } else {
                for card in &cards {
                    if let Some(issue) = &card.issue {
                        let issue_num = issue.number.unwrap_or(0);
                        let issue_title = issue.title.as_deref().unwrap_or("<no title>");

                        if verbose {
                            print_verbose_card(issue_num, issue_title, issue);
                        } else {
                            let assignee_str = match &issue.assignees {
                                Some(assignees) if !assignees.is_empty() => {
                                    let names: Vec<_> = assignees.iter()
                                        .filter_map(|a| a.login.as_deref())
                                        .map(|n| format!("@{}", n))
                                        .collect();
                                    if names.is_empty() {
                                        "(unassigned)".to_string()
                                    } else {
                                        names.join(",")
                                    }
                                }
                                _ => "(unassigned)".to_string(),
                            };
                            println!("  #{:<4} {}  {}", issue_num, issue_title, assignee_str);
                        }
                    }
                }
                println!();
            }
        }
    }

    Ok(())
}

/// Check if an error indicates the project API is not available on this instance.
/// Returns a more helpful error message if so.
fn check_project_api_available<T>(result: Result<T, ForgejoError>) -> eyre::Result<T> {
    match result {
        Ok(val) => Ok(val),
        Err(ForgejoError::ApiError(ref e)) if matches!(e.kind, forgejo_api::ApiErrorKind::NotFound { .. }) => {
            Err(eyre::eyre!(
                "Project board API not available on this Forgejo instance.\n\n\
                This feature requires Forgejo with project board API support.\n\
                Check if your instance has been updated to include project management endpoints."
            ))
        }
        Err(e) => Err(e.into()),
    }
}

/// Print a card with verbose details (assignees, labels, due date)
fn print_verbose_card(issue_num: i64, title: &str, issue: &Issue) {
    // Format: "#1   title                    @assignee  label1,label2  due:YYYY-MM-DD"

    // Truncate title to fit nicely
    let max_title_len = 35;
    let display_title = if title.len() > max_title_len {
        format!("{}...", &title[..max_title_len-3])
    } else {
        title.to_string()
    };

    // Build metadata parts
    let mut parts = Vec::new();

    // Assignees
    if let Some(assignees) = &issue.assignees {
        if !assignees.is_empty() {
            let names: Vec<_> = assignees.iter()
                .filter_map(|a| a.login.as_deref())
                .map(|n| format!("@{}", n))
                .collect();
            if !names.is_empty() {
                parts.push(names.join(","));
            }
        }
    }

    // Labels
    if let Some(labels) = &issue.labels {
        if !labels.is_empty() {
            let label_names: Vec<_> = labels.iter()
                .filter_map(|l| l.name.as_deref())
                .collect();
            if !label_names.is_empty() {
                parts.push(label_names.join(","));
            }
        }
    }

    // Due date
    if let Some(due) = &issue.due_date {
        if let Ok(format_desc) = time::format_description::parse("[year]-[month]-[day]") {
            if let Ok(formatted) = due.format(&format_desc) {
                parts.push(format!("due:{}", formatted));
            }
        }
    }

    // Print with alignment
    let metadata = parts.join("  ");
    if metadata.is_empty() {
        println!("  #{:<4} {}", issue_num, display_title);
    } else {
        println!("  #{:<4} {:<38} {}", issue_num, display_title, metadata);
    }
}