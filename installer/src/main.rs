use {
    anyhow::anyhow,
    futures::{future::join_all, StreamExt},
    log::{info, warn},
    std::{
        env,
        fs::{File, OpenOptions},
        io::{Read, Write},
        path::{Path, PathBuf},
        process::{self, Command},
    },
    tokio::fs,
};

const CLOUD_IMAGES: &str = "https://cloud-images.ubuntu.com/noble/current/";
pub const UBUNTU: &str = "noble-server-cloudimg-amd64.img";

pub fn temp_file(p: PathBuf) -> PathBuf { env::temp_dir().join(p) }

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
            fs::remove_file(v).await.map(|_| ())
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
    let pkgs = ["mtools", "libvirt-clients", "libvirt-daemon", "rsync"];

    let output = Command::new("apt").args(["list", "--installed"]).output()?;
    let output = String::from_utf8_lossy(&output.stdout);
    let lines = output.lines();

    let not_installed =
        pkgs.iter().filter(|p| !lines.clone().any(|l| l.contains(*p))).map(|p| p.to_string()).collect::<Vec<_>>();

    Ok(not_installed)
}

fn ovmf() -> (PathBuf, PathBuf) {
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

fn host_ssh_key() -> String {
    let mut key = String::new();
    if let Ok(mut file) = File::open("/etc/ssh/ssh_host_rsa_key.pub") {
        file.read_to_string(&mut key).unwrap();
    } else {
        warn!("Failed to read host ssh key");
    }
    key
}

#[allow(non_upper_case_globals)]
const user_data: &str = include_str!("../../files/cc.yaml");

#[allow(non_upper_case_globals)]
const domainxml: &str = include_str!("../../files/odde.xml");

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

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

    let ram = 2048;
    let storage = 5; // Gb
    let vcpus = 2;

    let wd = Path::new("oode");
    let _ = fs::create_dir(wd).await;

    info!("Downloading Ubuntu image...");
    let img_path = wd.join("oode.img");
    download(format!("{CLOUD_IMAGES}{UBUNTU}"), img_path.clone()).await.unwrap();

    // let nvram_path = wd.join("odde.nvram");
    let cidata_path = wd.join("oode.cidata");
    let user_data_path = wd.join("user-data");
    let meta_data_path = wd.join("meta-data");
    let xml_path = wd.join("odde.xml");

    if let Err(err) = cleanup(&[/* &nvram_path, */ &cidata_path, &user_data_path, &meta_data_path, &xml_path]).await {
        warn!("Failed to clean working dir (sans oode.img): {:?}", err);
        process::exit(1);
    }

    fs::write(
        xml_path.clone(),
        domainxml
            .replace("{memory_gib}", &((ram as f32 * 0.9313226) as i32).to_string())
            .replace("{vcpus}", &vcpus.to_string())
            // .replace("{ovmf_code}", &ovmf().0.display().to_string())
            // .replace("{ovmf_vars}", &ovmf().1.display().to_string())
            .replace("{uuid}", &String::from_utf8(Command::new("uuidgen").output().unwrap().stdout).unwrap())
            // .replace("{nvram_path}", &nvram_path.display().to_string())
            .replace("{vm_image_path}", &img_path.display().to_string())
            .replace("{cidata_image_path}", &cidata_path.display().to_string())
            .replace("{mac_address}", &mac_address::get_mac_address().unwrap().unwrap().to_string()),
    )
    .await
    .unwrap();

    fs::write(user_data_path.clone(), user_data.replace("{ssh_key}", &host_ssh_key())).await.unwrap();

    fs::write(meta_data_path.clone(), "instance-id: odde\nhostname: odde\n".to_string()).await.unwrap();

    info!("Truncating cidata file...");
    if let Err(err) = run(Command::new("truncate").args(["--size", "2M", &cidata_path.display().to_string()])) {
        warn!("Failed to truncate cidata file: {:?}", err);
        process::exit(1);
    }

    info!("Creating vfat filesystem on cidata file...");
    if let Err(err) = run(Command::new("mkfs.vfat").args(["-n", "CIDATA", &cidata_path.display().to_string()])) {
        warn!("Failed to create vfat filesystem on cidata file: {:?}", err);
        process::exit(1);
    }

    info!("Copying user-data and meta-data files to cidata...");
    if let Err(err) = run(Command::new("mcopy").args([
        "-oi",
        &cidata_path.display().to_string(),
        &user_data_path.display().to_string(),
        &meta_data_path.display().to_string(),
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
    if let Err(err) =
        run(Command::new("qemu-img").args(["resize", &img_path.display().to_string(), &format!("{}G", storage)]))
    {
        warn!("Failed to resize image file: {:?}", err);
        process::exit(1);
    }

    info!("Defining virtual machine...");
    if let Err(err) =
        run(Command::new("virsh").args(["--connect", "qemu:///session", "define", &xml_path.display().to_string()]))
    {
        warn!("Failed to define virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Starting virtual machine...");
    if let Err(err) = run(Command::new("virsh").args(["--connect", "qemu:///session", "start", "odde"])) {
        warn!("Failed to start virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Virtual machine started successfully");
}
