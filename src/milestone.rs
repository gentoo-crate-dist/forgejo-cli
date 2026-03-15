use clap::{Args, Subcommand};
use eyre::OptionExt;
use forgejo_api::Forgejo;

use crate::{
    keys::KeyInfo,
    repo::{RepoArg, RepoInfo, RepoName},
};

#[derive(Args, Clone, Debug)]
pub struct MilestoneCommand {
    /// The local git remote that points to the repo to operate on
    #[clap(long, short = 'R', global = true)]
    remote: Option<String>,
    /// The name of the repository to operate on
    #[clap(long, short, global = true)]
    repo: Option<RepoArg>,
    #[clap(subcommand)]
    command: MilestoneSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum MilestoneSubcommand {
    /// List milestones on a repo
    List {
        /// Filter by state: open, closed, or all
        #[clap(long, short, default_value = "open")]
        state: String,
    },
    /// View a milestone's details
    View { name: String },
    /// Create a new milestone
    Create {
        title: String,
        /// Description of the milestone.
        ///
        /// Using this flag without an argument will open your editor.
        #[clap(long, short)]
        description: Option<Option<String>>,
        /// Due date in YYYY-MM-DD format
        #[clap(long)]
        due_date: Option<String>,
    },
    /// Edit an existing milestone
    Edit {
        name: String,
        /// New title for the milestone
        #[clap(long)]
        rename: Option<String>,
        /// New description.
        ///
        /// Using this flag without an argument will open your editor.
        #[clap(long, short)]
        description: Option<Option<String>>,
        /// New due date in YYYY-MM-DD format
        #[clap(long)]
        due_date: Option<String>,
        /// New state: open or closed
        #[clap(long)]
        state: Option<String>,
    },
    /// Delete a milestone
    Delete { name: String },
}

impl MilestoneCommand {
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
            MilestoneSubcommand::List { state } => list_milestones(repo, &api, &state).await?,
            MilestoneSubcommand::View { name } => view_milestone(repo, &api, &name).await?,
            MilestoneSubcommand::Create {
                title,
                description,
                due_date,
            } => create_milestone(repo, &api, title, description, due_date).await?,
            MilestoneSubcommand::Edit {
                name,
                rename,
                description,
                due_date,
                state,
            } => edit_milestone(repo, &api, &name, rename, description, due_date, state).await?,
            MilestoneSubcommand::Delete { name } => delete_milestone(repo, &api, &name).await?,
        }
        Ok(())
    }
}

async fn find_milestone(
    repo: &RepoName,
    api: &Forgejo,
    name: &str,
) -> eyre::Result<forgejo_api::structs::Milestone> {
    let query = forgejo_api::structs::IssueGetMilestonesListQuery {
        state: Some("all".into()),
        name: Some(name.into()),
    };
    let milestones = api
        .issue_get_milestones_list(repo.owner(), repo.name(), query)
        .all()
        .await?;
    milestones
        .into_iter()
        .find(|m| m.title.as_deref() == Some(name))
        .ok_or_eyre(format!("milestone '{name}' not found"))
}

fn parse_due_date(date: &str) -> eyre::Result<time::OffsetDateTime> {
    let format = time::macros::format_description!("[year]-[month]-[day]");
    let date = time::Date::parse(date, format)?;
    Ok(date.with_hms(0, 0, 0).unwrap().assume_utc())
}

async fn list_milestones(repo: &RepoName, api: &Forgejo, state: &str) -> eyre::Result<()> {
    let query = forgejo_api::structs::IssueGetMilestonesListQuery {
        state: Some(state.into()),
        name: None,
    };
    let milestones = api
        .issue_get_milestones_list(repo.owner(), repo.name(), query)
        .all()
        .await?;

    let crate::SpecialRender {
        bold,
        reset,
        bright_green,
        bright_red,
        dark_grey,
        ..
    } = *crate::special_render();

    for ms in milestones {
        let title = ms.title.as_deref().unwrap_or("<untitled>");
        let open = ms.open_issues.unwrap_or(0);
        let closed = ms.closed_issues.unwrap_or(0);
        let state_str = match ms.state {
            Some(forgejo_api::structs::StateType::Open) => {
                format!("{bright_green}open{reset}")
            }
            Some(forgejo_api::structs::StateType::Closed) => {
                format!("{bright_red}closed{reset}")
            }
            None => "unknown".into(),
        };
        let due = ms
            .due_on
            .map(|d| {
                let fmt = time::macros::format_description!("[year]-[month]-[day]");
                d.format(fmt).unwrap_or_default()
            })
            .unwrap_or_default();
        let due_part = if due.is_empty() {
            String::new()
        } else {
            format!(" {dark_grey}due {due}{reset}")
        };
        println!("{bold}{title}{reset} ({state_str}) {dark_grey}{open} open, {closed} closed{reset}{due_part}");
    }
    Ok(())
}

