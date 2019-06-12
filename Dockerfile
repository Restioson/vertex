FROM rust:latest as build

WORKDIR /vertex/server
COPY server/Cargo.* .
COPY common ../common

# Build deps
RUN mkdir src/
RUN echo "fn main() {}" > src/main.rs
RUN cargo build --release

# Build project
COPY server/src src
RUN cargo build --release
RUN cp ./target/release/vertex_server ./vertex_server
EXPOSE 8080/tcp
CMD ["./vertex_server", "0.0.0.0:8080"]
