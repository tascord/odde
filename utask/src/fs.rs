use std::{path::Path, time::Duration};

use anyhow::anyhow;
use tokio::process::Command;

use crate::{home, ty::User};

pub const TIMEOUT: Duration = Duration::from_secs(90 * 60);

pub async fn get_logged_in_users() -> anyhow::Result<Vec<String>> {
    Ok(
        String::from_utf8(Command::new("who").output().await?.stdout)?
            .lines()
            .filter_map(|l| l.split_whitespace().next().map(|v| v.to_string()))
            .collect(),
    )
}

pub fn destroy(user: String) -> anyhow::Result<()> {
    std::fs::remove_dir_all(Path::new("/home/").join(user)).map_err(|e| anyhow!("{e:?}"))
}

pub fn create(user: User) -> anyhow::Result<()> {
    let path = Path::new("/home/").join(&user.name);
    std::fs::create_dir(path.clone())?;
    std::fs::copy(home().join("rm-applications"), path.join("rm-applications"))?;
    std::fs::create_dir(path.clone().join(".ssh")).unwrap();
    std::fs::write(
        path.clone().join(".ssh").join("authorized_keys"),
        user.keys.join("\n"),
    )
    .unwrap();

    // TODO: Authkeys might require chmod

    Command::new("chown")
        .args([
            &format!("{}:{}", user.name, user.name),
            &path.display().to_string(),
            "-R",
        ])
        .spawn()
        .unwrap();

    Ok(())
}
