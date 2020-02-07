use crate::*;

pub type ResponseResult = Result<OkResponse, ErrResponse>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OkResponse {
    NoData,
    AddCommunity { community: CommunityStructure },
    AddRoom { community: CommunityId, room: RoomStructure },
    MessageId { id: MessageId },
    User { id: UserId },
    Token { device: DeviceId, token: AuthToken },
    Invite { code: InviteCode },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub enum ErrResponse {
    Internal,
    UsernameAlreadyExists,
    InvalidUsername,
    InvalidPassword,
    InvalidDisplayName,
    /// Returned when the user that is sending a message is deleted while processing the message
    UserDeleted,
    DeviceDoesNotExist,
    IncorrectUsernameOrPassword,
    /// User is not able to perform said action with current authentication token, or request to
    /// revoke authentication token requires re-entry of password.
    AccessDenied,
    InvalidRoom,
    InvalidCommunity,
    InvalidInviteCode,
    InvalidUser,
    AlreadyInCommunity,
    TooManyInviteCodes
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMessage {
    pub id: RequestId,
    pub request: ClientRequest,
}

impl ClientMessage {
    pub fn new(request: ClientRequest, id: RequestId) -> Self {
        ClientMessage { request, id }
    }
}

impl Into<Bytes> for ClientMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ClientMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientRequest {
    LogOut,
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    CreateCommunity {
        name: String,
    },
    CreateRoom {
        name: String,
        community: CommunityId,
    },
    CreateInvite {
        community: CommunityId,
        expiration_date: Option<DateTime<Utc>>,
    },
    JoinCommunity(InviteCode),
    Delete(Delete),
    ChangeUsername {
        new_username: String,
    },
    ChangeDisplayName {
        new_display_name: String,
    },
    ChangePassword {
        old_password: String,
        new_password: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    Event(ServerEvent),
    Response {
        id: RequestId,
        result: ResponseResult,
    },
    MalformedMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerEvent {
    ClientReady(ClientReady),
    Message(ForwardedMessage),
    Edit(Edit),
    Delete(Delete),
    SessionLoggedOut,
    AddRoom {
        id: RoomId,
        name: String,
    },
    AddCommunity {
        id: CommunityId,
        name: String,
    },
    RemoveCommunity {
        id: CommunityId,
        reason: RemoveCommunityReason,
    },
}

impl Into<Bytes> for ServerMessage {
    fn into(self) -> Bytes {
        serde_cbor::to_vec(&self).unwrap().into()
    }
}

impl Into<Vec<u8>> for ServerMessage {
    fn into(self) -> Vec<u8> {
        serde_cbor::to_vec(&self).unwrap()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientReady {
    pub user: UserId,
    pub username: String,
    pub display_name: String,
}
