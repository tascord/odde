use std::{
    collections::HashMap,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use fs::*;
use hyper::{server::conn::http1, service::service_fn};
use hyper_util::rt::TokioIo;
use ipsea::log::warn;
use tokio::{net::TcpListener, process::Command, sync::mpsc::channel};

pub mod fs;
pub mod net;
pub mod ty;

pub fn home() -> PathBuf {
    Path::new("/opt/utask").to_path_buf()
}

pub async fn home_mgr() {
    let mut users = HashMap::<String, Instant>::new();
    loop {
        let currently_logged_in = get_logged_in_users()
            .await
            .inspect_err(|e| warn!("Failed to get logged in users: {}", e))
            .unwrap_or_default();

        let existing_users = users.clone();
        for u in currently_logged_in
            .iter()
            .filter(|u| !existing_users.contains_key(*u))
        {
            users.insert(u.to_string(), Instant::now());
        }

        let now = Instant::now();
        users.retain(|u, t| {
            if currently_logged_in.contains(u) {
                *t = now;
            }

            if now.duration_since(*t) > TIMEOUT {
                match destroy(u.to_string()) {
                    Ok(_) => false,
                    Err(e) => {
                        warn!("Failed to remove user {u}: {e:?}");
                        true
                    }
                }
            } else {
                true
            }
        });
    }
}

pub async fn git_mgr() {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await.unwrap();

    let (tx, mut rx) = channel::<()>(10);

    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let io = TokioIo::new(stream);
            let tx = tx.clone();
            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service_fn(|r| net::git_wh(r, tx.clone())))
                    .await
                {
                    eprintln!("Error serving connection: {:?}", err);
                }
            });
        }
    });

    tokio::spawn(async move {
        let mut last_pull = Instant::now();
        while let Some(_) = rx.recv().await {
            if Instant::now().duration_since(last_pull) < Duration::from_secs(100) {
                last_pull = Instant::now();
                let temp = Path::new("/tmp/rm-applications");
                let hard = home().join("/rm-applications");

                if temp.exists() {
                    std::fs::remove_dir_all(temp).unwrap();
                }

                if Command::new("git")
                    .args([
                        "clone",
                        "git@github.com:RMHEDGE/rm-applications.git",
                        &temp.display().to_string(),
                    ])
                    .status()
                    .await
                    .unwrap()
                    .success()
                {
                    std::fs::remove_dir_all(hard.clone()).unwrap();
                    std::fs::copy(temp, hard).unwrap();
                }
            }
        }
    });
}

pub async fn setup_user(user: String) -> anyhow::Result<()> {
    fs::create(user.clone())?;
    Command::new("useradd")
        .args([
            &Path::new("/home/").join(&user).display().to_string(),
            "-m",
            "-s",
            "fish",
            &user,
        ])
        .status()
        .await?;
    Ok(())
}
