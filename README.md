# Vertex
![Vertex logo](client/gtk/icon.png)

Vertex is a WIP end-to-end encrypted, decentralised, federated chat platform built on top of 
[MLS](https://messaginglayersecurity.rocks).

## Installation

### Development
1. Install the latest Rust stable compiler
2. Install gtk+, cairo, and glib development libraries
3. Install the openssl development package

### Server - Docker
To install the server via Docker, you will require Docker and Docker Compose. Once they are
installed, simply run `docker-compose up` in the main directory (`vertex/`). Add `--build` to rebuild for new changes.

**Warning:** *First time* compilation may take very long for the server (~10min). Grab a cup of coffee ;).
Luckily, you only need to do this once, except *if* the dependencies change *and* you are using Docker.

## Running
To run the server, do `cargo run` in the `server/` directory. You can pass it a port to run on,
e.g `cargo run -- 8081`.

To run the client, do `wasm-pack build`

## Configuration

### Server

The configuration file will be located in the standard configuration directories per platform, or in a similar location:

| Linux                                                             | Windows                                          | macOS                                      |
|-------------------------------------------------------------------|--------------------------------------------------|--------------------------------------------|
| `$XDG_CONFIG_HOME/vertex_server` or `$HOME/.config/vertex_server` | `{FOLDERID_RoamingAppData}/vertex_server/config` | `$HOME/Library/Preferences/<project_path>` |

On Docker, it should be located at ``

| Key                          | Value                                                                                                                                                                                                                               | Default                            |
|------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------|
| `max_password_len`           | The maximum password length that a user may enter.  It must be greater than or equal to 1 and the minimum password length. This is applied only for future passwords -- it is not retroactively applied. Should be a large value.   | 1000                               |
| `min_password_len`           | The minimum password length that a user may enter. It must be greater than 8. This is applied only for future passwords -- it is not retroactively applied. **Serious security consideration should be taken before altering.**     | 12                                 |
| `max_username_len`           | The maximum username length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future usernames -- it is not retroactively applied.                             | 64                                 |
| `min_username_len`           | The minimum username length that a user may enter. It must be greater than or equal to 1. This is applied only for future usernames -- it is not retroactively applied.                                                             | 1                                  |
| `max_display_name_len`       | The maximum display name length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future display names -- it is not retroactively applied.                     | 64                                 |
| `min_display_name_len`       | The minimum password length that a user may enter. It must be  greater than or equal to 1. This is  applied only for future display names -- it is not retroactively applied.                                                       | 1                                  |
| `profile_pictures`           | The directory to serve profile pictures from.                                                                                                                                                                                       | `./files/images/profile_pictures/` |
| `tokens_sweep_interval_secs` | How often to sweep the database for possibly expired tokens in seconds. A warning will be printed if this is less than the time taken to complete a single sweep.                                                                   | 1800 (30min)                       |
| `token_stale_days`           | How many days it takes for a token to become stale and require the user to refresh it with their password.                                                                                                                          | 7 (1 week)                         |
| `token_expiry_days`          | How many days it takes for a token to expire and the device to be deleted from the user's account.                                                                                                                                  | 90 (~3 months)                     |

It is written in TOML.

## Objectives

- [ ] Basic Messaging:
  - [x] Message routing to locally-connected clients
  - [x] Message editing
  - [x] Message deletion
  - [ ] Message history
- [ ] Client:
  - [x] Basic GUI client
  - [ ] Make it nicer to use -- gui, not commands
- [x] Login & persistent identity
- [ ] Federation
- [ ] Encryption
  - [ ] MLS
  - [ ] KeyTransparency or similar for Authentication Service
- [ ] Voice chat

## Licensing

The project is licensed under the GNU AGPL v3.

## Contributions
- [@Restioson](https://github.com/Restioson): programming
- [@gegy1000](https://github.com/gegy1000): programming
- @oof oof: icon
