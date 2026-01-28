use clap::{Args, Subcommand};
use eyre::OptionExt;
use forgejo_api::{
    structs::{
        CreateLabelOption, CreateOrgOption, EditLabelOption, EditOrgOption, OrgListLabelsQuery,
    },
    Forgejo,
};
use futures::{future, TryStreamExt};

use crate::{repo::RepoInfo, SpecialRender};

mod team;

#[derive(Args, Clone, Debug)]
pub struct OrgCommand {
    /// The local git remote that points to the repo to operate on.
    #[clap(long, short = 'R')]
    remote: Option<String>,
    #[clap(subcommand)]
    command: OrgSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum OrgSubcommand {
    /// List all organizations
    List {
        /// Which page of the results to view
        #[clap(long, short, default_value_t = 1)]
        page: u32,
        /// Only list organizations you are a member of.
        #[clap(long, short, conflicts_with = "page")]
        only_member_of: bool,
    },
    /// View info about an organization
    View {
        /// The name of the organization to view.
        name: String,
    },
    /// Create a new organization
    Create {
        /// The username for the organization.
        ///
        /// It can only have alphanumeric characters, dash, underscore, or period. It must start
        /// and end with an alphanumeric character, and can't have consecutive dashes, underscores,
        /// or periods.
        ///
        /// If you want a name that doesn't have these restrictions, see the `--full-name` option.
        name: String,
        #[clap(flatten)]
        options: OrgOptions,
    },
    /// Edit an organization's information.
    Edit {
        /// The name of the organization to edit.
        ///
        /// Note that this is the username, *not* the display name.
        name: String,
        #[clap(flatten)]
        options: OrgOptions,
    },
    /// View the activity in an organization
    Activity {
        /// The name of the organization to view activity for.
        name: String,
    },
    /// List the members of an organization
    Members {
        /// The name of the organization to view the members of.
        org: String,
        /// Which page of the results to view
        #[clap(long, short, default_value_t = 1)]
        page: u32,
    },
    /// View and change the visibility of your membership in an organization
    Visibility {
        /// The name of the organization to view your visibility in.
        org: String,
        /// Set a new visibility for yourself.
        #[clap(long, short)]
        set: Option<OrgMemberVisibility>,
    },
    #[clap(subcommand)]
    Team(team::TeamSubcommand),
    #[clap(subcommand)]
    Label(LabelSubcommand),
    #[clap(subcommand)]
    Repo(RepoSubcommand),
}

#[derive(Args, Clone, Debug)]
pub struct OrgOptions {
    /// The display name for the organization.
    ///
    /// This doesn't have the restrictions the `name` argument does, and can contain any UTF-8
    /// text.
    #[clap(long, short)]
    full_name: Option<String>,
    /// The organization's description
    #[clap(long, short)]
    description: Option<String>,
    /// Contact email for the organization
    #[clap(long, short)]
    email: Option<String>,
    /// The organizations's location
    #[clap(long, short)]
    location: Option<String>,
    /// The organization's website
    #[clap(long, short)]
    website: Option<String>,
    /// The visibility of the organization.
    ///
    /// Public organizations can be viewed by anyone, limited orgs can only be viewed by
    /// logged-in users, and private orgs can only be viewed by members of that org.
    #[clap(long, short)]
    visibility: Option<OrgVisibility>,
    /// Whether the admin of a repo can change org teams' access to it.
    #[clap(long, short)]
    admin_can_change_team_access: Option<bool>,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrgMemberVisibility {
    Private,
    Public,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrgVisibility {
    Private,
    Limited,
    Public,
}

impl Into<forgejo_api::structs::CreateOrgOptionVisibility> for OrgVisibility {
    fn into(self) -> forgejo_api::structs::CreateOrgOptionVisibility {
        use forgejo_api::structs::CreateOrgOptionVisibility as ApiVis;
        match self {
            OrgVisibility::Private => ApiVis::Private,
            OrgVisibility::Limited => ApiVis::Limited,
            OrgVisibility::Public => ApiVis::Public,
        }
    }
}

impl Into<forgejo_api::structs::EditOrgOptionVisibility> for OrgVisibility {
    fn into(self) -> forgejo_api::structs::EditOrgOptionVisibility {
        use forgejo_api::structs::EditOrgOptionVisibility as ApiVis;
        match self {
            OrgVisibility::Private => ApiVis::Private,
            OrgVisibility::Limited => ApiVis::Limited,
            OrgVisibility::Public => ApiVis::Public,
        }
    }
}

impl OrgCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        let repo = RepoInfo::get_current(host_name, None, self.remote.as_deref(), &keys)?;
        let api = keys.get_api(repo.host_url()).await?;
        match self.command {
            OrgSubcommand::List {
                page,
                only_member_of,
            } => list_orgs(&api, page, only_member_of).await?,
            OrgSubcommand::View { name } => view_org(&api, name).await?,
            OrgSubcommand::Create { name, options } => create_org(&api, name, options).await?,
            OrgSubcommand::Edit { name, options } => edit_org(&api, name, options).await?,
            OrgSubcommand::Activity { name } => list_activity(&api, name).await?,
            OrgSubcommand::Members { org, page } => list_org_members(&api, org, page).await?,
            OrgSubcommand::Visibility { org, set } => member_visibility(&api, org, set).await?,
            OrgSubcommand::Team(subcommand) => subcommand.run(&api).await?,
            OrgSubcommand::Label(subcommand) => subcommand.run(&api).await?,
            OrgSubcommand::Repo(subcommand) => subcommand.run(keys, &repo, &api).await?,
        }
        Ok(())
    }
}

fn is_valid_name_char(c: char) -> bool {
    match c {
        '-' | '_' | '.' => true,
        _ => c.is_ascii_alphanumeric(),
    }
}

async fn list_orgs(api: &Forgejo, page: u32, only_member_of: bool) -> eyre::Result<()> {
    let (total, orgs) = if only_member_of {
        let orgs = api.org_list_current_user_orgs().await?;
        (None, orgs)
    } else {
        let (headers, orgs) = api.org_get_all().page(page).await?;
        (Some(headers.x_total_count.unwrap_or_default() as u64), orgs)
    };

    if orgs.is_empty() {
        println!("No results");
    } else {
        let SpecialRender {
            bullet,
            bold,
            reset,
            ..
        } = *crate::special_render();
        for org in orgs {
            let name = org.name.ok_or_eyre("org does not have name")?;
            println!("{bullet} {bold}{name}{reset}");
        }
        if let Some(total) = total {
            println!("Page {} of {}", page, total.div_ceil(20));
        }
    }
    Ok(())
}

async fn view_org(api: &Forgejo, name: String) -> eyre::Result<()> {
    let org = api.org_get(&name).await?;

    let SpecialRender {
        bold,
        dash,
        bright_cyan,
        light_grey,
        reset,
        ..
    } = *crate::special_render();

    let name = org.name.as_deref().ok_or_eyre("org does not have name")?;
    let visibility = org
        .visibility
        .as_deref()
        .ok_or_eyre("new org does not have visibility")?;
    let vis_pretty = match visibility {
        "public" => "Public",
        "limited" => "Limited",
        "private" => "Private",
        _ => visibility,
    };

    if let Some(full_name) = &org.full_name {
        print!("{bold}{bright_cyan}{full_name}{reset} {light_grey}({name}){reset}");
    } else {
        print!("{bold}{bright_cyan}{name}{reset}");
    }
    print!(" {dash} {vis_pretty}");
    println!();
    let member_count = match api.org_list_members(&name).page(1).page_size(1).await {
        Ok((members_headers, _)) => members_headers.x_total_count.unwrap_or_default(),
        Err(_) => {
            let (members_headers, _) = api
                .org_list_public_members(&name)
                .page(1)
                .page_size(1)
                .await?;
            members_headers.x_total_count.unwrap_or_default()
        }
    };
    print!("{bold}{member_count}{reset} members");
    if let Ok((teams_headers, _)) = api.org_list_teams(&name).page(1).page_size(1).await {
        let teams = teams_headers.x_total_count.unwrap_or_default();
        println!(" {dash} {bold}{teams}{reset} teams");
    }
    println!();

    let mut first = true;
    if let Some(website) = &org.website {
        if !website.is_empty() {
            print!("{bold}{website}{reset}");
            first = false;
        }
    }
    if let Some(email) = &org.email {
        if !email.is_empty() {
            if !first {
                print!(" {dash} ");
            }
            print!("{email}");
            first = false;
        }
    }
    if let Some(location) = &org.location {
        if !location.is_empty() {
            if !first {
                print!(" {dash} ");
            }
            print!("{location}");
            first = false;
        }
    }
    if !first {
        println!();
    }

    if let Some(description) = &org.description {
        if !description.is_empty() {
            println!("\n{}\n", crate::markdown(&description));
        }
    }

    Ok(())
}

async fn create_org(api: &Forgejo, name: String, options: OrgOptions) -> eyre::Result<()> {
    if !name.chars().all(is_valid_name_char) {
        eyre::bail!("Organization names can only have alphanumeric characters, dash, underscore, or period. \n  If you want a name with other characters, try setting the --full-name flag");
    }
    if !name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        eyre::bail!("Organization names can only start with alphanumeric characters. \n  If you want a name that starts with other characters, try setting the --full-name flag");
    }
    if !name
        .chars()
        .last()
        .is_some_and(|c| c.is_ascii_alphanumeric())
    {
        eyre::bail!("Organization names can only end with alphanumeric characters. \n  If you want a name that ends with other characters, try setting the --full-name flag");
    }
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        // because of the prior check, if it isn't alphanumeric, it's definitely one of - _ or .
        if !c.is_alphanumeric() && !chars.peek().is_some_and(|c| c.is_alphanumeric()) {
            eyre::bail!("Organization names can't have consecutive non-alphanumberic characters.\n  If you want that in the name, try setting the --full-name flag");
        }
    }
    let opt = CreateOrgOption {
        description: options.description,
        email: options.email,
        full_name: options.full_name,
        location: options.location,
        repo_admin_change_team_access: options.admin_can_change_team_access,
        username: name,
        visibility: options.visibility.map(|v| v.into()),
        website: options.website,
    };
    let new_org = api.org_create(opt).await?;

    let name = new_org.name.ok_or_eyre("new org does not have name")?;
    let visibility = new_org
        .visibility
        .ok_or_eyre("new org does not have visibility")?;

    let SpecialRender {
        fancy,
        bold,
        light_grey,
        reset,
        ..
    } = *crate::special_render();
    print!("created new {visibility} org ");
    if let Some(full_name) = &new_org.full_name {
        if fancy {
            println!("{bold}{full_name}{reset} {light_grey}({name}){reset}");
        } else {
            println!("\"{full_name}\" ({name})");
        }
    } else {
        if fancy {
            println!("{bold}{name}{reset}");
        } else {
            println!("\"{name}\"");
        }
    }
    Ok(())
}

