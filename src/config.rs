use clap::{Args, Subcommand};
use eyre::Result;

/// Read the default project from git config
pub fn get_default_project() -> Result<Option<String>> {
    match git2::Repository::discover(".") {
        Ok(repo) => {
            match repo.config() {
                Ok(config) => {
                    match config.get_string("fj.defaultProject") {
                        Ok(value) => Ok(Some(value)),
                        Err(_) => Ok(None), // Config key doesn't exist
                    }
                }
                Err(_) => Ok(None), // Can't read config
            }
        }
        Err(_) => Ok(None), // Not in a git repo
    }
}

/// Set the default project in git config
pub fn set_default_project(project_ref: &str) -> Result<()> {
    let repo = git2::Repository::discover(".")?;
    let mut config = repo.config()?;
    config.set_str("fj.defaultProject", project_ref)?;
    Ok(())
}

/// Remove the default project from git config
pub fn unset_default_project() -> Result<()> {
    let repo = git2::Repository::discover(".")?;
    let mut config = repo.config()?;
    config.remove("fj.defaultProject")?;
    Ok(())
}

/// List all fj.* config entries
pub fn list_config() -> Result<Vec<(String, String)>> {
    let repo = git2::Repository::discover(".")?;
    let config = repo.config()?;
    let mut entries = Vec::new();

    let config_entries = config.entries(Some("fj\\.*"))?;
    config_entries.for_each(|entry| {
        if let (Some(name), Some(value)) = (entry.name(), entry.value()) {
            entries.push((name.to_string(), value.to_string()));
        }
    })?;

    Ok(entries)
}

#[derive(Args, Clone, Debug)]
pub struct ConfigCommand {
    #[clap(subcommand)]
    command: ConfigSubcommand,
}

#[derive(Subcommand, Clone, Debug)]
pub enum ConfigSubcommand {
    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },
    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },
    /// Remove a configuration key
    Unset {
        /// Configuration key
        key: String,
    },
    /// List all fj configuration
    List,
}

impl ConfigCommand {
    pub async fn run(self) -> Result<()> {
        use ConfigSubcommand::*;
        match self.command {
            Set { key, value } => {
                if key == "default-project" {
                    set_default_project(&value)?;
                    println!("Set fj.defaultProject = {}", value);
                } else {
                    eyre::bail!("Unknown config key: {}. Supported keys: default-project", key);
                }
            }
            Get { key } => {
                if key == "default-project" {
                    match get_default_project()? {
                        Some(value) => println!("{}", value),
                        None => {
                            eprintln!("No default project configured");
                            std::process::exit(1);
                        }
                    }
                } else {
                    eyre::bail!("Unknown config key: {}. Supported keys: default-project", key);
                }
            }
            Unset { key } => {
                if key == "default-project" {
                    unset_default_project()?;
                    println!("Removed fj.defaultProject");
                } else {
                    eyre::bail!("Unknown config key: {}. Supported keys: default-project", key);
                }
            }
            List => {
                let entries = list_config()?;
                if entries.is_empty() {
                    println!("No fj configuration found");
                } else {
                    for (key, value) in entries {
                        println!("{} = {}", key, value);
                    }
                }
            }
        }
        Ok(())
    }
}