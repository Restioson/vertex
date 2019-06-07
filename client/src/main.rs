use std::io::Cursor;
use websocket::client::ClientBuilder;
use websocket::{Message, OwnedMessage};
use uuid::Uuid;
use vertex_common::*;

fn main() {
    let client = ClientBuilder::new("ws://127.0.0.1:8080/client/")
        .unwrap()
        .add_protocol("rust-websocket")
        .connect_insecure()
        .unwrap();

    let (mut receiver, mut sender) = client.split().unwrap();

    let msgs = [
        ClientMessage::Login(Login {
            uuid: Uuid::new_v4(),
        }),
    ];

    for msg in &msgs {
        let bin = serde_cbor::to_vec(msg).unwrap();

        match sender.send_message(&OwnedMessage::Binary(bin)) {
            Ok(()) => (),
            Err(e) => {
                eprintln!("{:?}", e);
                let _ = sender.send_message(&Message::close());
                return;
            }
        }

        match receiver.recv_message() {
            Ok(m) => {
                let bin = match m {
                    OwnedMessage::Binary(b) => b,
                    _ => return eprintln!("Unexpected non-binary response: {:?}", m),
                };

                let mut bin = Cursor::new(bin);
                let msg: ServerMessage = match serde_cbor::from_reader(&mut bin) {
                    Ok(m) => m,
                    Err(e) => return eprintln!("Invalid reponse: {:?}", e),
                };

                println!("{:?}", msg);
            },
            Err(e) => eprintln!("{:?}", e),
        };
    }
}