async fn edit_org(api: &Forgejo, name: String, options: OrgOptions) -> eyre::Result<()> {
    let opt = EditOrgOption {
        description: options.description,
        email: options.email,
        full_name: options.full_name,
        location: options.location,
        repo_admin_change_team_access: options.admin_can_change_team_access,
        visibility: options.visibility.map(|v| v.into()),
        website: options.website,
    };
    api.org_edit(&name, opt).await?;
    Ok(())
}

async fn list_activity(api: &Forgejo, name: String) -> eyre::Result<()> {
    let query = forgejo_api::structs::OrgListActivityFeedsQuery::default();
    let (_, feed) = api.org_list_activity_feeds(&name, query).await?;

    for activity in feed {
        crate::user::print_activity(&activity)?;
    }
    Ok(())
}

async fn list_org_members(api: &Forgejo, org: String, page: u32) -> eyre::Result<()> {
    let my_username = api
        .user_get_current()
        .await?
        .login
        .ok_or_eyre("current user does not have username")?;
    let (count, users) = if api.org_is_member(&org, &my_username).await.is_ok() {
        let (headers, users) = api.org_list_members(&org).page(page).await?;
        (headers.x_total_count.unwrap_or_default() as u64, users)
    } else {
        let (headers, users) = api.org_list_public_members(&org).page(page).await?;
        (headers.x_total_count.unwrap_or_default() as u64, users)
    };

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
        println!("Page {} of {}", page, count.div_ceil(20));
    }
    Ok(())
}

