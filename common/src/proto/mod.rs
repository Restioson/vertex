pub mod types {
    include!(concat!(env!("OUT_DIR"), "/vertex.types.rs"));
}

pub mod structures {
    include!(concat!(env!("OUT_DIR"), "/vertex.structures.rs"));
}

pub mod responses {
    include!(concat!(env!("OUT_DIR"), "/vertex.responses.rs"));
}

pub mod events {
    include!(concat!(env!("OUT_DIR"), "/vertex.events.rs"));
}

pub mod requests {
    pub mod auth {
        include!(concat!(env!("OUT_DIR"), "/vertex.requests.auth.rs"));
    }

    pub mod active {
        include!(concat!(env!("OUT_DIR"), "/vertex.requests.active.rs"));
    }

    pub mod administration {
        include!(concat!(
            env!("OUT_DIR"),
            "/vertex.requests.administration.rs"
        ));
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DeserializeError {
    InvalidUuid(uuid::Error),
    NullField,
    InvalidEnumVariant,
    ProtobufError(prost::DecodeError),
}

impl From<uuid::Error> for DeserializeError {
    fn from(err: uuid::Error) -> Self {
        DeserializeError::InvalidUuid(err)
    }
}

impl From<prost::DecodeError> for DeserializeError {
    fn from(err: prost::DecodeError) -> Self {
        DeserializeError::ProtobufError(err)
    }
}

impl From<std::option::NoneError> for DeserializeError {
    fn from(_err: std::option::NoneError) -> Self {
        DeserializeError::NullField
    }
}
