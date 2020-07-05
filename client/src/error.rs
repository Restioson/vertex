use std::fmt::{self, Debug};
use vertex::{prelude::*, proto::DeserializeError};

#[derive(Debug)]
pub enum Error {
    InvalidUrl,
    Http(hyper::Error),
    Websocket(tungstenite::Error),
    Timeout,
    ErrorResponse(vertex::responses::Error),
    AuthErrorResponse(AuthError),
    UnexpectedMessage {
        expected: &'static str,
        got: Box<dyn Debug + Send>,
    },

    DeserializeError(DeserializeError),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use self::Error::*;
        match self {
            InvalidUrl => write!(f, "Invalid url"),
            Http(http) => {
                if http.is_connect() {
                    write!(f, "Couldn't connect to server")
                } else {
                    write!(f, "Network error")
                }
            }
            Websocket(ws) => write!(f, "{}", ws),
            Timeout => write!(f, "Connection timed out"),
            ErrorResponse(err) => write!(f, "{}", err),
            AuthErrorResponse(err) => write!(f, "{}", err),
            UnexpectedMessage { expected, got } => write!(
                f,
                "Received unexpected message: expected {}, got {:#?}",
                expected, got
            ),
            DeserializeError(_) => write!(f, "Failed to deserialize message"),
        }
    }
}

impl From<hyper::Error> for Error {
    fn from(error: hyper::Error) -> Self {
        Error::Http(error)
    }
}

impl From<tungstenite::Error> for Error {
    fn from(error: tungstenite::Error) -> Self {
        Error::Websocket(error)
    }
}

impl From<hyper::http::uri::InvalidUri> for Error {
    fn from(_: hyper::http::uri::InvalidUri) -> Self {
        Error::InvalidUrl
    }
}

impl From<AuthError> for Error {
    fn from(error: AuthError) -> Self {
        Error::AuthErrorResponse(error)
    }
}

impl From<url::ParseError> for Error {
    fn from(_: url::ParseError) -> Self {
        Error::InvalidUrl
    }
}

impl From<DeserializeError> for Error {
    fn from(err: DeserializeError) -> Self {
        Error::DeserializeError(err)
    }
}
#[macro_export]
macro_rules! expect {
    (if let $pat:pat = $v:ident { $expr: expr }) => {
        defile::expr! {
            match $v {
                $pat => $expr,
                other => Err(Error::UnexpectedMessage {
                    expected: expect!(@$pat),
                    got: Box::new(other),
                }),
            }
        }
    };

    (
        @parsed [$($parsed:tt)*]
        $ident:ident
        $($rest:tt)*
    ) => (expect! {
        @parsed [$($parsed)* $ident]
        $($rest)*
    });

    (
        @parsed [$($parsed:tt)*]
        ::
        $($rest:tt)*
    ) => (expect! {
        @parsed [$($parsed)* ::]
        $($rest)*
    });

    (
        @parsed [$($parsed:tt)*]
        $($otherwise:tt)*
    ) => (
        stringify!($($parsed)*)
    );

    (
        $($path:tt)*
    ) => (expect! {
        @parsed []
        $($path)*
    });
}
