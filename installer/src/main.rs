use {
    anyhow::{anyhow, bail},
    futures::{future::join_all, StreamExt},
    log::{info, warn, LevelFilter},
    std::{
        env,
        fs::OpenOptions,
        io::Write,
        path::{Path, PathBuf},
        process::{self, Command, Stdio},
    },
    tokio::fs,
};

const CLOUD_IMAGES: &str = "https://cloud-images.ubuntu.com/noble/current/";
pub const UBUNTU: &str = "noble-server-cloudimg-amd64.img";

async fn download(url: String, file: PathBuf) -> anyhow::Result<()> {
    if file.exists() {
        info!("File {} exists, skipping download.", file.display());
        return Ok(());
    }

    let mut file = OpenOptions::new().create_new(true).write(true).open(file)?;
    let res = reqwest::get(url.clone()).await.map_err(|e| anyhow!("Failed to fetch URL: {e:?}"))?;

    let size = res.content_length().unwrap_or_default() as f64;
    let mut stream = res.bytes_stream();

    let mut prc;
    let mut last_prc = 0.0;
    let mut bytes = 0.0;

    while let Some(by) = stream.next().await {
        let chunk = by.map_err(|e| anyhow!("Failed to process byte: {e:?}"))?;
        file.write_all(&chunk).map_err(|e| anyhow!("Failed to write chunk: {e:?}"))?;
        bytes += chunk.len() as f64;

        // Update progress
        prc = (bytes / size) * 100.0;
        if prc - last_prc > 1.0 {
            last_prc = prc;
        } else {
            continue;
        }

        info!("Downloaded {:.2}%", prc);
    }

    Ok(())
}

fn run(c: &mut Command) -> anyhow::Result<()> {
    c.status().map(|s| s.success().then_some(()).ok_or(anyhow!("Command failed")))??;
    Ok(())
}

async fn cleanup(p: &[&Path]) -> anyhow::Result<()> {
    join_all(p.iter().map(|v| async move {
        if fs::try_exists(v).await.unwrap_or(false) {
            match fs::remove_file(v).await.map(|_| ()) {
                Ok(_) => fs::write(v, &[]).await,
                e => e,
            }
        } else {
            Ok(())
        }
    }))
    .await
    .into_iter()
    .collect::<Result<(), _>>()?;
    Ok(())
}

async fn packages() -> anyhow::Result<Vec<String>> {
    let pkgs = ["mtools", "libvirt-clients", "libvirt-daemon", "rsync", "swtpm", "swtpm-tools"];

    let output = Command::new("apt").args(["list", "--installed"]).output()?;
    let output = String::from_utf8_lossy(&output.stdout);
    let lines = output.lines();

    let not_installed =
        pkgs.iter().filter(|p| !lines.clone().any(|l| l.contains(*p))).map(|p| p.to_string()).collect::<Vec<_>>();

    Ok(not_installed)
}

fn _ovmf() -> (PathBuf, PathBuf) {
    (
        [Path::new("/usr/share/ovmf/x64/OVMF_CODE.secboot.4m.fd"), Path::new("/usr/share/ovmf/x64/OVMF_CODE.ms.4m.fd")]
            .iter()
            .find(|p| p.exists())
            .expect("Found no OVMF firmware")
            .to_path_buf(),
        [Path::new("/usr/share/ovmf/x64/OVMF_VARS.secboot.4m.fd"), Path::new("/usr/share/ovmf/x64/OVMF_VARS.fd")]
            .iter()
            .find(|p| p.exists())
            .expect("Found no OVMF firmware")
            .to_path_buf(),
    )
}

fn host_ssh_key() -> anyhow::Result<String> {
    let keys = Command::new("fish").args(["-c", "cat ~/.ssh/id*.pub"]).output()?;
    let key = String::from_utf8_lossy(&keys.stdout)
        .split('\n')
        .filter(|s| !s.trim().is_empty())
        .next_back()
        .unwrap_or_default()
        .trim()
        .split(' ')
        .take(2) // Remove ssh:name
        .collect::<Vec<_>>()
        .join(" ")
        .to_string();

    if key.is_empty() {
        bail!("No ssh key found in ~/.ssh");
    }

    Ok(key)
}

fn logger() {
    let mut builder = pretty_env_logger::formatted_builder();
    if let Ok(rust_log) = env::var("RUST_LOG") {
        builder.parse_filters(&rust_log);
    } else {
        builder.filter_level(LevelFilter::Info);
    }
    builder.init();
}

fn kill_if_exists() -> anyhow::Result<()> {
    Command::new("virsh").args(["--connect", "qemu:///session", "destroy", "odde"]).stderr(Stdio::null()).status()?;
    Command::new("virsh").args(["--connect", "qemu:///session", "undefine", "odde"]).stderr(Stdio::null()).status()?;
    Ok(())
}

fn gb_to_gib(g: f32) -> f32 { g * 0.9313226 }

#[allow(non_upper_case_globals)]
const user_data: &str = include_str!("../../files/cc.yaml");

#[allow(non_upper_case_globals)]
const domainxml: &str = include_str!("../../files/odde.xml");

