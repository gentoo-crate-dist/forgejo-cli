use eyre::eyre;
use std::{collections::BTreeMap, io::ErrorKind};
use tokio::io::AsyncWriteExt;
use url::Url;

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct KeyInfo {
    pub hosts: BTreeMap<String, LoginInfo>,
}

impl KeyInfo {
    pub async fn load() -> eyre::Result<Self> {
        let path = directories::ProjectDirs::from("", "Cyborus", "forgejo-cli")
            .ok_or_else(|| eyre!("Could not find data directory"))?
            .data_dir()
            .join("keys.json");
        let json = tokio::fs::read(path).await;
        let this = match json {
            Ok(x) => serde_json::from_slice::<Self>(&x)?,
            Err(e) if e.kind() == ErrorKind::NotFound => {
                eprintln!("keys file not found, creating");
                Self::default()
            }
            Err(e) => return Err(e.into()),
        };
        Ok(this)
    }

    pub async fn save(&self) -> eyre::Result<()> {
        let json = serde_json::to_vec_pretty(self)?;
        let dirs = directories::ProjectDirs::from("", "Cyborus", "forgejo-cli")
            .ok_or_else(|| eyre!("Could not find data directory"))?;
        let path = dirs.data_dir();

        tokio::fs::create_dir_all(path).await?;

        tokio::fs::File::create(path.join("keys.json"))
            .await?
            .write_all(&json)
            .await?;

        Ok(())
    }

    pub fn get_login(&mut self, url: &Url) -> eyre::Result<&mut LoginInfo> {
        let host = crate::host_with_port(url);
        let login_info = self
            .hosts
            .get_mut(host)
            .ok_or_else(|| eyre!("not signed in to {host}"))?;
        Ok(login_info)
    }

    pub async fn get_api(&mut self, url: &Url) -> eyre::Result<forgejo_api::Forgejo> {
        self.get_login(url)?.api_for().await.map_err(Into::into)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
#[serde(tag = "type")]
pub enum LoginInfo {
    Application {
        name: String,
        token: String,
        api_url: Url,
    },
    OAuth {
        name: String,
        token: String,
        refresh_token: String,
        expires_at: time::OffsetDateTime,
        api_url: Url,
    },
}

impl LoginInfo {
    pub fn username(&self) -> &str {
        match self {
            LoginInfo::Application { name, .. } => name,
            LoginInfo::OAuth { name, .. } => name,
        }
    }

    pub async fn api_for(&mut self) -> eyre::Result<forgejo_api::Forgejo> {
        match self {
            LoginInfo::Application { token, api_url, .. } => {
                let api =
                    forgejo_api::Forgejo::new(forgejo_api::Auth::Token(token), api_url.clone())?;
                Ok(api)
            }
            LoginInfo::OAuth {
                token,
                refresh_token,
                expires_at,
                api_url,
                ..
            } => {
                if time::OffsetDateTime::now_utc() >= *expires_at {
                    let api = forgejo_api::Forgejo::new(forgejo_api::Auth::None, api_url.clone())?;
                    let (client_id, client_secret) = crate::auth::get_client_info_for(api_url)
                        .ok_or_else(|| {
                            eyre::eyre!("Can't refresh token; no client info for {api_url}. How did this happen?")
                        })?;
                    let response = api
                        .oauth_get_access_token(forgejo_api::structs::OAuthTokenRequest::Refresh {
                            refresh_token,
                            client_id,
                            client_secret,
                        })
                        .await?;
                    *token = response.access_token;
                    *refresh_token = response.refresh_token;
                    // A minute less, in case any weirdness happens at the exact moment it
                    // expires. Better to refresh slightly too soon than slightly too late.
                    let expires_in = std::time::Duration::from_secs(
                        response.expires_in.saturating_sub(60) as u64,
                    );
                    *expires_at = time::OffsetDateTime::now_utc() + expires_in;
                }
                let api =
                    forgejo_api::Forgejo::new(forgejo_api::Auth::Token(token), api_url.clone())?;
                Ok(api)
            }
        }
    }
}
