use {
    futures::{executor::block_on, future::try_join_all},
    log::{warn, LevelFilter},
    odde::{
        home, setup_user,
        ty::{Config, UTaskRequest},
    },
    std::{env, sync::Arc},
};

fn logger() {
    let mut builder = pretty_env_logger::formatted_builder();
    builder.filter_level(if env::var("RUST_LOG").is_ok() { LevelFilter::Debug } else { LevelFilter::Info });
    builder.init();
}

#[tokio::main]
async fn main() {
    logger();
    tokio::spawn(odde::git_mgr()); // Keep an up-to-date git instance locally
    tokio::spawn(odde::home_mgr()); // Nuke all accounts that havent been logged in for 90m

    let config: Arc<Config> =
        Arc::new(toml::from_str(&std::fs::read_to_string(home().join("config.toml")).unwrap()).unwrap());

    let proms = config.users.iter().map(|u| async { setup_user(u.clone()).await });

    let _ = try_join_all(proms).await;

    let _ = ipsea::start_server("odde".to_string(), {
        let config = config.clone();
        move |a: UTaskRequest, b| {
            let user = config.users.iter().find(|v| v.keys.iter().any(|k| a.key.contains(k)));

            if let Some(user) = user {
                let _ = block_on(odde::fs::create(user.clone())).inspect_err(|e| warn!("Failed to create wd: {e:?}"));
            } else {
                warn!("User with no configured key logging in...");
            }

            let _ = b.send(true);
        }
    });
}
