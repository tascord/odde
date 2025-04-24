use {
    crate::{home, ty::User},
    anyhow::bail,
    std::{path::Path, time::Duration},
    tokio::process::Command,
};

pub const TIMEOUT: Duration = Duration::from_secs(90 * 60);

pub async fn get_logged_in_users() -> anyhow::Result<Vec<String>> {
    Ok(String::from_utf8(Command::new("who").output().await?.stdout)?
        .lines()
        .filter_map(|l| l.split_whitespace().next().map(|v| v.to_string()))
        .collect())
}

pub async fn destroy(user: String) -> anyhow::Result<()> {
    match Command::new("sudo")
        .args(["rm", "-rf", &Path::new("/home/").join(user).display().to_string()])
        .status()
        .await
        .map(|r| r.success())
    {
        Ok(true) => Ok(()),
        Ok(false) => bail!("Non-zero status code"),
        Err(e) => bail!("{e:?}"),
    }
}

pub async fn create(user: User) -> anyhow::Result<()> {
    let path = Path::new("/home/").join(&user.name);
    std::fs::create_dir(path.clone())?;
    std::fs::copy(home().join("rm-applications"), path.join("rm-applications"))?;
    std::fs::create_dir(path.clone().join(".ssh")).unwrap();
    std::fs::write(path.clone().join(".ssh").join("authorized_keys"), user.keys.join("\n")).unwrap();

    // TODO: Authkeys might require chmod

    match Command::new("sudo")
        .args(["chown", &format!("{}:{}", user.name, user.name), &path.display().to_string(), "-R"])
        .status()
        .await
        .map(|r| r.success())
    {
        Ok(true) => Ok(()),
        Ok(false) => bail!("Non-zero status code"),
        Err(e) => bail!("{e:?}"),
    }
}