async fn member_visibility(
    api: &Forgejo,
    org: String,
    visibility: Option<OrgMemberVisibility>,
) -> eyre::Result<()> {
    let username = api
        .user_get_current()
        .await?
        .login
        .ok_or_eyre("current user does not have username")?;
    let SpecialRender {
        bright_blue, reset, ..
    } = crate::special_render();
    if api.org_is_member(&org, &username).await.is_ok() {
        match visibility {
            Some(OrgMemberVisibility::Private) => {
                api.org_conceal_member(&org, &username).await?;
                println!("You are now a private member of {bright_blue}{org}{reset}");
            }
            Some(OrgMemberVisibility::Public) => {
                api.org_conceal_member(&org, &username).await?;
                println!("You are now a public member of {bright_blue}{org}{reset}");
            }
            None => {
                if api.org_is_public_member(&org, &username).await.is_ok() {
                    println!("You are a public member of {bright_blue}{org}{reset}");
                } else {
                    println!("You are a private member of {bright_blue}{org}{reset}");
                }
            }
        }
    } else {
        println!("You are not a member of {bright_blue}{org}{reset}");
    }
    Ok(())
}

#[derive(Subcommand, Clone, Debug)]
pub enum LabelSubcommand {
    /// List all the issue labels an organization uses.
    List {
        /// The name of the organization to list the labels of.
        org: String,
    },
    /// Add a new issue label to an organization.
    Add {
        /// The name of the organization the label should be added to.
        org: String,
        /// The name of the label to add.
        name: String,
        /// The hexcode of the label to add.
        color: String,
        /// A description of what the label is for.
        #[clap(long, short)]
        description: Option<String>,
        /// If this label is named `{scope}/{name}`, make it exclusive with other labels with the
        /// same scope.
        #[clap(long, short)]
        exclusive: bool,
    },
    /// Edit an issue label an organization uses.
    Edit {
        /// The name of the organization the label is in.
        org: String,
        /// The name of the label to edit.
        name: String,
        /// Set a new name for the label.
        #[clap(long, short)]
        new_name: Option<String>,
        /// Set a new hexcode for the label.
        #[clap(long, short)]
        color: Option<String>,
        /// Set a description of what the label is for.
        #[clap(long, short)]
        description: Option<String>,
        /// Set whether this label is exclusive with others of the same scope.
        #[clap(long, short)]
        exclusive: bool,
        /// Set whether this label is archived.
        #[clap(long, short)]
        archived: Option<bool>,
    },
    /// Remove an issue label from an organization.
    Rm {
        /// The name of the organization the label is in.
        org: String,
        /// The name of the label to remove from the organization.
        label: String,
    },
}

