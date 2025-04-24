use {
    anyhow::{anyhow, bail, Result},
    log::{info, warn, LevelFilter},
    odde::ty::Config,
    std::{
        env,
        env::args,
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
        process::{self, Command, Stdio},
    },
    tokio::fs,
};

const CLOUD_IMAGES: &str = "https://cloud-images.ubuntu.com/noble/current/";
const UBUNTU: &str = "noble-server-cloudimg-amd64.img";

#[allow(non_upper_case_globals)]
const user_data: &str = include_str!("../../files/user-data.yaml");

async fn download_if_missing(url: &str, path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }

    info!("Downloading {url}...");
    let status = Command::new("curl").args(["-L", url, "-o", &path.display().to_string()]).status()?;

    if !status.success() {
        return Err(anyhow!("Failed to download image"));
    }
    Ok(())
}

fn host_ssh_key() -> anyhow::Result<Vec<String>> {
    let keys = Command::new("ssh-add").arg("-L").output()?;
    let keys = String::from_utf8_lossy(&keys.stdout).lines().map(|l| l.to_string()).collect::<Vec<_>>();

    if keys.is_empty() {
        bail!("No ssh key found in agent");
    }

    Ok(keys)
}

fn logger() {
    let mut builder = pretty_env_logger::formatted_builder();
    builder.filter_level(if env::var("RUST_LOG").is_ok() { LevelFilter::Debug } else { LevelFilter::Info });
    builder.init();
}

#[tokio::main]
async fn main() -> Result<()> {
    logger();

    let config_str =
        fs::read_to_string("./config.toml").await.map_err(|e| anyhow!("Failed to read ./config.toml: {e:?}"))?;
    let config = toml::from_str::<Config>(&config_str).map_err(|e| anyhow!("Invalid ./config.toml: {e:?}"))?;

    let work_dir = PathBuf::from("odde");
    fs::create_dir_all(&work_dir).await?;

    // Download Ubuntu cloud image
    let img_path = work_dir.join("ubuntu.img");
    download_if_missing(&format!("{CLOUD_IMAGES}{UBUNTU}"), &img_path).await?;

    // Create cloud-init config files
    let user_data_path = work_dir.join("user-data");
    let meta_data_path = work_dir.join("meta-data");

    match host_ssh_key() {
        Ok(keys) => {
            fs::write(
                user_data_path.clone(),
                user_data
                    .replace("#keys", &keys.iter().map(|k| format!("      - \"{k}\"")).collect::<Vec<_>>().join("\n"))
                    .replace("#config", &config_str.lines().map(|k| format!("      {k}")).collect::<Vec<_>>().join("\n")),
            )
            .await
            .unwrap();
        }
        Err(err) => {
            warn!("Failed to fetch host ssh key: {:?}", err);
            process::exit(1);
        }
    }

    fs::write(&meta_data_path, "instance-id: odde\nlocal-hostname: odde\n").await?;

    // Create cloud-init ISO
    let seed_path = work_dir.join("seed.iso");
    if let Err(e) = Command::new("cloud-localds")
        .args([
            &seed_path.display().to_string(),
            &user_data_path.display().to_string(),
            &meta_data_path.display().to_string(),
        ])
        .status()
    {
        warn!("Failed to create cloud-init ISO: {e}");
        process::exit(1);
    }

    if args().any(|a| a.contains("--dry")) {
        info!("--dry provided, cancelling early :)");
        process::exit(0);
    };

    // Resize image
    if let Err(e) = Command::new("qemu-img")
        .args(["resize", &img_path.display().to_string(), &format!("{}G", config.vm.storage)])
        .status()
    {
        warn!("Failed to resize image: {e}");
        process::exit(1);
    }

    // Start QEMU
    let qemu = Command::new("qemu-system-x86_64")
        .args([
            "-machine",
            "accel=kvm:tcg",
            // "-cpu", "host",
            "-m",
            &format!("{}G", config.vm.memory),
            "-smp",
            "2",
            "-nographic",
            "-drive",
            &format!("file={},if=virtio,format=qcow2", img_path.display()),
            "-drive",
            &format!("file={},if=virtio,format=raw", seed_path.display()),
            "-netdev",
            "user,id=net0,hostfwd=tcp::2222-:22",
            "-device",
            "virtio-net-pci,netdev=net0",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();

    if let Err(e) = qemu {
        warn!("Failed to start QEMU: {e}");
        process::exit(1);
    }

    let mut line = String::new();
    let mut stdout = BufReader::new(qemu.unwrap().stdout.take().unwrap());
    while stdout.read_line(&mut line).is_ok() {
        if line.contains("Permit User Sessions") && line.contains("OK") {
            let _ = Command::new("ssh-keygen")
                .args(["-f", "~/.ssh/known_hosts", "-R", "[localhost]:2222"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .stdin(Stdio::null())
                .spawn()
                .map(|mut s| s.wait());

            info!("Building odde-service");
            match Command::new("cargo").args(["build", "--all", "--release"]).status().map(|v| v.success()) {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Setting homedir permissions");
            match Command::new("ssh")
                .args([
                    "odde@localhost",
                    "-p",
                    "2222",
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                    "sudo mkdir -p /home/odde && sudo chown -R odde:odde /home/odde && sudo chmod 755 /home/odde",
                ])
                .status()
                .map(|v| v.success())
            {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Copying odde binary");
            match Command::new("rsync")
                .args([
                    "-azhP",
                    "-e",
                    "ssh -p 2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
                    "./target/release/odde",
                    "odde@localhost:/home/odde/odde",
                ])
                .status()
                .map(|v| v.success())
            {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Copying odde-pam binary");
            match Command::new("rsync")
                .args([
                    "-azhP",
                    "-e",
                    "ssh -p 2222 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null",
                    "./target/release/odde-pam",
                    "odde@localhost:/home/odde/odde-pam",
                ])
                .status()
                .map(|v| v.success())
            {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Setting binary permissions");
            match Command::new("ssh")
                .args([
                    "odde@localhost",
                    "-p",
                    "2222",
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                    "sudo chown odde:odde /home/odde/odde && sudo chmod 755 /home/odde/odde",
                ])
                .status()
                .map(|v| v.success())
            {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Restarting odde service");
            match Command::new("ssh")
                .args([
                    "odde@localhost",
                    "-p",
                    "2222",
                    "-o",
                    "StrictHostKeyChecking=no",
                    "-o",
                    "UserKnownHostsFile=/dev/null",
                    "sudo systemctl enable odde && sudo systemctl start odde",
                ])
                .status()
                .map(|v| v.success())
            {
                Ok(false) => warn!("Non-zero status code"),
                Err(e) => warn!("Failed to run: {e:?}"),
                _ => {}
            }

            info!("Done!");
            process::exit(0);
        }
    }

    Ok(())
}
