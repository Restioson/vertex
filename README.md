# Vertex
![Vertex logo](client/gtk/icon.png)

Vertex is a WIP end-to-end encrypted, decentralised, federated chat platform built on top of 
[MLS](https://messaginglayersecurity.rocks).

## Installation
1. Install the latest Rust stable compiler
2. Install [wasm-pack](https://rustwasm.github.io/wasm-pack/installer/)
3. Install npm
4. Install the packages for the client: `cd client/electron && npm install`

## Running
To run the server, do `cargo run` in the `server/` directory. You can pass it a port to run on,
e.g `cargo run -- 8081`.

To run the client, do `wasm-pack build`

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