impl LabelSubcommand {
    async fn run(self, api: &Forgejo) -> eyre::Result<()> {
        match self {
            LabelSubcommand::List { org } => list_org_labels(&api, org).await?,
            LabelSubcommand::Add {
                org,
                name,
                color,
                description,
                exclusive,
            } => add_org_label(&api, org, name, color, description, exclusive).await?,
            LabelSubcommand::Edit {
                org,
                name,
                new_name,
                color,
                description,
                exclusive,
                archived,
            } => {
                edit_org_label(
                    &api,
                    org,
                    name,
                    new_name,
                    color,
                    description,
                    exclusive,
                    archived,
                )
                .await?
            }
            LabelSubcommand::Rm { org, label } => remove_org_label(&api, org, label).await?,
        }
        Ok(())
    }
}

async fn list_org_labels(api: &Forgejo, org: String) -> eyre::Result<()> {
    let labels = api
        .org_list_labels(&org, OrgListLabelsQuery::default())
        .all()
        .await?;
    crate::render_label_list(&labels)?;
    Ok(())
}

async fn find_label_by_name(
    api: &Forgejo,
    org: &str,
    name: &str,
) -> eyre::Result<Option<forgejo_api::structs::Label>> {
    Ok(api
        .org_list_labels(&org, OrgListLabelsQuery::default())
        .stream()
        .try_filter(|label| {
            future::ready(
                label
                    .name
                    .as_deref()
                    .is_some_and(|label_name| label_name == name),
            )
        })
        .try_next()
        .await?)
}