#[tokio::main]
async fn main() {
    logger();

    match packages().await {
        Ok(v) if !v.is_empty() => {
            warn!("Missing required packages: {}", v.join(", "));
            process::exit(1);
        }
        Err(e) => {
            warn!("Failed to check installed packages: {:?}", e);
            process::exit(1);
        }
        _ => {}
    }

    if let Err(err) = kill_if_exists() {
        warn!("Failed to clean destroy existing agent: {:?}", err);
        process::exit(1);
    }

    let ram = 2.0; // Gb
    let storage = 5; // Gb
    let vcpus = 2;

    let wd = Path::new("odde");
    let _ = fs::create_dir(wd).await;
    info!("Installing ODDE with {ram}gb memory, {storage}gb storage, and {vcpus} vcpus");

    info!("Downloading Ubuntu image...");
    let img_path = wd.join("odde.img");
    download(format!("{CLOUD_IMAGES}{UBUNTU}"), img_path.clone()).await.unwrap();

    // let nvram_path = wd.join("odde.nvram");
    let cidata_path = wd.join("odde.cidata");
    let user_data_path = wd.join("user-data");
    let meta_data_path = wd.join("meta-data");
    let xml_path = wd.join("odde.xml");

    if let Err(err) = cleanup(&[/* &nvram_path, */ &cidata_path, &user_data_path, &meta_data_path, &xml_path]).await {
        warn!("Failed to clean working dir (sans odde.img): {:?}", err);
        process::exit(1);
    }

    fs::write(
        xml_path.clone(),
        domainxml
            .replace("{memory_gib}", &(gb_to_gib(ram) as i32).to_string())
            .replace("{vcpus}", &vcpus.to_string())
            // .replace("{ovmf_code}", &ovmf().0.display().to_string())
            // .replace("{ovmf_vars}", &ovmf().1.display().to_string())
            .replace("{uuid}", &String::from_utf8(Command::new("uuidgen").output().unwrap().stdout).unwrap())
            // .replace("{nvram_path}", &nvram_path.display().to_string())
            .replace("{vm_image_path}", &img_path.canonicalize().unwrap().display().to_string())
            .replace("{cidata_image_path}", &cidata_path.canonicalize().unwrap().display().to_string())
            .replace("{mac_address}", &mac_address::get_mac_address().unwrap().unwrap().to_string()),
    )
    .await
    .unwrap();

    fs::write(meta_data_path.clone(), "instance-id: odde\nhostname: odde\n".to_string()).await.unwrap();

    info!("Fetching SSH key...");

    match host_ssh_key() {
        Ok(key) => {
            fs::write(user_data_path.clone(), user_data.replace("{ssh_key}", &key)).await.unwrap();
        }
        Err(err) => {
            warn!("Failed to fetch host ssh key: {:?}", err);
            process::exit(1);
        }
    }

    info!("Truncating cidata file...");
    if let Err(err) =
        run(Command::new("truncate").args(["--size", "2M", &cidata_path.canonicalize().unwrap().display().to_string()]))
    {
        warn!("Failed to truncate cidata file: {:?}", err);
        process::exit(1);
    }

    info!("Creating vfat filesystem on cidata file...");
    if let Err(err) =
        run(Command::new("mkfs.vfat").args(["-n", "CIDATA", &cidata_path.canonicalize().unwrap().display().to_string()]))
    {
        warn!("Failed to create vfat filesystem on cidata file: {:?}", err);
        process::exit(1);
    }

    info!("Copying user-data and meta-data files to cidata...");
    if let Err(err) = run(Command::new("mcopy").args([
        "-oi",
        &cidata_path.canonicalize().unwrap().display().to_string(),
        &user_data_path.canonicalize().unwrap().display().to_string(),
        &meta_data_path.canonicalize().unwrap().display().to_string(),
        "::",
    ])) {
        warn!("Failed to copy user-data and meta-data files to cidata: {:?}", err);
        process::exit(1);
    }

    // if let Err(err) = fs::copy(ovmf().1, &nvram_path).await {
    //     warn!("Failed to copy NVRAM file: {:?}", err);
    //     process::exit(1);
    // }

    info!("Resizing image file...");
    if let Err(err) = run(Command::new("qemu-img").args([
        "resize",
        &img_path.canonicalize().unwrap().display().to_string(),
        &format!("{}G", storage),
    ])) {
        warn!("Failed to resize image file: {:?}", err);
        process::exit(1);
    }

    info!("Defining virtual machine...");
    if let Err(err) = run(Command::new("virsh").args([
        "--connect",
        "qemu:///session",
        "define",
        &xml_path.canonicalize().unwrap().display().to_string(),
    ])) {
        warn!("Failed to define virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Starting virtual machine...");
    if let Err(err) = run(Command::new("virsh").args(["--connect", "qemu:///session", "start", "odde"])) {
        warn!("Failed to start virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Forwarding ssh to :2222...");
    if let Err(err) = run(Command::new("virsh").args([
        "--connect",
        "qemu:///session",
        "qemu-monitor-command",
        "odde",
        "--hmp",
        "--cmd",
        "hostfwd_add tcp::2222-:22",
    ])) {
        warn!("Failed to forward virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Virtual machine started successfully");
}
