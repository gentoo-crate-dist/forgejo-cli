use clap::Args;
#[cfg(feature = "update-check")]
use eyre::OptionExt;

#[derive(Args, Clone, Debug)]
pub struct VersionCommand {
    /// Checks for updates
    #[clap(long)]
    #[cfg(feature = "update-check")]
    check: bool,
    #[clap(short, long)]
    verbose: bool,
}

const BUILD_TYPE: &str = match option_env!("BUILD_TYPE") {
    Some(s) => s,
    None => "crates.io",
};

impl VersionCommand {
    pub async fn run(self) -> eyre::Result<()> {
        println!("{} v{}", env!("CARGO_BIN_NAME"), env!("CARGO_PKG_VERSION"));
        if self.verbose {
            println!("user agent: {}", crate::USER_AGENT);
            println!("build type: {BUILD_TYPE}");
            println!("    target: {}", env!("BUILD_TARGET"));
        }
        #[cfg(feature = "update-check")]
        self.update_msg().await?;
        Ok(())
    }

    #[cfg(feature = "update-check")]
    pub async fn update_msg(self) -> eyre::Result<()> {
        use std::cmp::Ordering;

        if self.check {
            let url = url::Url::parse("https://codeberg.org/")?;
            let api = forgejo_api::Forgejo::with_user_agent(
                forgejo_api::Auth::None,
                url,
                crate::USER_AGENT,
            )?;

            let latest = api
                .repo_get_latest_release("forgejo-contrib", "forgejo-cli")
                .await?;
            let latest_tag = latest
                .tag_name
                .ok_or_eyre("latest release does not have name")?;
            let latest_ver = latest_tag
                .strip_prefix("v")
                .unwrap_or(&latest_tag)
                .parse::<semver::Version>()?;

            let current_ver = env!("CARGO_PKG_VERSION").parse::<semver::Version>()?;

            match current_ver.cmp(&latest_ver) {
                Ordering::Less => {
                    let latest_url = latest
                        .html_url
                        .ok_or_eyre("latest release does not have url")?;
                    println!("New version available: {latest_ver}");
                    println!("Get it at {}", latest_url);
                }
                Ordering::Equal => {
                    println!("Up to date!");
                }
                Ordering::Greater => {
                    println!("You are ahead of the latest published version");
                }
            }
        } else {
            println!("Check for a new version with `fj version --check`");
        }
        Ok(())
    }
}
