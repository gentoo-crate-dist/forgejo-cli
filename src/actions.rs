use std::{collections::BTreeMap, fmt::Display};

use clap::{Args, Subcommand};
use eyre::{bail, OptionExt};
use forgejo_api::{
    structs::{
        ActionVariable, CreateOrUpdateSecretOption, CreateVariableOption, UpdateVariableOption,
    },
    Forgejo, ForgejoError,
};
use hyper::StatusCode;
use time::Duration;

use crate::{
    repo::{RepoArg, RepoInfo, RepoName},
    SpecialRender,
};

#[derive(Args, Clone, Debug)]
pub struct ActionsCommand {
    /// The local git remote that points to the repo to operate on
    #[clap(long, short = 'R', global = true)]
    remote: Option<String>,

    /// The repo to operate on
    #[clap(long, short, global = true)]
    repo: Option<RepoArg>,

    #[clap(subcommand)]
    command: ActionsSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ActionsSubcommand {
    /// List the tasks on a repo
    Tasks {
        /// The page to show. One page always includes up to 20 tasks.
        #[clap(long, short, default_value = "1")]
        page: u32,
    },

    /// List and manage variables
    Variables {
        #[clap(subcommand)]
        command: ActionsVariablesSubcommmand,
    },

    Secrets {
        #[clap(subcommand)]
        command: ActionsSecretsSubcommmand,
    },

    /// Dispatch a workflow
    Dispatch {
        /// Name of the workflow to dispatch
        name: String,

        /// Git revision to dispatch the workflow on
        r#ref: String,

        #[clap(long, short = 'I', value_parser = parse_dispatch_kvs)]
        inputs: Vec<(String, String)>,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ActionsVariablesSubcommmand {
    /// List variables
    List {
        /// Also print owner_id and repo_id
        #[clap(long, short)]
        verbose: bool,
    },

    /// Create a new variable
    Create {
        /// The name of the new variable
        name: String,

        /// The data to save into the variable. Omit to invoke editor.
        data: Option<String>,

        /// Override existing variables
        #[clap(long, short)]
        force: bool,
    },

    /// Delete a variable
    Delete {
        /// The variable to delete
        name: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be deleted without actually deleting
        #[clap(long)]
        dry_run: bool,
    },
}

#[derive(Subcommand, Clone, Debug)]
pub enum ActionsSecretsSubcommmand {
    /// List secrets
    List,

    /// Create a new secret
    Create {
        /// The name of the new secret
        name: String,

        /// The data to save into the secret.
        data: String,
    },

    /// Delete a secret
    Delete {
        /// The secret to delete
        name: String,
        /// Skip confirmation prompt
        #[clap(long, short = 'f')]
        force: bool,
        /// Show what would be deleted without actually deleting
        #[clap(long)]
        dry_run: bool,
    },
}

impl ActionsCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        let repo =
            RepoInfo::get_current(host_name, self.repo.as_ref(), self.remote.as_deref(), keys)?;

        let api = keys.get_api(repo.host_url()).await?;
        let repo = repo
            .name()
            .ok_or_eyre("can't figure what repo to access, try specifying with `--repo`")?;
        match self.command {
            ActionsSubcommand::Tasks { page } => view_tasks(repo, &api, page).await?,

            ActionsSubcommand::Variables { command } => match command {
                ActionsVariablesSubcommmand::List { verbose } => {
                    list_variables(repo, &api, verbose).await?
                }
                ActionsVariablesSubcommmand::Create { name, data, force } => {
                    create_variable(repo, &api, name, data, force).await?
                }
                ActionsVariablesSubcommmand::Delete { name, force, dry_run } => {
                    delete_variable(repo, &api, name, force, dry_run).await?
                }
            },

            ActionsSubcommand::Secrets { command } => match command {
                ActionsSecretsSubcommmand::List => list_secrets(repo, &api).await?,
                ActionsSecretsSubcommmand::Create { name, data } => {
                    create_secret(repo, &api, name, data).await?
                }
                ActionsSecretsSubcommmand::Delete { name, force, dry_run } => {
                    delete_secret(repo, &api, name, force, dry_run).await?
                }
            },

            ActionsSubcommand::Dispatch {
                name,
                r#ref,
                inputs,
            } => dispatch(repo, &api, name, r#ref, inputs.into_iter().collect()).await?,
        }

        Ok(())
    }
}

async fn view_tasks(repo: &RepoName, api: &Forgejo, page: u32) -> eyre::Result<()> {
    // We don't iterate this to collect all tasks (not just the ones on the first page) like the
    // issue search subcommand will do, because it's unlikely someone wants to see *all* tasks.
    let res = api
        .list_action_tasks(repo.owner(), repo.name())
        .page(page)
        .page_size(20)
        .await?;

    if res.total_count == Some(1) {
        println!("1 task");
    } else {
        println!("{} tasks", res.total_count.unwrap_or(0));
    }

    let SpecialRender {
        fancy,
        reset,

        bold,
        bright_green,
        light_grey,
        bright_red,
        yellow,
        bright_blue,
        ..
    } = *crate::special_render();

    for task in res.workflow_runs.unwrap_or_default() {
        let task_sym = match task.status.as_deref() {
            // Don't use symbols when we're not in fancy mode.
            x if !fancy => x.unwrap_or("?"),

            // See: https://codeberg.org/forgejo/forgejo/src/commit/5380f23daba969057d9afc53c3dc746eca95188c/models/actions/status.go#L26
            Some("success") => &format!("{bright_green}✓{reset}"),
            Some("cancelled") => &format!("{light_grey}!{reset}"),
            Some("failure") => &format!("{bright_red}×{reset}"),
            Some("waiting") => &format!("{light_grey}{reset}"),
            Some("running") => &format!("{yellow}●{reset}"),
            Some("skipped") => &format!("{light_grey}{reset}"),
            Some("blocked") => &format!("{bright_red}{reset}"),
            Some(x) => x,
            None => "?",
        };

        let sha = task.head_sha.unwrap_or_default();
        let sha = if sha.len() > 10 { &sha[0..10] } else { &sha };

        let time = if let (Some(end), Some(start)) = (task.updated_at, task.run_started_at) {
            end - start
        } else {
            Duration::default()
        };

        println!(
            "#{bold}{}{reset} ({bright_blue}{}{reset}) {} {} {bright_green}{}{reset} ({}): {yellow}{}{reset}",
            task.run_number.unwrap_or(0),
            sha,
            task_sym,
            task.name.unwrap_or_default(),
            time,
            task.event.unwrap_or_default(),
            task.display_title.unwrap_or_default(),
        );
    }

    Ok(())
}

async fn list_variables(repo: &RepoName, api: &Forgejo, verbose: bool) -> eyre::Result<()> {
    let variables = api
        .get_repo_variables_list(repo.owner(), repo.name())
        .all()
        .await?;

    for var in variables {
        println!("{}", DisplayActionVariable::new(var, verbose)?);
    }

    Ok(())
}

async fn create_variable(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    data: Option<String>,
    force: bool,
) -> eyre::Result<()> {
    let mut data = if let Some(data) = data {
        data
    } else {
        let mut data = String::new();
        crate::editor(&mut data, Some("variable_content.txt")).await?;
        data
    };

    match api
        .create_repo_variable(
            repo.owner(),
            repo.name(),
            &name,
            CreateVariableOption {
                // If we don't have force enabled, we will not need the data again to (potentially)
                // make another request. To avoid a clone in this case, we take the string here,
                // replacing it with an empty one.
                value: if force {
                    data.clone()
                } else {
                    std::mem::take(&mut data)
                },
            },
        )
        .await
    {
        Err(ForgejoError::ApiError(forgejo_api::ApiError {
            kind: forgejo_api::ApiErrorKind::Other(StatusCode::CONFLICT),
            ..
        })) => {
            if !force {
                bail!("variable already exists, pass --force to replace it.");
            }

            eprintln!("variable already exists, updating.");
            api.update_repo_variable(
                repo.owner(),
                repo.name(),
                &name,
                UpdateVariableOption {
                    name: None,
                    value: data,
                },
            )
            .await?;
        }
        Err(e) => return Err(e.into()),
        Ok(()) => {}
    }

    Ok(())
}

async fn delete_variable(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if dry_run {
        println!("Would delete variable: {}", name);
        println!("\nRun with --force to actually delete.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Delete variable `{}`?", name);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Deleting variable: {}", name);
    api.delete_repo_variable(repo.owner(), repo.name(), &name)
        .await?;
    println!("Variable {name} deleted.");

    Ok(())
}

async fn list_secrets(repo: &RepoName, api: &Forgejo) -> eyre::Result<()> {
    let secrets = api
        .repo_list_actions_secrets(repo.owner(), repo.name())
        .all()
        .await?;

    for secret in secrets {
        println!(
            "({}) {}",
            crate::DisplayOptional(secret.created_at, "?"),
            crate::DisplayOptional(secret.name, "?")
        );
    }

    Ok(())
}

async fn create_secret(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    data: String,
) -> eyre::Result<()> {
    api.update_repo_secret(
        repo.owner(),
        repo.name(),
        &name,
        CreateOrUpdateSecretOption { data },
    )
    .await?;

    Ok(())
}

async fn delete_secret(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    force: bool,
    dry_run: bool,
) -> eyre::Result<()> {
    if dry_run {
        println!("Would delete secret: {}", name);
        println!("\nRun with --force to actually delete.");
        return Ok(());
    }

    if !force && !crate::yes_mode() {
        let prompt = format!("Delete secret `{}`? This cannot be undone.", name);
        if !crate::prompt_bool(&prompt, false).await? {
            println!("Aborted.");
            return Ok(());
        }
    }

    crate::verbose_log!("Deleting secret: {}", name);
    api.delete_repo_secret(repo.owner(), repo.name(), &name)
        .await?;
    println!("Secret {name} deleted.");

    Ok(())
}

async fn dispatch(
    repo: &RepoName,
    api: &Forgejo,
    name: String,
    r#ref: String,
    inputs: BTreeMap<String, String>,
) -> eyre::Result<()> {
    let n_inputs = inputs.len();
    api.dispatch_workflow(
        repo.owner(),
        repo.name(),
        &name,
        forgejo_api::structs::DispatchWorkflowOption {
            inputs: Some(inputs),
            return_run_info: Some(false),
            r#ref: r#ref.clone(),
        },
    )
    .await?;

    println!("Dispatched workflow {name} in {ref} with {n_inputs} input(s).");

    Ok(())
}

struct DisplayActionVariable {
    name: String,
    data: String,
    owner_id: Option<i64>,
    repo_id: Option<i64>,
    verbose: bool,
}

impl DisplayActionVariable {
    fn new(value: ActionVariable, verbose: bool) -> eyre::Result<Self> {
        Ok(Self {
            name: value
                .name
                .ok_or_eyre("Server returned ActionVariable without name?!")?,
            // The API usually (always?) returns Some("") here. The page on variables also notes
            // that their value cannot be read by other means than being passed to a CI job.
            data: value.data.unwrap_or_default(),
            owner_id: value.owner_id,
            repo_id: value.repo_id,
            verbose,
        })
    }
}

impl Display for DisplayActionVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.verbose {
            write!(
                f,
                "({}, {}) ",
                crate::DisplayOptional(self.owner_id, "?"),
                crate::DisplayOptional(self.repo_id, "?"),
            )?;
        }

        write!(f, "{}", self.name)?;

        if !self.data.is_empty() {
            write!(f, " = {}", self.data)?;
        }

        Ok(())
    }
}

fn parse_dispatch_kvs(s: &str) -> eyre::Result<(String, String)> {
    let eq_idx = s
        .find('=')
        .ok_or_eyre("Input argument does not contain a '=' character!")?;

    Ok((s[..eq_idx].to_string(), s[eq_idx + 1..].to_string()))
}
