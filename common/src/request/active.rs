use crate::*;

pub type ResponseResult = Result<OkResponse, ErrResponse>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum OkResponse {
    NoData,
    AddCommunity { community: CommunityStructure },
    AddRoom { community: CommunityId, room: RoomStructure },
    ConfirmMessage(MessageConfirmation),
    User { id: UserId },
    UserProfile(UserProfile),
    Token { device: DeviceId, token: AuthToken },
    Invite { code: InviteCode },
    RoomUpdate(RoomUpdate),
    MessageHistory(MessageHistory),
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
    TooManyInviteCodes,
    InvalidMessageSelector,
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
#[non_exhaustive]
pub enum ClientRequest {
    LogOut,
    SendMessage(ClientSentMessage),
    EditMessage(Edit),
    GetRoomUpdate {
        community: CommunityId,
        room: RoomId,
        last_received: Option<MessageId>,
        message_count: usize,
    },
    GetMessages {
        community: CommunityId,
        room: RoomId,
        selector: MessageSelector,
        count: usize,
    },
    SelectRoom {
        community: CommunityId,
        room: RoomId,
    },
    DeselectRoom,
    SetAsRead {
        community: CommunityId,
        room: RoomId,
    },
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
    GetUserProfile(UserId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ServerMessage {
    Event(ServerEvent),
    Response {
        id: RequestId,
        result: ResponseResult,
    },
    MalformedMessage,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ServerEvent {
    ClientReady(ClientReady),
    AddMessage {
        community: CommunityId,
        room: RoomId,
        message: Message,
    },
    NotifyMessageReady {
        room: RoomId,
        community: CommunityId,
    },
    Edit(Edit),
    Delete(Delete),
    SessionLoggedOut,
    AddRoom {
        community: CommunityId,
        structure: RoomStructure,
    },
    AddCommunity(CommunityStructure),
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
    pub profile: UserProfile,
    pub communities: Vec<CommunityStructure>,
}
