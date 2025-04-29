use {
    crate::{
        git_id, home,
        ty::{Config, User},
    },
    anyhow::{anyhow, bail},
    log::{info, warn},
    std::{path::Path, sync::Arc, time::Duration},
    tokio::process::Command,
};

pub const TIMEOUT: Duration = Duration::from_secs(90 * 60);

pub async fn get_logged_in_users(config: Arc<Config>) -> anyhow::Result<Vec<User>> {
    let result = Ok(String::from_utf8(Command::new("who").output().await?.stdout)?
        .lines()
        .filter_map(|l| l.split_whitespace().next().and_then(|v| config.users.iter().find(|u| u.name == *v)))
        .cloned()
        .collect());
    result
}

pub async fn destroy(user: &User) -> anyhow::Result<()> {
    info!("Destroying user: {}", user.name);
    let path = Path::new("/home/").join(&user.name);
    info!("Removing directory: {}", path.display());
    match Command::new("sudo").args(["rm", "-rf", &path.display().to_string()]).status().await.map(|r| r.success()) {
        Ok(true) => {
            info!("Successfully removed directory: {}", path.display());
            Ok(())
        }
        Ok(false) => {
            warn!("Non-zero status code");
            bail!("Non-zero status code")
        }
        Err(e) => {
            warn!("{e:?}");
            bail!("{e:?}")
        }
    }
}

async fn command_is_okay(c: &mut Command) -> Result<(), ()> {
    info!("Running command: {:?}", c);
    match c.status().await.map(|v| v.success()) {
        Ok(true) => {
            info!("Command succeeded");
            Ok(())
        }
        Ok(false) => {
            warn!("Non-zero status code");
            Err(())
        }
        Err(e) => {
            warn!("{e:?}");
            Err(())
        }
    }
}

pub async fn create(user: &User, config: Arc<Config>) -> anyhow::Result<()> {
    info!("Creating user: {}", user.name);
    let path = Path::new("/home/").join(&user.name);
    info!("Creating home directory: {}", path.display());

    // Create home dir
    command_is_okay(Command::new("sudo").args(["mkdir", &path.display().to_string()]))
        .await
        .map_err(|_| anyhow!("Failed to make user dir"))?;

    // Copy repos
    for repo in config.git.urls.clone() {
        let id = git_id(&repo);
        info!("Copying repo: {} to {}", home().join(&id).display(), path.join(&id).display());
        command_is_okay(Command::new("sudo").args([
            "cp",
            &home().join(&id).display().to_string(),
            &path.join(&id).display().to_string(),
        ]))
        .await
        .map_err(|_| anyhow!("Failed to copy git repo {repo}"))?;
    }

    // Create ssh dir
    info!("Creating .ssh directory: {}", path.join(".ssh").display());
    command_is_okay(Command::new("sudo").args(["mkdir", &path.join(".ssh").display().to_string()]))
        .await
        .map_err(|_| anyhow!("Failed to make .ssh dir"))?;

    // Write authorized keys
    let loc = path.clone().join(".ssh").join("authorized_keys");
    info!("Writing authorized keys to: {}", loc.display());
    for key in user.keys.clone() {
        info!("Writing key: {}", key);
        command_is_okay(Command::new("fish").args(["-c", &format!("echo {} | tee -a {}", key, loc.display())]))
            .await
            .map_err(|_| anyhow!("Failed to write key to authorized_keys"))?;
    }

    // TODO: Authkeys might require chmod
    info!("Chowning directory: {} to {}:{}", path.display(), user.name, user.name);
    match Command::new("sudo")
        .args(["chown", &format!("{}:{}", user.name, user.name), &path.display().to_string(), "-R"])
        .status()
        .await
        .map(|r| r.success())
    {
        Ok(true) => {
            info!("Successfully chowned directory: {} to {}:{}", path.display(), user.name, user.name);
            Ok(())
        }
        Ok(false) => {
            warn!("Non-zero status code");
            bail!("Non-zero status code")
        }
        Err(e) => {
            warn!("{e:?}");
            bail!("{e:?}")
        }
    }
}
