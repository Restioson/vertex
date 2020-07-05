use backtrace::Backtrace;

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

#[derive(Clone, Debug)]
pub struct DeserializeError {
    pub flavour: ErrorFlavour,
    pub backtrace: Backtrace,
}

impl DeserializeError {
    pub fn invalid_enum_variant() -> DeserializeError {
        DeserializeError {
            flavour: ErrorFlavour::InvalidEnumVariant,
            backtrace: Backtrace::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ErrorFlavour {
    InvalidUuid(uuid::Error),
    NullField,
    InvalidEnumVariant,
    ProtobufError(prost::DecodeError),
    IntOutOfRange,
}

impl From<uuid::Error> for DeserializeError {
    fn from(err: uuid::Error) -> Self {
        DeserializeError {
            backtrace: Backtrace::new(),
            flavour: ErrorFlavour::InvalidUuid(err),
        }
    }
}

impl From<prost::DecodeError> for DeserializeError {
    fn from(err: prost::DecodeError) -> Self {
        DeserializeError {
            backtrace: Backtrace::new(),
            flavour: ErrorFlavour::ProtobufError(err),
        }
    }
}

impl From<std::option::NoneError> for DeserializeError {
    fn from(_err: std::option::NoneError) -> Self {
        DeserializeError {
            backtrace: Backtrace::new(),
            flavour: ErrorFlavour::NullField,
        }
    }
}

impl From<std::num::TryFromIntError> for DeserializeError {
    fn from(_err: std::num::TryFromIntError) -> Self {
        DeserializeError {
            backtrace: Backtrace::new(),
            flavour: ErrorFlavour::IntOutOfRange,
        }
    }
}
