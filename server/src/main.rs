#![feature(type_alias_impl_trait, generic_associated_types, type_ascription)]

use std::convert::Infallible;
use std::fs::OpenOptions;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use std::{env, fmt::Debug, fs};

use directories::ProjectDirs;
use futures::StreamExt;
use log::{info, LevelFilter};
use warp::Filter;
use xtra::prelude::*;
use xtra::Disconnected;

use client::ActiveSession;
use database::Database;
use vertex::*;

use crate::client::{session::WebSocketMessage, Authenticator};
use crate::community::CommunityActor;
use crate::config::Config;
use crate::database::{DbResult, MalformedInviteCode};
use warp::reply::Reply;

mod auth;
mod client;
mod community;
mod config;
mod database;

#[derive(Clone)]
pub struct Global {
    pub database: Database,
    pub config: Arc<Config>,
}

#[derive(Debug, Clone)]
pub struct SendMessage<T: Debug>(T);

impl<T: Debug + Send + 'static> Message for SendMessage<T> {
    type Result = ();
}

/// Marker trait for `vertex_common` structs that are actor messages too
trait VertexActorMessage: Send + 'static {
    type Result: Send;
}

impl VertexActorMessage for ClientSentMessage {
    type Result = MessageId;
}

impl VertexActorMessage for Edit {
    type Result = ();
}

struct IdentifiedMessage<T: VertexActorMessage> {
    user: UserId,
    device: DeviceId,
    message: T,
}

impl<T> Message for IdentifiedMessage<T>
where
    T: VertexActorMessage,
    T::Result: 'static,
{
    type Result = Result<T::Result, ErrResponse>;
}

fn handle_disconnected(actor_name: &'static str) -> impl Fn(Disconnected) -> ErrResponse {
    move |_| {
        log::warn!(
            "{} actor disconnected. This may be a timing anomaly.",
            actor_name
        );
        ErrResponse::Internal
    }
}

fn setup_logging(config: &Config) {
    let dirs = ProjectDirs::from("", "vertex_chat", "vertex_server")
        .expect("Error getting project directories");
    let dir = dirs.data_dir().join("logs");

    fs::create_dir_all(&dir)
        .unwrap_or_else(|_| panic!("Error creating log dirs ({})", dir.to_string_lossy(),));

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}] [{}] [{}] {}",
                chrono::Local::now().to_rfc3339(),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(LevelFilter::from_str(&config.log_level).unwrap())
        .chain(std::io::stdout())
        .chain(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(
                    dir.join(
                        chrono::Local::now()
                            .format("vertex_server_%Y-%m-%d_%H-%M-%S.log")
                            .to_string(),
                    ),
                )
                .expect("Error opening log file"),
        )
        .apply()
        .expect("Error setting logger settings");

    info!("Logging set up");
}

async fn load_communities(db: Database) {
    let stream = db
        .get_all_communities()
        .await
        .expect("Error loading communities");
    futures::pin_mut!(stream);

    while let Some(res) = stream.next().await {
        let community_record = res.expect("Error loading community");
        CommunityActor::load_and_spawn(community_record, db.clone())
            .await
            .expect("Error loading community!");
    }
}

#[tokio::main]
async fn main() {
    println!("Vertex server starting...");

    let config = config::load_config();
    setup_logging(&config);

    let (cert_path, key_path) = config::ssl_config();
    let database = Database::new().await.expect("Error in database setup");
    tokio::spawn(database.clone().sweep_tokens_loop(
        config.token_expiry_days,
        Duration::from_secs(config.tokens_sweep_interval_secs),
    ));
    tokio::spawn(
        database
            .clone()
            .sweep_invite_codes_loop(Duration::from_secs(config.invite_codes_sweep_interval_secs)),
    );

    load_communities(database.clone()).await;

    let config = Arc::new(config);
    let global = Global {
        database,
        config: config.clone(),
    };
    let global = warp::any().map(move || global.clone());

    let authenticate = warp::path("authenticate")
        .and(global.clone())
        .and(warp::query())
        .and(warp::ws())
        .and_then(
            |global: Global, authenticate, ws: warp::ws::Ws| async move {
                let response: Box<dyn warp::Reply> =
                    match self::authenticate(global.clone(), ws, authenticate).await {
                        Ok(response) => Box::new(response),
                        Err(e) => return reply_cbor(Err(e): Result<(), AuthError>),
                    };
                Ok(response)
            },
        );

    let register = warp::path("register")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(|global, bytes| async move { reply_cbor(self::register(global, bytes).await) });

    let create_token = warp::path("create")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(
            |global, bytes| async move { reply_cbor(self::create_token(global, bytes).await) },
        );

    let revoke_token = warp::path("revoke")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(
            |global, bytes| async move { reply_cbor(self::revoke_token(global, bytes).await) },
        );

    let refresh_token = warp::path("refresh")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(
            |global, bytes| async move { reply_cbor(self::refresh_token(global, bytes).await) },
        );

    let invite = warp::path!("invite" / String)
        //  .and(warp::header::<String>("host")) // https://github.com/seanmonstar/warp/issues/432
        .and(global.clone())
        .and_then(|invite, global| self::invite_reply(global, invite));

    let token = warp::path("token").and(create_token.or(revoke_token).or(refresh_token));
    let client = warp::path("client").and(authenticate.or(register.or(token)));
    let routes = invite.or(client);

    info!("Vertex server starting on addr {}", config.ip);

    if config.https {
        warp::serve(routes)
            .tls()
            .cert_path(cert_path)
            .key_path(key_path)
            .run(config.ip)
            .await;
    } else {
        warp::serve(routes).run(config.ip).await;
    }
}

