use {
    fs::*,
    futures::executor::block_on,
    hyper::{server::conn::http1, service::service_fn},
    hyper_util::rt::TokioIo,
    log::warn,
    regex::Regex,
    std::{
        collections::HashMap,
        env,
        net::SocketAddr,
        path::{Path, PathBuf},
        sync::Arc,
        time::{Duration, Instant},
    },
    tokio::{net::TcpListener, process::Command, sync::mpsc::channel},
    ty::{Config, User},
};

pub mod fs;
pub mod net;
pub mod ty;

pub fn home() -> PathBuf { Path::new("/home/odde").to_path_buf() }

pub async fn home_mgr(config: Arc<Config>) {
    let mut users = HashMap::<User, Instant>::new();
    loop {
        let currently_logged_in = get_logged_in_users(config.clone())
            .await
            .inspect_err(|e| warn!("Failed to get logged in users: {}", e))
            .unwrap_or_default();

        let existing_users = users.clone();
        for u in currently_logged_in.iter().filter(|u| !existing_users.contains_key(*u)) {
            users.insert(u.clone(), Instant::now());
        }

        let now = Instant::now();
        users.retain(|u, t| {
            if currently_logged_in.contains(u) {
                *t = now;
            }

            if now.duration_since(*t) > TIMEOUT {
                match block_on(destroy(&u)) {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Failed to remove user {}: {e:?}", u.name);
                    }
                }

                match block_on(create(&u, config.clone())) {
                    Ok(_) => {}
                    Err(e) => {
                        warn!("Failed to remove user {}: {e:?}", u.name);
                    }
                }

                false
            } else {
                true
            }
        });
    }
}

pub async fn git_mgr(config: Arc<Config>) {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.unwrap();

    let (tx, mut rx) = channel::<()>(10);

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let tx = tx.clone();
            tokio::task::spawn(async move {
                if let Err(err) =
                    http1::Builder::new().serve_connection(io, service_fn(|r| net::git_wh(r, tx.clone()))).await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    });

    let urls = config.git.urls.clone();
    tokio::spawn(async move {
        let mut last_pull = Instant::now();
        while (rx.recv().await).is_some() {
            if Instant::now().duration_since(last_pull) < Duration::from_secs(100) {
                last_pull = Instant::now();
                for repo in urls.clone() {
                    let id = git_id(&repo);
                    let temp = Path::new(&env::temp_dir()).join(&id);
                    let hard = home().join(id);

                    if temp.exists() {
                        std::fs::remove_dir_all(temp.clone()).unwrap();
                    }

                    for repo in config.git.urls.clone() {
                        if Command::new("git")
                            .args(["clone", &repo, &temp.clone().display().to_string()])
                            .status()
                            .await
                            .unwrap()
                            .success()
                        {
                            std::fs::remove_dir_all(hard.clone().clone()).unwrap();
                            std::fs::copy(temp.clone(), hard.clone()).unwrap();
                        }
                    }
                }
            }
        }
    });
}

pub async fn setup_user(user: User, config: Arc<Config>) -> anyhow::Result<()> {
    Command::new("sudo").args(["useradd", "-s", "fish", &user.name]).status().await?;
    fs::create(&user, config.clone()).await?;
    Ok(())
}

pub fn git_id(url: &str) -> String {
    let re = Regex::new(r"(?:[^/:]+?\/([^/:]+?))(?:\.git|$)").unwrap();
    re.captures(url).unwrap().get(1).unwrap().as_str().to_string()
}
