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
installed, simply run `docker-compose up`.

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

| Key                    | Value                                                                                                                                                                                                                               |
|------------------------|-------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `max_password_len`     | The maximum password length that a user may enter.  It must be greater than or equal to 1 and the minimum password length. This is applied only for future passwords -- it is not retroactively applied. Should be a large value.   |
| `min_password_len`     | The minimum password length that a user may enter. It must be greater than 8. This is applied only for future passwords -- it is not retroactively applied. **Serious security consideration should be taken before altering.**     |
| `max_username_len`     | The maximum username length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future usernames -- it is not retroactively applied.                             |
| `min_username_len`     | The minimum username length that a user may enter. It must be greater than or equal to 1. This is applied only for future usernames -- it is not retroactively applied.                                                             |
| `max_display_name_len` | The maximum display name length that a user may enter. It must be greater than or equal to 1 and the minimum password length. This is applied only for future display names -- it is not retroactively applied.                     |
| `min_display_name_len` | The minimum password length that a user may enter. It must be  greater than or equal to 1. This is  applied only for future display names -- it is not retroactively applied.                                                       |
| `max_bio_len`          | The maximum bio/"about me" text length per user.                                                                                                                                                                                    |
| `files_directory`      | The directory to serve static files from (e.g profile pictures). They are served at the root, `/`, so `[files_directory]/images/profile_pictures` is served at `/images/profile_pictures`. It should only be used for public files. |

It is notated with TOML.

## Objectives

- [ ] Basic Messaging:
  - [x] Message routing to locally-connected clients
  - [x] Message editing
  - [x] Message deletion
  - [ ] Message history
- [ ] Client:
  - [x] Basic GUI client
  - [ ] Make it nicer to use -- gui, not commands
- [ ] Login & persistent identity
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
