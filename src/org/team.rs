use std::collections::BTreeMap;

use clap::{Args, Subcommand};
use eyre::OptionExt;
use forgejo_api::{
    structs::{CreateTeamOption, EditTeamOption},
    Forgejo,
};
use futures::{future, TryStreamExt};

use crate::SpecialRender;

#[derive(Subcommand, Clone, Debug)]
pub enum TeamSubcommand {
    /// View all the teams in an organization
    List {
        /// The name of the organization to list the teams in.
        org: String,
    },
    /// View info about a single team
    View {
        /// The name of the organization the team is part of.
        org: String,
        /// The name of the new team
        name: String,
        #[clap(long, short = 'p')]
        list_permissions: bool,
    },
    /// Create a new team
    Create {
        /// The name of the organization to create the team in.
        org: String,
        /// The name of the new team
        ///
        /// This must only contain alphanumeric characters.
        name: String,
        #[clap(flatten)]
        flags: TeamCreateFlags,
        #[clap(flatten)]
        options: TeamOptions,
    },
    /// Edit a team's information and permissions
    Edit {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to edit
        name: String,
        /// Can members of this team to create repos in the organization?
        #[clap(long, short)]
        new_name: Option<String>,
        #[clap(flatten)]
        flags: TeamEditFlags,
        #[clap(flatten)]
        options: TeamOptions,
    },
    /// Delete a team from an organization.
    ///
    /// Note that this does NOT delete the repos the team has!
    Delete {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to delete
        name: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be deleted without actually deleting
        #[clap(long)]
        dry_run: bool,
    },
    #[clap(subcommand)]
    Repo(TeamRepoSubcommand),
    #[clap(subcommand)]
    Member(TeamMemberSubcommand),
}

#[derive(Args, Clone, Debug)]
pub struct TeamOptions {
    /// A description of what the team does.
    #[clap(long, short)]
    description: Option<String>,
    /// A comma-separated list of read permissions to give this team
    ///
    /// List of permissions:
    ///  - wiki
    ///  - ext_wiki
    ///  - issues
    ///  - ext_issues
    ///  - pulls
    ///  - projects
    ///  - actions
    ///  - code
    ///  - releases
    ///  - packages
    ///
    /// Alternatively, you can use `all` to allow every read permission.
    #[clap(long, short)]
    read_permissions: Option<String>,
    /// A comma-separated list of read+write permissions to give this team
    ///
    /// List of permissions:
    ///  - wiki
    ///  - ext_wiki
    ///  - issues
    ///  - ext_issues
    ///  - pulls
    ///  - projects
    ///  - actions
    ///  - code
    ///  - releases
    ///  - packages
    ///
    /// Alternatively, you can use `all` to allow every read+write permission
    #[clap(long, short)]
    write_permissions: Option<String>,
}

#[derive(Args, Clone, Debug)]
pub struct TeamCreateFlags {
    /// Allow members of this team to create repos in the organization.
    #[clap(long, short)]
    can_create_repos: bool,
    /// Give this team access to every repo.
    #[clap(long, short)]
    include_all_repos: bool,
    /// Give this team administrator abilities in the organization.
    #[clap(long, short = 'A')]
    admin: bool,
}

#[derive(Args, Clone, Debug)]
pub struct TeamEditFlags {
    /// Allow members of this team to create repos in the organization.
    #[clap(long, short)]
    can_create_repos: Option<bool>,
    /// Give this team access to every repo.
    #[clap(long, short)]
    include_all_repos: Option<bool>,
    /// Give this team administrator abilities in the organization.
    #[clap(long, short = 'A')]
    admin: Option<bool>,
}

impl TeamSubcommand {
    pub async fn run(self, api: &forgejo_api::Forgejo) -> eyre::Result<()> {
        match self {
            TeamSubcommand::List { org } => list_teams(&api, org).await?,
            TeamSubcommand::View {
                org,
                name,
                list_permissions,
            } => view_team(&api, org, name, list_permissions).await?,
            TeamSubcommand::Create {
                org,
                name,
                flags,
                options,
            } => create_team(&api, org, name, flags, options).await?,
            TeamSubcommand::Edit {
                org,
                name,
                new_name,
                flags,
                options,
            } => edit_team(&api, org, name, new_name, flags, options).await?,
            TeamSubcommand::Delete { org, name, force, dry_run } => {
                delete_team(&api, org, name, force, dry_run).await?
            }
            TeamSubcommand::Repo(subcommand) => subcommand.run(&api).await?,
            TeamSubcommand::Member(subcommand) => subcommand.run(&api).await?,
        }
        Ok(())
    }
}