async fn view_milestone(repo: &RepoName, api: &Forgejo, name: &str) -> eyre::Result<()> {
    let ms = find_milestone(repo, api, name).await?;

    let crate::SpecialRender {
        bold,
        reset,
        bright_green,
        bright_red,
        dark_grey,
        ..
    } = *crate::special_render();

    let title = ms.title.as_deref().unwrap_or("<untitled>");
    let open = ms.open_issues.unwrap_or(0);
    let closed = ms.closed_issues.unwrap_or(0);
    let state_str = match ms.state {
        Some(forgejo_api::structs::StateType::Open) => format!("{bright_green}open{reset}"),
        Some(forgejo_api::structs::StateType::Closed) => format!("{bright_red}closed{reset}"),
        None => "unknown".into(),
    };
    let due = ms
        .due_on
        .map(|d| {
            let fmt = time::macros::format_description!("[year]-[month]-[day]");
            d.format(fmt).unwrap_or_default()
        })
        .unwrap_or_default();

    println!("{bold}{title}{reset}");
    println!("{state_str}");
    println!("{dark_grey}{open} open, {closed} closed{reset}");
    if !due.is_empty() {
        println!("{dark_grey}due {due}{reset}");
    }
    if let Some(desc) = &ms.description {
        if !desc.is_empty() {
            println!();
            println!("{}", crate::markdown(desc));
        }
    }
    Ok(())
}

async fn create_milestone(
    repo: &RepoName,
    api: &Forgejo,
    title: String,
    description: Option<Option<String>>,
    due_date: Option<String>,
) -> eyre::Result<()> {
    let description = match description {
        Some(Some(desc)) => Some(desc),
        Some(None) => {
            let mut s = String::new();
            crate::editor(&mut s, Some("md")).await?;
            Some(s)
        }
        None => None,
    };
    let due_on = due_date.map(|d| parse_due_date(&d)).transpose()?;

    let opt = forgejo_api::structs::CreateMilestoneOption {
        title: Some(title.clone()),
        description,
        due_on,
        state: None,
    };
    api.issue_create_milestone(repo.owner(), repo.name(), opt)
        .await?;
    println!("created milestone {title}");
    Ok(())
}

async fn edit_milestone(
    repo: &RepoName,
    api: &Forgejo,
    name: &str,
    rename: Option<String>,
    description: Option<Option<String>>,
    due_date: Option<String>,
    state: Option<String>,
) -> eyre::Result<()> {
    let ms = find_milestone(repo, api, name).await?;
    let id = ms.id.ok_or_eyre("milestone does not have id")?;

    let description = match description {
        Some(Some(desc)) => Some(desc),
        Some(None) => {
            let mut s = ms.description.unwrap_or_default();
            crate::editor(&mut s, Some("md")).await?;
            Some(s)
        }
        None => None,
    };
    let due_on = due_date.map(|d| parse_due_date(&d)).transpose()?;

    let opt = forgejo_api::structs::EditMilestoneOption {
        title: rename,
        description,
        due_on,
        state,
    };
    api.issue_edit_milestone(repo.owner(), repo.name(), id, opt)
        .await?;
    println!("edited milestone {name}");
    Ok(())
}

async fn delete_milestone(repo: &RepoName, api: &Forgejo, name: &str) -> eyre::Result<()> {
    let ms = find_milestone(repo, api, name).await?;
    let id = ms.id.ok_or_eyre("milestone does not have id")?;
    api.issue_delete_milestone(repo.owner(), repo.name(), id)
        .await?;
    println!("deleted milestone {name}");
    Ok(())
}
