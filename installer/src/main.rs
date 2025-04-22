use std::{fs::File, io::{Read, Write}, ops::Deref, path::{Path, PathBuf}, process::{self, Command}};

use anyhow::anyhow;
use futures::StreamExt;
use log::{info, trace, warn};
use tokio::fs;

const CLOUD_IMAGES: &str = "https://cloud-images.ubuntu.com/noble/current/";
pub const UBUNTU: &str = "noble-server-cloudimg-amd64.img";

#[derive(Clone, Debug)]
struct TFile(PathBuf, bool); // Transgender File
impl AsRef<Path> for TFile {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Deref for TFile {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for TFile {
    fn drop(&mut self) {
        if self.1 {
            trace!("Deleting temp file {}", self.0.display());
            let _ = std::fs::remove_file(self.0.to_path_buf()).inspect_err(|e| eprintln!("Failed to remove temp file {}: {e:?}", self.0.display()));
        }
    }
}

impl TFile {
    pub fn new(p: impl Into<PathBuf>) -> TFile {
        Self(p.into(), true)
    }

    pub fn in_temp_dir(p: impl Into<PathBuf>) -> TFile {
        let mut path = std::env::temp_dir();
        path.push(p.into());
        Self(path, true)
    }

    pub async fn persist(mut self, location: impl Into<PathBuf>) -> anyhow::Result<TFile> {
        let mut new_location = location.into();
        let md = new_location.metadata()?;
        if md.is_dir() {
            new_location.push(self.0.file_name().unwrap());
        }

        fs::rename(self.0.clone(), new_location.clone()).await?;
        self.0 = new_location;
        self.1 = false;
        Ok(self)
    }
}

async fn download(url: String, file: TFile) -> anyhow::Result<TFile> {
    let res = reqwest::get(url.clone())
        .await
        .map_err(|e| anyhow!("Failed to fetch URL: {e:?}"))?;

    let size = res.content_length().unwrap_or_default() as f64;
    let mut stream = res.bytes_stream();

    let mut last_prc = 0.0; 
    let mut prc = 0.0;
    let mut bytes = 0.0;
    while let Some(by) = stream.next().await {
        let chunk = by.map_err(|e| anyhow!("Failed to process byte: {e:?}"))?;
        file.as_ref(&chunk)
            .map_err(|e| anyhow!("Failed to write chunk: {e:?}"))?;
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

    // Rename the temporary file to the final path
    fs::rename(temp_path, final_path)
        .await
        .map_err(|e| anyhow!("Failed to rename file: {e:?}"))?;

    // Return the TFile instance
    Ok(TFile::in_temp_dir(final_path))
}

fn run(c: &mut Command) -> anyhow::Result<()> {
    c.status().map(|s| s.success().then_some(()).ok_or(anyhow!("Command failed")))??;
    Ok(())
}

fn ovmf() -> (PathBuf, PathBuf) {
    (
        [
        Path::new("/usr/share/ovmf/x64/OVMF_CODE.secboot.4m.fd"),
        Path::new("/usr/share/ovmf/x64/OVMF_CODE.ms.4m.fd"),
    ].iter().find(|p| p.exists()).expect("Found no OVMF firmware").to_path_buf(),
    [
        Path::new("/usr/share/ovmf/x64/OVMF_VARS.secboot.4m.fd"),
        Path::new("/usr/share/ovmf/x64/OVMF_VARS.fd"),
    ].iter().find(|p| p.exists()).expect("Found no OVMF firmware").to_path_buf(),
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

const user_data: &str = include_str!("../../files/cc.yaml");
const domainxml: &str = include_str!("../../files/odde.xml");

#[tokio::main]
async fn main() {

    pretty_env_logger::init();

    let ram = 2048;
    let storage = 5; // Gb
    let vcpus = 2;

    let wd = Path::new("oode");
    let _ = fs::create_dir(wd).await;

    info!("Downloading Ubuntu image...");
    let img_path = download(format!("{CLOUD_IMAGES}{UBUNTU}")).await.unwrap();
    let img_path = img_path.persist(wd).await.unwrap();

    let nvram_path = TFile::in_temp_dir("odde.nvram").persist(wd).await.unwrap();
    let cidata_path = TFile::in_temp_dir("oode.cidata").persist(wd).await.unwrap();
    let user_data_path = TFile::in_temp_dir("user-data");
    let meta_data_path = TFile::in_temp_dir("meta-data");
    
    let xml_path = TFile::in_temp_dir("odde.xml");
    fs::write(xml_path.clone(), domainxml
        .replace("{memory_gib}", &(ram as f32 * 0.9313226).to_string())
        .replace("{vcpus}", &vcpus.to_string())
        .replace("{ovmf_code}", &ovmf().0.display().to_string())
        .replace("{ovmf_vars}", &ovmf().1.display().to_string())
        .replace("{uuid}", &String::from_utf8(Command::new("uuidgen").output().unwrap().stdout).unwrap())
        .replace("{nvram_path}", &nvram_path.display().to_string())
        .replace("{vm_image_path}", &img_path.display().to_string())
        .replace("{cidata_image_path}", &cidata_path.display().to_string())
        .replace("{mac_address}", &mac_address::get_mac_address().unwrap().unwrap().to_string())
    ).await.unwrap();

    fs::write(user_data_path.clone(), user_data
        .replace("{ssh_key}", &host_ssh_key())
    ).await.unwrap();

    fs::write(meta_data_path.clone(), format!(
        "instance-id: odde\nhostname: odde\n"
    )).await.unwrap();
    
    if let Err(err) = run(Command::new("truncate").args([
        "--size",
        "2M",
        &cidata_path.display().to_string()
    ])) {
        warn!("Failed to truncate cidata file: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = run(Command::new("mkfs.vfat").args([
        "-n",
        "CIDATA",
        &cidata_path.display().to_string()
    ])) {
        warn!("Failed to create vfat filesystem on cidata file: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = run(Command::new("mcopy").args([
        "-oi",
        &cidata_path.display().to_string(),
        &user_data_path.display().to_string(),
        &meta_data_path.display().to_string(),
        "::"
    ])) {
        warn!("Failed to copy user-data and meta-data files to cidata: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = fs::copy(ovmf().1, &nvram_path).await {
        warn!("Failed to copy NVRAM file: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = run(Command::new("qemu-img").args([
        "resize",
        &img_path.display().to_string(),
        &format!("{}G", storage)
    ])) {
        warn!("Failed to resize image file: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = run(Command::new("virsh").args([
        "--connect",
        "qemu:///session",
        "define",
        &xml_path.display().to_string(),
    ])) {
        warn!("Failed to define virtual machine: {:?}", err);
        process::exit(1);
    }

    if let Err(err) = run(Command::new("virsh").args([
        "--connect",
        "qemu:///session",
        "start",
        "odde"
    ])) {
        warn!("Failed to start virtual machine: {:?}", err);
        process::exit(1);
    }

    info!("Virtual machine started successfully");
   
}
