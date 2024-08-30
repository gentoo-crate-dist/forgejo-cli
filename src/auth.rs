use clap::Subcommand;
use eyre::OptionExt;

#[derive(Subcommand, Clone, Debug)]
pub enum AuthCommand {
    /// Log in to an instance.
    ///
    /// Opens an auth page in your browser
    Login,
    /// Deletes login info for an instance
    Logout { host: String },
    /// Add an application token for an instance
    ///
    /// Use this if `fj auth login` doesn't work
    AddKey {
        /// The user that the key is associated with
        user: String,
        /// The key to add. If not present, the key will be read in from stdin.
        key: Option<String>,
    },
    SetApiUrl {
        /// Sets the URL to access for API calls for this host.
        ///
        /// This can be useful if the api is hosted on a subdomain, or if the
        /// ssh and http access are on different domains.
        url: url::Url,
    },
    /// List all instances you're currently logged into
    List,
}

impl AuthCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        match self {
            AuthCommand::Login => {
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None)?;
                let host_url = repo_info.host_url();
                let client_info = get_client_info_for(host_url);
                if let Some((client_id, _)) = client_info {
                    oauth_login(keys, host_url, client_id).await?;
                } else {
                    let host_domain = host_url.host_str().ok_or_eyre("invalid host")?;
                    let host_path = host_url.path().strip_suffix("/").unwrap_or(host_url.path());
                    let applications_url =
                        format!("https://{host_domain}{host_path}/user/settings/applications");

                    println!("{host_domain}{host_path} doesn't support easy login");
                    println!();
                    println!("Please visit {applications_url}");
                    println!("to create a token, and use it to log in with `fj auth add-key`");
                }
            }
            AuthCommand::Logout { host } => {
                let info_opt = keys.hosts.remove(&host);
                if let Some(info) = info_opt {
                    eprintln!("signed out of {}@{}", &info.username(), host);
                } else {
                    eprintln!("already not signed in to {host}");
                }
            }
            AuthCommand::AddKey { user, key } => {
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None)?;
                let host_url = repo_info.host_url();
                let key = match key {
                    Some(key) => key,
                    None => crate::readline("new key: ").await?.trim().to_string(),
                };
                let host = crate::host_with_port(&host_url);
                if !keys.hosts.contains_key(host) {
                    keys.hosts.insert(
                        host.to_owned(),
                        crate::keys::LoginInfo::Application {
                            name: user,
                            token: key,
                            api_url: host_url.clone(),
                        },
                    );
                } else {
                    println!("key for {host} already exists");
                }
            }
            AuthCommand::SetApiUrl { url } => {
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None)?;
                let host_url = repo_info.host_url();
                let host = crate::host_with_port(&host_url);
                match keys.hosts.get_mut(host) {
                    Some(key_info) => match key_info {
                        crate::LoginInfo::OAuth { api_url, .. } => *api_url = url,
                        crate::LoginInfo::Application { api_url, .. } => *api_url = url,
                    },
                    None => println!("Not signed in to {host}"),
                }
            }
            AuthCommand::List => {
                if keys.hosts.is_empty() {
                    println!("No logins.");
                }
                for (host_url, login_info) in &keys.hosts {
                    println!("{}@{}", login_info.username(), host_url);
                }
            }
        }
        Ok(())
    }
}

pub fn get_client_info_for(url: &url::Url) -> Option<(&'static str, &'static str)> {
    let client_info = match (crate::host_with_port(url), url.path()) {
        ("codeberg.org", "/") => option_env!("CLIENT_INFO_CODEBERG"),
        _ => None,
    };
    client_info.and_then(|info| info.split_once(":"))
}