#[inline]
fn reply_cbor<T: serde::Serialize>(value: T) -> Result<Box<dyn warp::Reply>, Infallible> {
    Ok(Box::new(serde_cbor::to_vec(&value).unwrap()))
}

async fn authenticate(
    global: Global,
    ws: warp::ws::Ws,
    authenticate: AuthenticateRequest,
) -> Result<impl warp::Reply, AuthError> {
    let authenticator = Authenticator {
        global: global.clone(),
    };

    let (user, device, perms) = authenticator
        .authenticate(authenticate.device, authenticate.token)
        .await?;

    match client::session::insert(global.database.clone(), user, device).await? {
        Ok(_) => {
            let upgrade = ws.on_upgrade(move |websocket| {
                let (sink, stream) = websocket.split();

                let session = ActiveSession::new(sink, global, user, device, perms);
                let session = session.spawn();

                session.clone().attach_stream(stream.map(WebSocketMessage));

                // if the session fails to spawn, that means it has since been removed. we can ignore the error.
                let _ = client::session::upgrade(user, device, session);

                futures::future::ready(())
            });

            Ok(upgrade)
        }
        Err(_) => Err(AuthError::TokenInUse),
    }
}

async fn register(global: Global, bytes: bytes::Bytes) -> AuthResult<RegisterUserResponse> {
    let register: RegisterUserRequest =
        serde_cbor::from_slice(&bytes).map_err(|_| AuthError::Internal)?;

    let credentials = register.credentials;
    let display_name = register
        .display_name
        .unwrap_or_else(|| credentials.username.clone());

    let authenticator = Authenticator { global };
    authenticator.create_user(credentials, display_name).await
}

async fn create_token(global: Global, bytes: bytes::Bytes) -> AuthResult<CreateTokenResponse> {
    let create_token: CreateTokenRequest =
        serde_cbor::from_slice(&bytes).map_err(|_| AuthError::Internal)?;

    let authenticator = Authenticator { global };
    authenticator
        .create_token(create_token.credentials, create_token.options)
        .await
}

async fn refresh_token(global: Global, bytes: bytes::Bytes) -> AuthResult<()> {
    let refresh_token: RefreshTokenRequest =
        serde_cbor::from_slice(&bytes).map_err(|_| AuthError::Internal)?;

    let authenticator = Authenticator { global };
    authenticator
        .refresh_token(refresh_token.credentials, refresh_token.device)
        .await
}

async fn revoke_token(global: Global, bytes: bytes::Bytes) -> AuthResult<()> {
    let revoke_token: RevokeTokenRequest =
        serde_cbor::from_slice(&bytes).map_err(|_| AuthError::Internal)?;

    let authenticator = Authenticator { global };
    authenticator
        .revoke_token(revoke_token.credentials, revoke_token.device)
        .await
}

async fn invite_reply(
    global: Global,
    //  hostname: String, // https://github.com/seanmonstar/warp/issues/432
    invite_code: String,
) -> Result<Box<dyn Reply>, Infallible> {
    let res = invite(global, invite_code).await;

    match res {
        Ok(Ok(html)) => Ok(Box::new(warp::reply::html(html))),
        _ => {
            let response = http::response::Builder::new()
                .status(404) // Not found
                .body("")
                .unwrap();
            Ok(Box::new(response))
        }
    }
}

async fn invite(
    global: Global,
    //  hostname: String, // https://github.com/seanmonstar/warp/issues/432
    invite_code: String,
) -> DbResult<Result<String, MalformedInviteCode>> {
    let code = InviteCode(invite_code.clone());
    let id = match global.database.get_community_from_invite_code(code).await? {
        Ok(Some(id)) => id,
        _ => return Ok(Err(MalformedInviteCode)),
    };
    let community_record = match global.database.get_community_metadata(id).await? {
        Some(rec) => rec,
        None => return Ok(Err(MalformedInviteCode)),
    };

    let html = format!(
        r#"
            <head>
                <meta property="og:title" content="Vertex Community Invite"/>
                <meta property="og:description" content="You are invited to join {community} on Vertex!"/>
            </head>
            <body>
                <script>
                    // Redirect to vertex://...
                    var no_http = window.location.href.replace("https", "").replace("http", "");
                    window.location.replace("vertex" + no_http);
                </script>
            </script>
        "#,
        //        hostname = hostname, // TODO https://github.com/seanmonstar/warp/issues/432
        // We just use JS as a workaround
        community = community_record.name,
    );

    Ok(Ok(html))
}
