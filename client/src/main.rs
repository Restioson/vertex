use std::io::{self, Cursor, BufRead, Write};
use websocket::client::{ClientBuilder, sync::Client};
use websocket::OwnedMessage;
use websocket::stream::sync::TcpStream;
use uuid::Uuid;
use vertex_common::*;

fn main() {
    let mut client = ClientBuilder::new("ws://127.0.0.1:8080/client/")
        .unwrap()
        .add_protocol("rust-websocket")
        .connect_insecure()
        .unwrap();

    send_message(&mut client, ClientMessage::Login(Login { id: Uuid::new_v4() }));

    loop {
        print!("[S]end or [l]isten? ");
        io::stdout().lock().flush().unwrap();

        let ans = io::stdin().lock().lines().next().unwrap().unwrap();
        match &*ans.trim().to_lowercase() {
            "s" | "send" => send_loop(&mut client),
            "l" | "listen" => listen_loop(&mut client),
            _ => println!("Please input s, send, l, or listen"),
        }
    }

}

fn send_loop(client: &mut Client<TcpStream>) {
    let room = new_room(client);

    println!("Room id: {}", room);

    print!("> ");
    io::stdout().lock().flush().unwrap();

    for line in io::stdin().lock().lines().map(|l| l.unwrap()) {
        send_message(client, ClientMessage::SendMessage(ClientSentMessage {
            content: line,
            to_room: room,
        }));

        print!("> ");
        io::stdout().lock().flush().unwrap();
    }
}

fn listen_loop(client: &mut Client<TcpStream>) {
    print!("Room id > ");
    io::stdout().lock().flush().unwrap();

    let room = Uuid::parse_str(&io::stdin().lock().lines().next().unwrap().unwrap()).unwrap();
    send_message(client, ClientMessage::JoinRoom(room));

    loop {
        match client.recv_message() {
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
        }
    }
}

fn new_room(client: &mut Client<TcpStream>) -> Uuid {
    let bin = serde_cbor::to_vec(&ClientMessage::CreateRoom).unwrap();

    match client.send_message(&OwnedMessage::Binary(bin)) {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{:?}", e);
            let _ = client.send_message(&websocket::Message::close());
            panic!("error sending");
        }
    }

    match client.recv_message() {
        Ok(m) => {
            let bin = match m {
                OwnedMessage::Binary(b) => b,
                _ => panic!("Unexpected non-binary response: {:?}", m),
            };

            let mut bin = Cursor::new(bin);
            let res: ServerMessage = match serde_cbor::from_reader(&mut bin) {
                Ok(m) => m,
                Err(e) => panic!("Invalid response: {:?}", e),
            };

            match res {
                ServerMessage::Success(Success::Room { id }) => id,
                _ => panic!("Invalid response: {:?}", res),
            }
        },
        Err(e) => panic!("{:?}", e),
    }
}

fn send_message(client: &mut Client<TcpStream>, msg: ClientMessage) {
    let bin = serde_cbor::to_vec(&msg).unwrap();

    match client.send_message(&OwnedMessage::Binary(bin)) {
        Ok(()) => (),
        Err(e) => {
            eprintln!("{:?}", e);
            let _ = client.send_message(&websocket::Message::close());
            panic!("error sending");
        }
    }

    match client.recv_message() {
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
    }
}
