# Vertex
<img alt="Vertex logo" style="text-align: center;" src="https://i.imgur.com/HcHTAYS.png"/>

Vertex is a WIP end-to-end encrypted, decentralised, federated chat platform built on top of 
[MLS](https://messaginglayersecurity.rocks).

## Installation
1. Install the latest Rust stable compiler
2. Install [`wasm-pack`](https://rustwasm.github.io/wasm-pack/installer/)
3. Install `gcc`, `clang`, etc - general build tools (`clang` is important though).

## Running
To run the server, do `cargo run` in the `server/` directory. You can pass it a port to run on,
e.g `cargo run -- 8081`.

## Objectives

- [ ] Basic Messaging:
  - [x] Message routing to locally-connected clients
  - [x] Message editing
  - [x] Message deletion
  - [ ] Message history
- [ ] Client:
  - [ ] Basic GUI electron client
  - [ ] Web client
- [ ] Login & persistent identity
- [ ] Federation
- [ ] Encryption
  - [ ] MLS
  - [ ] KeyTransparency or similar for Authentication Service
- [ ] Voice chat


## Licensing

The project is licensed under the GNU AGPL v3.
