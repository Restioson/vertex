use chrono::{DateTime, Utc};
use native_tls::TlsConnector;
use std::convert::Into;
use std::io::{self, Cursor};
use std::time::{Duration, Instant};
use url::Url;
use vertex_common::*;
use websocket::client::ClientBuilder;
use websocket::stream::sync::{TcpStream, TlsStream};
use websocket::sync::Client;
use websocket::{OwnedMessage, WebSocketError};

pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(2);

pub struct Config {
    pub url: Url,
}

pub struct Vertex {
    socket: Client<TlsStream<TcpStream>>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub device_id: Option<DeviceId>,
    logged_in: bool,
    heartbeat: Instant,
}

impl Vertex {
    pub fn new(config: Config) -> Self {
        let socket = ClientBuilder::from_url(&config.url)
            .connect_secure(Some(
                TlsConnector::builder()
                    .danger_accept_invalid_certs(true) // TODO needed for self signed certs
                    .build()
                    .expect("Error setting TLS settings"),
            ))
            .expect("Error connecting to websocket");

        socket
            .stream_ref()
            .get_ref()
            .set_read_timeout(Some(Duration::from_micros(1)))
            .unwrap();

        Vertex {
            socket,
            username: None,
            display_name: None,
            device_id: None,
            logged_in: false,
            heartbeat: Instant::now(),
        }
    }

    pub fn handle(&mut self) -> Option<Action> {
        let message = match self.receive() {
            Some(Ok(m)) => m,
            Some(Err(e)) => return Some(Action::Error(e)),
            None => return None,
        };

        match message {
            ServerMessage::Response {
                response,
                request_id: _,
            } => {
                match response {
                    // TODO associate with a particular message id: @gegy1000
                    RequestResponse::Success(_) => None,
                    RequestResponse::Error(e) => Some(Action::Error(Error::ServerError(e))),
                }
            }
            ServerMessage::Error(e) => Some(Action::Error(Error::ServerError(e))),
            ServerMessage::Message(m) => Some(Action::AddMessage(m.into())),
            ServerMessage::SessionLoggedOut => {
                self.username = None;
                self.display_name = None;
                self.device_id = None;
                self.logged_in = false; // TODO proper log out function

                Some(Action::LoggedOut)
            }
            other => panic!("message {:?} is unimplemented", other),
        }
    }

    fn send(&mut self, msg: ClientRequest) -> Result<(), Error> {
        self.socket
            .send_message(&OwnedMessage::Binary(msg.into()))
            .map_err(Error::WebSocketError)
    }

    fn request(&mut self, msg: ClientMessage) -> Result<RequestId, Error> {
        let request = ClientRequest::new(msg);
        let request_id = request.request_id;
        self.send(request)?;
        Ok(request_id)
    }

    fn receive(&mut self) -> Option<Result<ServerMessage, Error>> {
        if Instant::now().duration_since(self.heartbeat) > HEARTBEAT_TIMEOUT {
            return Some(Err(Error::ServerTimedOut));
        }

        let msg = match self.socket.recv_message() {
            Ok(msg) => Ok(msg),
            Err(WebSocketError::NoDataAvailable) => return None,
            Err(WebSocketError::IoError(e)) => {
                if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut {
                    return None;
                } else {
                    Err(WebSocketError::IoError(e))
                }
            }
            Err(e) => Err(e),
        };

        let bin = match msg {
            Ok(OwnedMessage::Binary(bin)) => bin,
            Ok(OwnedMessage::Pong(_)) => {
                self.heartbeat = Instant::now();
                return None;
            }
            Ok(_) => return Some(Err(Error::InvalidServerMessage)),
            Err(e) => return Some(Err(Error::WebSocketError(e))),
        };

        let mut bin = Cursor::new(bin);
        Some(serde_cbor::from_reader(&mut bin).map_err(|_| Error::InvalidServerMessage))
    }

