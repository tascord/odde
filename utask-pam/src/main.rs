use std::{env::var, process};

fn main() {
    let env = var("SSH_AUTH_INFO_0").unwrap_or_default();
    ipsea::send_command(
        "utask",
        &utask::ty::UTaskRequest { key: env },
        Some(|_: bool| {
            process::exit(0);
        }),
    ).unwrap();
}