async fn find_team_by_name(
    api: &Forgejo,
    org: &str,
    name: &str,
) -> eyre::Result<forgejo_api::structs::Team> {
    api.org_list_teams(&org)
        .stream()
        .try_filter(|team| {
            future::ready(
                team.name
                    .as_deref()
                    .is_some_and(|team_name| team_name == name),
            )
        })
        .try_next()
        .await?
        .ok_or_else(|| eyre::eyre!("Unknown team {name}"))
}

async fn list_teams(api: &Forgejo, org: String) -> eyre::Result<()> {
    let mut teams = api.org_list_teams(&org).all().await?;
    teams.sort_unstable_by_key(permission_sort_id);

    let SpecialRender {
        bright_blue,
        bold,
        reset,
        bullet,
        ..
    } = crate::special_render();
    for team in teams {
        let team_name = team.name.as_deref().ok_or_eyre("team does not have name")?;
        println!("{bullet} {bold}{bright_blue}{team_name}{reset}");
    }
    Ok(())
}

fn permission_sort_id(team: &forgejo_api::structs::Team) -> u32 {
    use forgejo_api::structs::TeamPermission as Perm;
    match &team.permission {
        Some(Perm::Owner) => 0,
        Some(Perm::Admin) => 1,
        Some(Perm::Write) => 2,
        Some(Perm::Read) => 3,
        Some(Perm::None) | None => 4,
    }
}

const ALL_UNITS: &[&str] = &[
    "repo.wiki",
    "repo.ext_wiki",
    "repo.issues",
    "repo.ext_issues",
    "repo.pulls",
    "repo.projects",
    "repo.actions",
    "repo.code",
    "repo.releases",
    "repo.packages",
];

async fn view_team(
    api: &Forgejo,
    org: String,
    name: String,
    list_permissions: bool,
) -> eyre::Result<()> {
    let team = find_team_by_name(api, &org, &name).await?;

    let SpecialRender {
        bright_blue,
        bright_red,
        bold,
        reset,
        dash,
        ..
    } = crate::special_render();

    print!("{bright_blue}{bold}{name}{reset} {dash} in org {bold}{org}{reset}");
    if team
        .permission
        .is_some_and(|p| p == forgejo_api::structs::TeamPermission::Admin)
    {
        print!(" {dash} {bright_red}Admin{reset}");
    }
    println!();

    if let Some(description) = &team.description {
        if !description.is_empty() {
            println!("\n{}", crate::markdown(description));
        }
    }

    if list_permissions {
        println!();
        let units = team
            .units_map
            .as_ref()
            .ok_or_eyre("team does not have permission units")?;
        let mut ro_perms = Vec::new();
        let mut rw_perms = Vec::new();
        for (unit, permission) in units {
            match &**permission {
                "read" => ro_perms.push(unit),
                "write" | "admin" | "owner" => rw_perms.push(unit),
                _ => (),
            }
        }

        let get_unit_name = |unit| match unit {
            "repo.wiki" => "Wikis",
            "repo.ext_wiki" => "External Wikis",
            "repo.issues" => "Issues",
            "repo.ext_issues" => "External Issues",
            "repo.pulls" => "Pull Requests",
            "repo.projects" => "Projects",
            "repo.actions" => "CI",
            "repo.code" => "Code",
            "repo.releases" => "Releases",
            "repo.packages" => "Packages",
            _ => "Unknown",
        };
        if !ro_perms.is_empty() {
            print!("Read Only: ");
            for (i, unit) in ro_perms.iter().enumerate() {
                let unit_name = get_unit_name(unit);
                if i > 0 {
                    print!(", ");
                }
                print!("{unit_name}");
            }
            println!();
        }
        if !rw_perms.is_empty() {
            print!("Read/Write: ");
            for (i, unit) in rw_perms.iter().enumerate() {
                let unit_name = get_unit_name(unit);
                if i != 0 {
                    print!(", ");
                }
                print!("{unit_name}");
            }
            println!();
        }
    }

    Ok(())
}

fn create_unit_map(ro_perms: Option<&str>, rw_perms: Option<&str>) -> BTreeMap<String, String> {
    let mut units = BTreeMap::new();
    if let Some(ro_perms) = ro_perms {
        if ro_perms == "all" {
            for ro in ALL_UNITS {
                units.insert(ro.to_string(), "read".to_owned());
            }
        } else {
            for ro in ro_perms.split(",") {
                units.insert(format!("repo.{ro}"), "read".to_owned());
            }
        }
    }
    if let Some(rw_perms) = rw_perms {
        if rw_perms.trim() == "all" {
            for rw in ALL_UNITS {
                units.insert(rw.to_string(), "write".to_owned());
            }
        } else {
            for rw in rw_perms.split(",") {
                units.insert(format!("repo.{rw}"), "write".to_owned());
            }
        }
    }
    units
}

