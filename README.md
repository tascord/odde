```
      ·▄▄▄▄  ·▄▄▄▄  ▄▄▄ .▄▄ 
▪     ██▪ ██ ██▪ ██ ▀▄.▀·██▌
 ▄█▀▄ ▐█· ▐█▌▐█· ▐█▌▐▀▀▪▄▐█·
▐█▌.▐▌██. ██ ██. ██ ▐█▄▄▌.▀ 
 ▀█▄▀▪▀▀▀▀▀• ▀▀▀▀▀•  ▀▀▀  ▀ 
```

# On Demand Developer Environment

ODDE is a development environment manager that creates disposable, pre-configured virtual machines for development work. It automatically handles user authentication, environment setup, and cleanup of inactive sessions.

## Features

- 🔐 SSH key-based authentication 
- 🔄 Automatic environment provisioning
- ⏲️ Session timeouts (90 minutes of inactivity)
- 🛠️ Pre-configured development tools
- 📦 Automatic Git repository synchronization
- 🖥️ Custom VM resource allocation

## Installation

1. Clone the repository:
    ```bash
    git clone https://github.com/yourusername/odde.git
    cd odde
    ```

2. Create a configuration file:
    ```bash
    cp config.example.toml config.toml
    ```

3. Edit `config.toml` with your settings:
    ```toml
    [vm]
    memory = 8.0     # GB of RAM
    storage = 90     # GB of storage

    [git]
    key = "your-ssh-key"
    urls = ["git@github.com:username/repo.git"]

    [users]
    username = [
        "ssh-key-1",
        "ssh-key-2"
    ]
    ```

4. Install:
    ```bash
    cargo run -p installer
    ```

## Usage

1. Start the ODDE service:
    ```bash
    ./target/release/installer
    ```

2. Connect to your environment:
    ```bash
    ssh -p 2222 username@localhost
    ```

## Requirements

- Linux host system
- QEMU/KVM
- Rust toolchain
- cloud-init
- SSH agent with loaded keys