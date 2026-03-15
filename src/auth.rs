use clap::Subcommand;
use eyre::OptionExt;
use sha2::Digest;

use std::collections::BTreeMap;
#[cfg(unix)]
use std::path::PathBuf;

#[derive(Subcommand, Clone, Debug)]
pub enum AuthCommand {
    /// Log in to an instance.
    ///
    /// Opens an auth page in your browser
    Login,
    /// Deletes login info for an instance
    Logout {
        host: String,
    },
    /// Add an application token for an instance
    ///
    /// Use this if `fj auth login` doesn't work
    AddKey {
        /// The user that the key is associated with
        user: String,
        /// The key to add. If not present, the key will be read in from stdin.
        key: Option<String>,
    },
    UseSsh {
        use_ssh: Option<bool>,
    },
    /// List all instances you're currently logged into
    List,
}

impl AuthCommand {
    pub async fn run(self, keys: &mut crate::KeyInfo, host_name: Option<&str>) -> eyre::Result<()> {
        match self {
            AuthCommand::Login => {
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None, &keys)?;
                let host_url = repo_info.host_url();
                let client_info = get_client_info_for(host_url).await?;
                if let Some(client_id) = &client_info {
                    oauth_login(keys, host_url, client_id).await?;
                } else {
                    let host_domain = crate::host_name(&host_url);
                    let applications_url =
                        format!("https://{host_domain}/user/settings/applications");

                    println!("Your installation of fj doesn't support `login` for {host_domain}");
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
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None, &keys)?;
                let host_url = repo_info.host_url();
                let key = match key {
                    Some(key) => key,
                    None => crate::readline("new key: ").await?.trim().to_string(),
                };
                let host = crate::host_name(&host_url);
                if !keys.hosts.contains_key(host) {
                    let mut login = crate::keys::LoginInfo::Application {
                        name: user,
                        token: key,
                    };
                    add_ssh_alias(&mut login, host_url, keys).await;
                    keys.hosts.insert(host.to_owned(), login);
                } else {
                    println!("key for {host} already exists");
                }
            }
            AuthCommand::UseSsh { use_ssh } => {
                let repo_info = crate::repo::RepoInfo::get_current(host_name, None, None, &keys)?;
                let host = crate::host_name(&repo_info.host_url());
                if !keys.hosts.contains_key(host) {
                    println!("not logged in to {host}");
                } else {
                    if use_ssh.unwrap_or(true) {
                        let already_present = keys.default_ssh.insert(host.to_string());
                        if already_present {
                            println!("now will use SSH for {host} by default");
                        } else {
                            println!("already using SSH for {host} by default");
                        }
                    } else {
                        let was_present = keys.default_ssh.remove(host);
                        if was_present {
                            println!("will no longer use SSH for {host} by default");
                        } else {
                            println!("already not using SSH for {host} by default");
                        }
                    }
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

pub async fn get_client_info_for(url: &url::Url) -> eyre::Result<Option<String>> {
    let host = crate::host_name(url);
    let host = host.strip_suffix("/").unwrap_or(host);
    let mut possible_paths = Vec::with_capacity(3);

    // On MacOS, `directories::ProjectDirs` doesn't include the `.config` path
    // like it does on Linux.
    #[cfg(target_os = "macos")]
    if let Some(user_dirs) = directories::UserDirs::new() {
        possible_paths.push(user_dirs.home_dir().join(".config/forgejo-cli/client_ids"));
    }

    if let Some(dirs) = directories::ProjectDirs::from("", "Cyborus", "forgejo-cli") {
        possible_paths.push(dirs.config_dir().join("client_ids"));
    }

    #[cfg(unix)]
    possible_paths.push(PathBuf::from("/etc/fj/client_ids"));

    for possible_path in possible_paths {
        if let Ok(file) = tokio::fs::read_to_string(possible_path).await {
            let ids = parse_client_info_file(&file)?;
            if let Some(id) = ids.get(host) {
                return Ok(Some(id.to_string()));
            }
        }
    }

    let builtin = match host {
        "codeberg.org" => "19ac3dd0-e101-445d-aa60-d8ea3876bc5d",
        "code.forgejo.org" => "ab67d8a2-72bd-42e8-ae05-937eaba31e24",
        "v7.next.forgejo.org" => "adf79db0-0e6c-41d8-93a9-3c13e797e880",
        "v11.next.forgejo.org" => "0df6d672-fe05-4c9a-a5a9-e111e4905e14",
        "v12.next.forgejo.org" => "df333c23-09a7-41ee-ad52-de673166dbb8",
        "v13.next.forgejo.org" => "ef27a227-65f4-4bcb-be56-f8c9b44457b0",
        "v14.next.forgejo.org" => "2dc5d6d7-01b0-47b4-814e-b4b60aea2376",
        "git.disroot.org" => "c6051ae0-6d21-4c17-92e6-41b957376d09",
        "git.pub.solar" => "6c7fad2f-41c4-4c2d-90b2-5f7fd19c9be2",
        "git.kaki87.net" => "951299e6-cf99-4a9e-8aaf-4b4b4ac36f04",
        "git.gay" => "15233962-8f9d-4192-a7d7-129fb8c6bbff",
        "git.auxolotl.org" => "09fb4377-1e98-4c94-a43f-2c9843388e11",
        "git.lix.systems" => "71ec029f-b5a1-4079-8e06-5b957288b063",
        "code.ffmpeg.org" => "75d19c4d-01d0-4825-8953-76ad66543f2c",
        "forge.fedoraproject.org" => "b15a2f44-75b0-4d2f-a740-50e45cc161a3",
        _ => return Ok(None),
    };

    Ok(Some(builtin.to_string()))
}

fn parse_client_info_file(file: &str) -> eyre::Result<BTreeMap<&str, &str>> {
    file.lines()
        .map(|s| s.split_once("#").map(|s| s.0).unwrap_or(s).trim())
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(line_num, s)| {
            let mut iter = s.split_whitespace();
            let host = iter.next().expect("can't fail, empty lines filtered");
            let client_id = iter
                .next()
                .ok_or_else(|| eyre::eyre!("missing client id on line {}", line_num + 1))?;
            Ok::<_, eyre::Error>((host, client_id))
        })
        .collect::<Result<BTreeMap<&str, &str>, _>>()
}

//pub fn get_client_info_for(url: &url::Url) -> Option<&'static str> {
//    let host = crate::host_with_port_and_path(url);
//    let host = host.strip_suffix("/").unwrap_or(host);
//    include!(concat!(env!("OUT_DIR"), "/oauth_client_info.rs"))
//}

async fn oauth_login(
    keys: &mut crate::KeyInfo,
    host: &url::Url,
    client_id: &str,
) -> eyre::Result<()> {
    use base64ct::Encoding;
    use rand::{distr::Alphanumeric, prelude::*};

    let mut rng = rand::rng();

    let state = (0..32)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect::<String>();
    let code_verifier = (0..43)
        .map(|_| rng.sample(Alphanumeric) as char)
        .collect::<String>();
    let code_challenge =
        base64ct::Base64UrlUnpadded::encode_string(sha2::Sha256::digest(&code_verifier).as_slice());

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

    let api = forgejo_api::Forgejo::with_user_agent(
        forgejo_api::Auth::None,
        host.clone(),
        crate::USER_AGENT,
    )?;
    let request = forgejo_api::structs::OAuthTokenRequest::Public {
        client_id,
        code_verifier: &code_verifier,
        code: &code,
        redirect_uri: url::Url::parse("http://127.0.0.1:26218/").unwrap(),
    };
    let response = api.oauth_get_access_token(request).await?;

    let api = forgejo_api::Forgejo::with_user_agent(
        forgejo_api::Auth::OAuth2(&response.access_token),
        host.clone(),
        crate::USER_AGENT,
    )?;
    let current_user = api.user_get_current().await?;
    let name = current_user
        .login
        .ok_or_eyre("user does not have login name")?;

    // A minute less, in case any weirdness happens at the exact moment it
    // expires. Better to refresh slightly too soon than slightly too late.
    let expires_in = std::time::Duration::from_secs(response.expires_in.saturating_sub(60) as u64);
    let expires_at = time::OffsetDateTime::now_utc() + expires_in;
    let mut login_info = crate::keys::LoginInfo::OAuth {
        name,
        token: response.access_token,
        refresh_token: response.refresh_token,
        expires_at,
    };
    add_ssh_alias(&mut login_info, host, keys).await;
    let domain = crate::host_name(&host);
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

async fn add_ssh_alias(
    login: &mut crate::keys::LoginInfo,
    host_url: &url::Url,
    keys: &mut crate::keys::KeyInfo,
) {
    let api = match login.api_for(host_url).await {
        Ok(x) => x,
        Err(_) => return,
    };
    if let Some(ssh_url) = get_instance_ssh_url(api).await {
        let http_host = crate::host_name(&host_url);
        let ssh_host = crate::host_name(&ssh_url);
        if http_host != ssh_host {
            keys.aliases
                .insert(ssh_host.to_string(), http_host.to_string());
        }
    }
}

async fn get_instance_ssh_url(api: forgejo_api::Forgejo) -> Option<url::Url> {
    let query = forgejo_api::structs::RepoSearchQuery::default();
    let results = api.repo_search(query).page_size(1).await.ok()?;
    let ssh_url = results.data?.pop()?.ssh_url?;
    let (instance_ssh_url, _) = crate::repo::url_strip_repo_name(ssh_url).ok()?;
    Some(instance_ssh_url)
}