async fn oauth_login(
    keys: &mut crate::KeyInfo,
    host: &url::Url,
    client_id: &'static str,
) -> eyre::Result<()> {
    use base64ct::Encoding;
    use rand::{distributions::Alphanumeric, prelude::*};

    let mut rng = thread_rng();

    let state = (0..32)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect::<String>();
    let code_verifier = (0..43)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect::<String>();
    let code_challenge =
        base64ct::Base64Url::encode_string(sha256::digest(&code_verifier).as_bytes());

    let mut auth_url = host.clone();
    auth_url
        .path_segments_mut()
        .map_err(|_| eyre::eyre!("invalid url"))?
        .extend(["login", "oauth", "authorize"]);
    auth_url.query_pairs_mut().extend_pairs([
        ("client_id", client_id),
        ("redirect_uri", "http://127.0.0.1:26218/"),
        ("response_type", "code"),
        ("code_challenge_method", "S256"),
        ("code_challenge", &code_challenge),
        ("state", &state),
    ]);
    open::that(auth_url.as_str()).unwrap();

    let (handle, mut rx) = auth_server();
    let res = rx.recv().await.unwrap();
    handle.abort();
    let code = match res {
        Ok(Some((code, returned_state))) => {
            if returned_state == state {
                code
            } else {
                eyre::bail!("returned with invalid state");
            }
        }
        Ok(None) => {
            println!("Login canceled");
            return Ok(());
        }
        Err(e) => {
            eyre::bail!("Failed to authenticate: {e}");
        }
    };

    let api = forgejo_api::Forgejo::new(forgejo_api::Auth::None, host.clone())?;
    let request = forgejo_api::structs::OAuthTokenRequest::Public {
        client_id,
        code_verifier: &code_verifier,
        code: &code,
        redirect_uri: url::Url::parse("http://127.0.0.1:26218/").unwrap(),
    };
    let response = api.oauth_get_access_token(request).await?;

    let api = forgejo_api::Forgejo::new(
        forgejo_api::Auth::OAuth2(&response.access_token),
        host.clone(),
    )?;
    let current_user = api.user_get_current().await?;
    let name = current_user
        .login
        .ok_or_eyre("user does not have login name")?;

    // A minute less, in case any weirdness happens at the exact moment it
    // expires. Better to refresh slightly too soon than slightly too late.
    let expires_in = std::time::Duration::from_secs(response.expires_in.saturating_sub(60) as u64);
    let expires_at = time::OffsetDateTime::now_utc() + expires_in;
    let login_info = crate::keys::LoginInfo::OAuth {
        name,
        token: response.access_token,
        refresh_token: response.refresh_token,
        expires_at,
        api_url: host.clone(),
    };
    let domain = crate::host_with_port(&host);
    keys.hosts.insert(domain.to_owned(), login_info);

    Ok(())
}

use tokio::{sync::mpsc::Receiver, task::JoinHandle};

fn auth_server() -> (
    JoinHandle<eyre::Result<()>>,
    Receiver<Result<Option<(String, String)>, String>>,
) {
    let addr: std::net::SocketAddr = ([127, 0, 0, 1], 26218).into();
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let tx = std::sync::Arc::new(tx);
    let handle = tokio::spawn(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let server =
            hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new());
        let svc = hyper::service::service_fn(|req: hyper::Request<hyper::body::Incoming>| {
            let tx = std::sync::Arc::clone(&tx);
            async move {
                let mut code = None;
                let mut state = None;
                let mut error_description = None;
                if let Some(query) = req.uri().query() {
                    for item in query.split("&") {
                        let (key, value) = item.split_once("=").unwrap_or((item, ""));
                        match key {
                            "code" => code = Some(value),
                            "state" => state = Some(value),
                            "error_description" => error_description = Some(value),
                            _ => eprintln!("unknown key {key} {value}"),
                        }
                    }
                }
                let (response, message) = match (code, state, error_description) {
                    (_, _, Some(error)) => (Err(error.to_owned()), "Failed to authenticate"),
                    (Some(code), Some(state), None) => (
                        Ok(Some((code.to_owned(), state.to_owned()))),
                        "Authenticated! Close this tab and head back to your terminal",
                    ),
                    _ => (Ok(None), "Canceled"),
                };
                tx.send(response).await.unwrap();
                Ok::<_, hyper::Error>(hyper::Response::new(message.to_owned()))
            }
        });
        loop {
            let (connection, _addr) = listener.accept().await.unwrap();
            server
                .serve_connection(hyper_util::rt::TokioIo::new(connection), svc)
                .await
                .unwrap();
        }
    });
    (handle, rx)
}