    fn receive_blocking(&mut self) -> Result<ServerMessage, Error> {
        // TODO eventual timeout
        loop {
            match self.receive() {
                Some(res) => return res,
                None => (),
            };
        }
    }

    pub fn create_user(
        &mut self,
        username: &str,
        display_name: &str,
        password: &str,
    ) -> Result<UserId, Error> {
        if !self.logged_in {
            let request_id = self.request(ClientMessage::CreateUser {
                username: username.to_string(),
                display_name: display_name.to_string(),
                password: password.to_string(),
            })?;

            let msg = self.receive_blocking()?;
            match msg.clone() {
                ServerMessage::Response {
                    response,
                    request_id: response_id,
                } => {
                    match response {
                        // TODO do this more asynchronously @gegy1000
                        RequestResponse::Success(Success::User { id })
                            if response_id == request_id =>
                        {
                            Ok(id)
                        }
                        RequestResponse::Error(e) => Err(Error::ServerError(e)),
                        _ => Err(Error::IncorrectServerMessage(msg)),
                    }
                }
                msg @ _ => Err(Error::IncorrectServerMessage(msg)),
            }
        } else {
            Err(Error::AlreadyLoggedIn)
        }
    }

    pub fn change_username(&mut self, new_username: &str) -> Result<(), Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::ChangeUsername {
            new_username: new_username.to_string(),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        self.username = Some(new_username.to_string());
                        self.change_display_name(new_username)?;
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn change_display_name(&mut self, new_display_name: &str) -> Result<(), Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::ChangeDisplayName {
            new_display_name: new_display_name.to_string(),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        self.display_name = Some(new_display_name.to_string());
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<(), Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::ChangePassword {
            old_password: old_password.to_string(),
            new_password: new_password.to_string(),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        // TODO request re-login here later @gegy1000
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    // TODO pub just for testing. @gegy1000 if you could integrate this into the login flow...
    pub fn refresh_token(
        &mut self,
        device_id: DeviceId,
        username: &str,
        password: &str,
    ) -> Result<(), Error> {
        let request_id = self.request(ClientMessage::RefreshToken {
            device_id,
            username: username.to_string(),
            password: password.to_string(),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn login(
        &mut self,
        token: Option<(DeviceId, AuthToken)>,
        username: &str,
        password: &str,
    ) -> Result<(DeviceId, AuthToken), Error> {
        if self.logged_in {
            return Err(Error::AlreadyLoggedIn);
        }

        let (device_id, token) = match token {
            Some(token) => token,
            // TODO allow user to configure these parameters?
            None => self.create_token(username, password, None, TokenPermissionFlags::ALL)?,
        };

        let request_id = self.request(ClientMessage::Login {
            device_id,
            token: token.clone(),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::User { id: _ })
                        if response_id == request_id =>
                    {
                        self.username = Some(username.to_string());
                        self.display_name = Some(username.to_string()); // TODO configure this
                        self.device_id = Some(device_id);
                        self.logged_in = true;
                        Ok((device_id, token))
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn create_room(&mut self) -> Result<RoomId, Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::CreateRoom)?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::Room { id }) if response_id == request_id => {
                        Ok(id)
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            ServerMessage::Error(e) => Err(Error::ServerError(e)),
            _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn join_room(&mut self, room: RoomId) -> Result<(), Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::JoinRoom(room))?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) if response_id == request_id => {
                        Ok(())
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            ServerMessage::Error(e) => Err(Error::ServerError(e)),
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    /// Sends a message
    pub fn send_message(&mut self, msg: String, to_room: RoomId) -> Result<(), Error> {
        if self.logged_in {
            let request_id = self.request(ClientMessage::SendMessage(ClientSentMessage {
                to_room,
                content: msg,
            }))?;

            let msg = self.receive_blocking()?;
            match msg.clone() {
                ServerMessage::Response {
                    response,
                    request_id: response_id,
                } => {
                    match response {
                        // TODO do this more asynchronously @gegy1000
                        RequestResponse::Success(Success::NoData) if response_id == request_id => {
                            Ok(())
                        }
                        RequestResponse::Error(e) => Err(Error::ServerError(e)),
                        _ => Err(Error::IncorrectServerMessage(msg)),
                    }
                }
                msg @ _ => Err(Error::IncorrectServerMessage(msg)),
            }
        } else {
            Err(Error::NotLoggedIn)
        }
    }

    /// Should be called once every `HEARTBEAT_INTERVAL`
    pub fn heartbeat(&mut self) -> Result<(), Error> {
        self.socket
            .send_message(&OwnedMessage::Ping(vec![]))
            .map_err(Error::WebSocketError)
    }

    fn create_token(
        &mut self,
        username: &str,
        password: &str,
        expiration_date: Option<DateTime<Utc>>,
        permission_flags: TokenPermissionFlags,
    ) -> Result<(DeviceId, AuthToken), Error> {
        let request_id = self.request(ClientMessage::CreateToken {
            username: username.to_string(),
            password: password.to_string(),
            device_name: None, // TODO
            expiration_date,
            permission_flags,
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::Token { device_id, token }) => {
                        if response_id == request_id {
                            Ok((device_id, token))
                        } else {
                            Err(Error::IncorrectServerMessage(msg))
                        }
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }

    pub fn revoke_token(&mut self, password: &str, to_revoke: DeviceId) -> Result<(), Error> {
        self.revoke_token_inner(Some(password), to_revoke)?;

        if let Some(current_device_id) = self.device_id {
            if current_device_id == to_revoke {
                self.logged_in = false;
                self.device_id = None;
                self.username = None;
                self.display_name = None;
            }
        }

        Ok(())
    }

    pub fn revoke_current_token(&mut self) -> Result<(), Error> {
        self.revoke_token_inner(None, self.device_id.ok_or(Error::NotLoggedIn)?)?;
        self.logged_in = false;
        self.device_id = None;
        self.username = None;
        self.display_name = None;

        Ok(())
    }

    fn revoke_token_inner(
        &mut self,
        password: Option<&str>,
        to_revoke: DeviceId,
    ) -> Result<(), Error> {
        if !self.logged_in {
            return Err(Error::NotLoggedIn);
        }

        let request_id = self.request(ClientMessage::RevokeToken {
            device_id: to_revoke,
            password: password.map(|s| s.to_string()),
        })?;

        let msg = self.receive_blocking()?;
        match msg.clone() {
            ServerMessage::Response {
                response,
                request_id: response_id,
            } => {
                match response {
                    // TODO do this more asynchronously @gegy1000
                    RequestResponse::Success(Success::NoData) => {
                        if response_id == request_id {
                            Ok(())
                        } else {
                            Err(Error::IncorrectServerMessage(msg))
                        }
                    }
                    RequestResponse::Error(e) => Err(Error::ServerError(e)),
                    _ => Err(Error::IncorrectServerMessage(msg)),
                }
            }
            msg @ _ => Err(Error::IncorrectServerMessage(msg)),
        }
    }
}

#[derive(Debug)]
pub struct Message {
    pub author: String,
    pub room: RoomId,
    pub content: String,
}

impl From<ForwardedMessage> for Message {
    fn from(msg: ForwardedMessage) -> Self {
        Message {
            author: format!("{}", msg.author.0),
            room: msg.room,
            content: msg.content,
        }
    }
}

/// An action that the GUI should take
#[derive(Debug)]
pub enum Action {
    AddMessage(Message),
    LoggedOut,
    Error(Error),
}

#[derive(Debug)]
pub enum Error {
    NotLoggedIn,
    AlreadyLoggedIn,
    WebSocketError(WebSocketError),
    /// A message from the server that doesn't deserialize correctly
    InvalidServerMessage,
    /// A message from the server that doesn't make sense in a specific context
    IncorrectServerMessage(ServerMessage),
    ServerError(ServerError),
    ServerTimedOut,
}