async fn create_team(
    api: &Forgejo,
    org: String,
    name: String,
    flags: TeamCreateFlags,
    options: TeamOptions,
) -> eyre::Result<()> {
    let units = create_unit_map(
        options.read_permissions.as_deref(),
        options.write_permissions.as_deref(),
    );
    let opt = CreateTeamOption {
        can_create_org_repo: Some(flags.can_create_repos),
        description: options.description,
        includes_all_repositories: Some(flags.include_all_repos),
        name,
        permission: flags
            .admin
            .then(|| forgejo_api::structs::CreateTeamOptionPermission::Admin),
        units: None,
        units_map: Some(units),
    };
    let new_team = api.org_create_team(&org, opt).await?;
    let org = new_team.organization.ok_or_eyre("team doesn't have org")?;
    let org_name = org
        .name
        .or(org.full_name)
        .ok_or_eyre("org doesn't have name")?;
    let name = new_team.name.ok_or_eyre("team doesn't have name")?;

    let SpecialRender {
        bright_blue,
        bold,
        reset,
        ..
    } = crate::special_render();
    print!("created new ");
    if flags.admin {
        print!("admin ");
    }
    println!("team {bright_blue}{bold}{name}{reset} in {bold}{org_name}{reset}");
    Ok(())
}

async fn edit_team(
    api: &Forgejo,
    org: String,
    name: String,
    new_name: Option<String>,
    flags: TeamEditFlags,
    options: TeamOptions,
) -> eyre::Result<()> {
    let team = find_team_by_name(api, &org, &name).await?;
    let id = team.id.ok_or_eyre("team does not have id")?;

    // EditTeamOption's team field is a String rather than Option<String>
    // That should be fixed, but this gets around it for now.
    let new_name = new_name.unwrap_or(name);
    let units = create_unit_map(
        options.read_permissions.as_deref(),
        options.write_permissions.as_deref(),
    );

    let options = EditTeamOption {
        can_create_org_repo: flags.can_create_repos,
        description: options.description,
        includes_all_repositories: flags.include_all_repos,
        name: new_name,
        permission: flags
            .admin
            .and_then(|b| b.then(|| forgejo_api::structs::EditTeamOptionPermission::Admin)),
        units: None,
        units_map: Some(units),
    };
    api.org_edit_team(id, options).await?;

    Ok(())
}

