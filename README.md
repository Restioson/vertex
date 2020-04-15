# Vertex
![Vertex logo](client/gtk/res/icon.png)

Vertex is a WIP end-to-end encrypted (not implemented yet), decentralised, federated chat platform.

## Installation

### For Developing
1. Install the latest Rust **nightly** compiler
2. Install gtk+ development libraries  (guide [here](https://gtk-rs.org/docs-src/requirements))
3. Install the openssl development package (`sudo apt install libssl-dev` on Linux)
4. Install OpenAL and libsndfile (guide [here](https://docs.rs/crate/ears/0.7.0))

### Server - Docker
1. Install Docker and Docker Compose. On Linux, this is as simple as `sudo apt install docker.io docker-compose` 
   (sometimes `docker` instead of `docker.io`).
2. **(Optional)** If you do not already have a certificate, you can get one in two different ways. If you are unsure
   which to pick, the simplest is (i) and it will probably work until the release of the first version of Vertex.
   1. You can get an unverified certificate by self-signing one with OpenSSL 
      [guide](https://www.ibm.com/support/knowledgecenter/SSMNED_5.0.0/com.ibm.apic.cmc.doc/task_apionprem_gernerate_self_signed_openSSL.html).
      This currently works for the client, but in the future, it will trigger a warning or cause an error.
   2. You can get a verified certificate for free with LetsEncrypt through its [Certbot](https://certbot.eff.org/instructions). 
      You may also want to put an Nginx/Apache reverse proxy in front of Vertex so that you don't have to restart it every time 
      to renew a certificate 
      [(guide)](https://medium.com/@mightywomble/how-to-set-up-nginx-reverse-proxy-with-lets-encrypt-8ef3fd6b79e5).
3. Copy your certificate and key files to `server/docker/` (named `cert.pem` and `key.pem` respectively).
4. Run `VERTEX_SERVER_PORT=443 docker-compose up` in the main directory `vertex/` (if this does not work, 
   try `VERTEX_SERVER_PORT=443 sudo docker-compose up`). Add `--build` to the end to rebuild for new updates. 
   To set the password for `key.pem`, edit the values in `vertex/.env`. 

**Warning:** *First time* (and in general on Docker) compilation may take very long for the server (~10min). Grab a cup 
of coffee ;).

## Running
To run the server, do `cargo +nightly run` in the `server/` directory.

To run the client, do `cargo +nightly run -- --ip <ip of server>` in the `client/gtk` directory.

#### Deploying - Linux
Run `./flatpak/flatpak.sh` in the `client/gtk` directory. It will ask for `sudo` permissions in order to delete the
`.flatpak-builder` directory (which currently breaks Cargo). Don't just believe us though, go read the shell script for
yourself :)

## Objectives

- [x] Basic Messaging:
  - [x] Message routing to locally-connected clients
  - [x] Message editing
  - [x] Message deletion
  - [x] Message history
- [ ] Client:
  - [x] Basic GUI client
  - [x] Make it nicer to use -- gui, not commands
  - [ ] Settings
    - [ ] Community settings
    - [ ] User settings
    - [ ] Room settings
    - [ ] Styling
- [x] Login & persistent identity
- [ ] Permissions
  - [ ] Administration (instance level)
    - [ ] Bans
  - [ ] Moderation (community level)
    - [ ] Bans
    - [ ] Permissions system
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
