use std::{
    collections::VecDeque,
    env::var,
    process::{self, Command},
};

const BANNER: &str = include_str!("../../files/banner");
const BANNER_IMG: &str = include_str!("../../files/banner_img");

fn time() -> String {
    Command::new("timedatectl")
        .args(["show", "-p", "TimeUSec", "--value"])
        .output()
        .map(|o| String::from_utf8(o.stdout).ok())
        .ok()
        .flatten()
        .ok_or("")
        .unwrap()
}

fn banner() {
    let img = BANNER_IMG.lines().map(|l| l.to_string()).collect::<Vec<_>>();
    let mut info = VecDeque::from_iter(BANNER.replace("#time", &time()).lines().map(|l| l.to_string()));
    let padding = img.len().saturating_sub(info.len());
    if padding > 1 {
        let half_pad = padding / 2;
        for _ in 0..half_pad {
            info.push_back(String::new());
            info.push_front(String::new());
        }
    }

    img.into_iter().zip(info).for_each(|(a, b)| {
        println!("{a}\t{}", b);
    });
}

fn main() {
    let env = var("SSH_AUTH_INFO_0").unwrap_or_default();
    ipsea::send_command(
        "odde",
        &odde::ty::ODDERequest { key: env },
        Some(|_: bool| {
            banner();
            process::exit(0);
        }),
    )
    .unwrap();
}
