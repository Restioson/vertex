# Vertex
![Vertex logo](client/gtk/res/icon.png)

Vertex is a WIP end-to-end encrypted (not implemented yet), decentralised, federated chat platform.

## Installation

### For Developing
1. Install the latest Rust **nightly** compiler
2. Install gtk+ development libraries  (guide [here](https://gtk-rs.org/docs-src/requirements))
3. Install the openssl development package (`sudo apt install libssl-dev` on Linux)

### Server - Docker
1. Install Docker and Docker Compose. On Linux, this is as simple as `sudo apt install docker.io docker-compose` 
   (sometimes `docker` instead of `docker.io`).
2. **(Optional)** If you do not already have a certificate, you can get one in two different ways. If you are unsure
   which to pick, the simplest is (i) and it will probably work until the release of the first version of Vertex.
   1. You can get an unverified certificate by self-signing one with OpenSSL 
      [guide](https://www.ibm.com/support/knowledgecenter/SSMNED_5.0.0/com.ibm.apic.cmc.doc/task_apionprem_gernerate_self_signed_openSSL.html).
      This currently works for the client, but in the future, it will trigger a warning or cause an error.
   2. You can get a verified certificate for free with LetsEncrypt through its [Certbot](https://certbot.eff.org/instructions). 
      You may also want to put an Nginx reverse proxy in front of Vertex 
      so that you don't have to restart it every time to renew a certificate 
      [(guide)](https://medium.com/@mightywomble/how-to-set-up-nginx-reverse-proxy-with-lets-encrypt-8ef3fd6b79e5).
3. Copy your certificate and key files to `server/docker/` (named `cert.pem` and `key.pem` respectively).
4. Run `docker-compose up` in the main directory `vertex/` (if this does not work, try `sudo docker-compose up`).
   Run `docker-compose up --build` to rebuild for new changes. To set the server IP and password for `key.pem`, edit the
   values in `vertex/docker_env.env`

**Warning:** *First time* compilation may take very long for the server (~10min). Grab a cup of coffee ;).
Luckily, you only need to do this once, except *if* the dependencies change *and* you are using Docker.

## Running
To run the server, do `cargo run` in the `server/` directory. You can pass it a port to run on,
e.g `cargo run -- 8081`.

To run the client, do `cargo run -- --ip <ip of server>` in the `client/gtk` directory.

## Server Configuration

The configuration file will be located in the standard configuration directories per platform, or in a similar location:

| Linux                                                             | Windows                                                      | macOS                                                             |
|-------------------------------------------------------------------|--------------------------------------------------------------|-------------------------------------------------------------------|
| `$XDG_CONFIG_HOME/vertex_server` or `$HOME/.config/vertex_server` | `{FOLDERID_RoamingAppData}\vertex_chat\vertex_server\config` | `$HOME/Library/Preferences/vertex_chat.vertex_server`             |

When using Docker, put the `config.toml` in the `server/docker/` folder. Upon changing this file, please make sure to
rebuild the docker image with `docker-compose up --build`.

| Key                          | Value                                                                                                                                                                                                                               | Default                            |
|------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------|
| `max_password_len`           | The maximum password length that a user may enter.  It must be greater than or equal to 1 and the minimum password length. This is applied only for future passwords -- it is not retroactively applied. Should be a large value.   | 1000                               |
| `min_password_len`           | The minimum password length that a user may enter. It must be greater than 8. This is applied only for future passwords -- it is not retroactively applied. **Serious security consideration should be taken before altering.**     | 12                                 |
| `max_username_len`           | The maximum username length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future usernames -- it is not retroactively applied.                             | 64                                 |
| `min_username_len`           | The minimum username length that a user may enter. It must be greater than or equal to 1. This is applied only for future usernames -- it is not retroactively applied.                                                             | 1                                  |
| `max_display_name_len`       | The maximum display name length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future display names -- it is not retroactively applied.                     | 64                                 |
| `min_display_name_len`       | The minimum password length that a user may enter. It must be  greater than or equal to 1. This is  applied only for future display names -- it is not retroactively applied.                                                       | 1                                  |
| `tokens_sweep_interval_secs` | How often to sweep the database for possibly expired tokens in seconds. A warning will be printed if this is less than the time taken to complete a single sweep.                                                                   | 1800 (30min)                       |
| `token_stale_days`           | How many days it takes for a token to become stale and require the user to refresh it with their password.                                                                                                                          | 7 (1 week)                         |
| `token_expiry_days`          | How many days it takes for a token to expire and the device to be deleted from the user's account.                                                                                                                                  | 90 (~3 months)                     |
| `log_level`                  | The minimum log level to display log statements for. Valid options are `trace`, `debug`, `info`, `warn`, and `error`. It must be written in quotation marks (e.g `"info"`).                                                         | `"info"                            |

It is written in TOML. 

The server must also be provided with a certificate and private key pair. They should be named `cert.pem` and `key.pem`
respectively, and be contained in the standard configuration directories, as is the config file. The private key must be
encrypted, and it must not have a passphrase.
When using Docker, put the `cert.pem` and `key.pem` in the `server/docker/` folder. Upon changing these, please make
sure to rebuild the docker image with `docker-compose up --build`. 

The server's log files can be found in the log folder, under the standard data directories:

| Linux                                                                            | Windows                                                          | macOS                                                               |
|----------------------------------------------------------------------------------|------------------------------------------------------------------|---------------------------------------------------------------------|
| `$XDG_DATA_HOME/vertex_server/logs/` or `$HOME/.local/share/vertex_server/logs/` | `{FOLDERID_RoamingAppData}\vertex_chat\vertex_server\data\logs\` | `$HOME/Library/Application Support/vertex_chat.vertex_server/logs/` |

## Objectives

- [ ] Basic Messaging:
  - [x] Message routing to locally-connected clients
  - [x] Message editing
  - [x] Message deletion
  - [ ] Message history
- [ ] Client:
  - [x] Basic GUI client
  - [x] Make it nicer to use -- gui, not commands
- [x] Login & persistent identity
- [ ] Federation
- [ ] Encryption
  - [ ] MLS
  - [ ] KeyTransparency or similar for Authentication Service
- [ ] Voice chat

## Current State and Usability

Vertex is not usable in its current state, and it is not recommended to install it except for development purposes.

## Licensing

The project is licensed under the GNU AGPL v3.

## Contributions
- [@Restioson](https://github.com/Restioson): programming
- [@gegy1000](https://github.com/gegy1000): programming
- @oof oof: icon (licensed under CC0)