async fn delete_team(
    api: &Forgejo,
    org: String,
    name: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    let team_full_name = format!("{}/{}", org, name);

    if dry_run {
        println!("Would delete team: {}", team_full_name);
        println!("\nRun with --force to actually delete.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Delete team {}?", team_full_name);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Deleting team: {}", team_full_name);
    let id = find_team_by_name(api, &org, &name)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    api.org_delete_team(id).await?;
    println!("Deleted team {}", team_full_name);
    Ok(())
}

#[derive(Subcommand, Clone, Debug)]
pub enum TeamRepoSubcommand {
    /// List all the repos this team can access
    List {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to view the repos of.
        team: String,
        /// Which page of the results to view
        #[clap(long, short, default_value_t = 1)]
        page: u32,
    },
    /// Add access to an existing repo to a team
    Add {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to add a repo to.
        team: String,
        /// The name of the repo to add to the team.
        repo: String,
    },
    /// Remove access to a repo from a team
    ///
    /// Note that this does NOT delete the repository!
    Rm {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to remove the repo from.
        team: String,
        /// The name of the repo to remove from the team.
        repo: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be done without actually doing it
        #[clap(long)]
        dry_run: bool,
    },
}

impl TeamRepoSubcommand {
    async fn run(self, api: &Forgejo) -> eyre::Result<()> {
        match self {
            TeamRepoSubcommand::List { org, team, page } => {
                list_team_repos(&api, org, team, page).await?
            }
            TeamRepoSubcommand::Add { org, team, repo } => {
                add_repo_to_team(&api, org, team, repo).await?
            }
            TeamRepoSubcommand::Rm { org, team, repo, force, dry_run } => {
                remove_repo_from_team(&api, org, team, repo, force, dry_run).await?
            }
        }
        Ok(())
    }
}

async fn list_team_repos(api: &Forgejo, org: String, team: String, page: u32) -> eyre::Result<()> {
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    let (headers, repos) = api.org_list_team_repos(id).page(page).page_size(20).await?;

    let SpecialRender { bullet, .. } = crate::special_render();
    if repos.is_empty() {
        println!("No results");
    } else {
        for repo in repos {
            let full_name = repo
                .full_name
                .as_deref()
                .ok_or_eyre("repo does not have full name")?;
            println!("{bullet} {full_name}");
        }
        let count = headers.x_total_count.unwrap_or_default();
        println!("Page {} of {}", page, (count as u64).div_ceil(20));
    }
    Ok(())
}

async fn add_repo_to_team(
    api: &Forgejo,
    org: String,
    team: String,
    repo: String,
) -> eyre::Result<()> {
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    api.org_add_team_repository(id, &org, &repo).await?;
    let SpecialRender {
        bold,
        reset,
        bright_blue,
        ..
    } = crate::special_render();
    println!("Added {bold}{org}/{repo}{reset} to team {bright_blue}{bold}{team}{reset}");
    Ok(())
}

async fn remove_repo_from_team(
    api: &Forgejo,
    org: String,
    team: String,
    repo: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if dry_run {
        println!("Would remove repo {}/{} from team {}", org, repo, team);
        println!("\nRun with --force to actually remove.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Remove repo {}/{} from team {}?", org, repo, team);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Removing repo {}/{} from team {}", org, repo, team);
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    api.org_remove_team_repository(id, &org, &repo).await?;
    let SpecialRender {
        bold,
        reset,
        bright_blue,
        ..
    } = crate::special_render();
    println!("Removed {bold}{org}/{repo}{reset} from team {bright_blue}{bold}{team}{reset}");
    Ok(())
}

#[derive(Subcommand, Clone, Debug)]
pub enum TeamMemberSubcommand {
    /// List all the members of a team
    List {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to view the members of.
        team: String,
        /// Which page of the results to view
        #[clap(long, short, default_value_t = 1)]
        page: u32,
    },
    /// Add someone to a team
    Add {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to add a user to.
        team: String,
        /// The name of the user to add to the team.
        user: String,
    },
    /// Remove someone from a team
    Rm {
        /// The name of the organization the team is in.
        org: String,
        /// The name of the team to remove the user from.
        team: String,
        /// The name of the user to remove from the team.
        user: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be done without actually doing it
        #[clap(long)]
        dry_run: bool,
    },
}

impl TeamMemberSubcommand {
    async fn run(self, api: &Forgejo) -> eyre::Result<()> {
        match self {
            TeamMemberSubcommand::List { org, team, page } => {
                list_team_members(&api, org, team, page).await?
            }
            TeamMemberSubcommand::Add { org, team, user } => {
                add_user_to_team(&api, org, team, user).await?
            }
            TeamMemberSubcommand::Rm { org, team, user, force, dry_run } => {
                remove_user_from_team(&api, org, team, user, force, dry_run).await?
            }
        }
        Ok(())
    }
}

async fn list_team_members(
    api: &Forgejo,
    org: String,
    team: String,
    page: u32,
) -> eyre::Result<()> {
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    let (headers, users) = api
        .org_list_team_members(id)
        .page(page)
        .page_size(20)
        .await?;

    let SpecialRender {
        bullet,
        light_grey,
        bright_cyan,
        reset,
        ..
    } = crate::special_render();
    if users.is_empty() {
        println!("No results");
    } else {
        for user in users {
            let username = user
                .login
                .as_deref()
                .ok_or_eyre("repo does not have full name")?;
            match user.full_name.as_deref().filter(|s| !s.is_empty()) {
                Some(full_name) => println!(
                    "{bullet} {bright_cyan}{full_name}{reset} {light_grey}({username}){reset}"
                ),
                None => println!("{bullet} {bright_cyan}{username}{reset}"),
            }
        }
        let count = headers.x_total_count.unwrap_or_default();
        println!("Page {} of {}", page, (count as u64).div_ceil(20));
    }
    Ok(())
}

async fn add_user_to_team(
    api: &Forgejo,
    org: String,
    team: String,
    user: String,
) -> eyre::Result<()> {
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    api.org_add_team_member(id, &user).await?;
    let SpecialRender {
        bold,
        reset,
        bright_blue,
        bright_cyan,
        ..
    } = crate::special_render();
    println!("Added {bright_cyan}{bold}{user}{reset} to team {bright_blue}{bold}{team}{reset}");
    Ok(())
}

async fn remove_user_from_team(
    api: &Forgejo,
    org: String,
    team: String,
    user: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if dry_run {
        println!("Would remove user {} from team {}", user, team);
        println!("\nRun with --force to actually remove.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Remove user {} from team {}?", user, team);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Removing user {} from team {}", user, team);
    let id = find_team_by_name(api, &org, &team)
        .await?
        .id
        .ok_or_eyre("team does not have id")?;
    api.org_remove_team_member(id, &user).await?;
    let SpecialRender {
        bold,
        reset,
        bright_blue,
        bright_cyan,
        ..
    } = crate::special_render();
    println!("Removed {bright_cyan}{bold}{user}{reset} from team {bright_blue}{bold}{team}{reset}");
    Ok(())
}
