use std::sync::Arc;

use futures::future::try_join_all;
use ipsea::log::warn;
use odde::{
    home, setup_user,
    ty::{Config, UTaskRequest},
};

#[tokio::main]
async fn main() {
    tokio::spawn(odde::git_mgr()); // Keep an up-to-date git instance locally
    tokio::spawn(odde::home_mgr()); // Nuke all accounts that havent been logged in for 90m

    let config: Arc<Config> = Arc::new(
        toml::from_str(&std::fs::read_to_string(home().join("config.toml")).unwrap()).unwrap(),
    );

    let proms = config
        .users
        .iter()
        .map(|u| async { setup_user(u.clone()).await });

    let _ = try_join_all(proms).await;

    let _ = ipsea::start_server("odde".to_string(), {
        let config = config.clone();
        move |a: UTaskRequest, b| {
            let user = config
                .users
                .iter()
                .find(|v| v.keys.iter().any(|k| a.key.contains(k)));

            if let Some(user) = user {
                let _ = odde::fs::create(user.clone());
            } else {
                warn!("User with no configured key logging in...");
            }

            let _ = b.send(true);
        }
    });
}