async fn add_org_label(
    api: &Forgejo,
    org: String,
    name: String,
    color: String,
    description: Option<String>,
    exclusive: bool,
) -> eyre::Result<()> {
    let color = color
        .strip_prefix("#")
        .map(|s| s.to_owned())
        .unwrap_or(color);
    let opt = CreateLabelOption {
        color,
        description,
        exclusive: Some(exclusive),
        is_archived: Some(false),
        name,
    };
    let label = api.org_create_label(&org, opt).await?;
    println!("Created new label {}", crate::render_label(&label)?);
    Ok(())
}

async fn edit_org_label(
    api: &Forgejo,
    org: String,
    name: String,
    new_name: Option<String>,
    color: Option<String>,
    description: Option<String>,
    exclusive: bool,
    archived: Option<bool>,
) -> eyre::Result<()> {
    let old_label = find_label_by_name(api, &org, &name)
        .await?
        .ok_or_eyre("label not found")?;
    let id = old_label.id.ok_or_eyre("label does not have id")?;
    let color = color.map(|color| {
        color
            .strip_prefix("#")
            .map(|s| s.to_owned())
            .unwrap_or(color)
    });
    let opt = EditLabelOption {
        color,
        description,
        exclusive: Some(exclusive),
        is_archived: archived,
        name: new_name,
    };
    let label = api.org_edit_label(&org, id, opt).await?;
    println!(
        "Changed label {} to {}",
        crate::render_label(&old_label)?,
        crate::render_label(&label)?
    );
    Ok(())
}

async fn remove_org_label(api: &Forgejo, org: String, name: String) -> eyre::Result<()> {
    let label = find_label_by_name(api, &org, &name)
        .await?
        .ok_or_eyre("label not found")?;
    let id = label.id.ok_or_eyre("label does not have id")?;
    api.org_delete_label(&org, id).await?;
    println!("Removed label {}", crate::render_label(&label)?);
    Ok(())
}

#[derive(Subcommand, Clone, Debug)]
pub enum RepoSubcommand {
    /// List all the repos owned by this organization.
    List {
        /// The name of the organization to list the repos of.
        org: String,
        /// Which page of the results to view
        #[clap(long, short, default_value_t = 1)]
        page: u32,
    },
    /// Create a new repository in this organization.
    Create {
        /// The name of the organization to create the repo in.
        org: String,
        #[clap(flatten)]
        args: crate::repo::RepoCreateArgs,
    },
}

impl RepoSubcommand {
    async fn run(
        self,
        keys: &crate::KeyInfo,
        repo_info: &RepoInfo,
        api: &Forgejo,
    ) -> eyre::Result<()> {
        match self {
            RepoSubcommand::List { org, page } => list_org_repos(&api, org, page).await?,
            RepoSubcommand::Create {
                org,
                args:
                    crate::repo::RepoCreateArgs {
                        repo,
                        description,
                        private,
                        remote,
                        push,
                        ssh,
                    },
            } => {
                let url_host = crate::host_name(&repo_info.host_url());
                let ssh = ssh
                    .unwrap_or_else(|| Some(keys.default_ssh.contains(url_host)))
                    .unwrap_or(true);
                crate::repo::create_repo(
                    &api,
                    Some(org),
                    repo,
                    description,
                    private,
                    remote,
                    push,
                    ssh,
                )
                .await?
            }
        }
        Ok(())
    }
}

async fn list_org_repos(api: &Forgejo, org: String, page: u32) -> eyre::Result<()> {
    let (headers, repos) = api.org_list_repos(&org).page(page).await?;
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
        let count = headers.x_total_count.unwrap_or_default() as u64;
        println!("Page {} of {}", page, count.div_ceil(20));
    }
    Ok(())
}
