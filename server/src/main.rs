#![feature(type_alias_impl_trait, generic_associated_types, type_ascription)]

use std::convert::Infallible;
use std::fs;
use std::fs::OpenOptions;
use std::num::NonZeroU32;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use directories::ProjectDirs;
use futures::StreamExt;
use governor::clock::DefaultClock;
use governor::state::keyed::DashMapStateStore;
use governor::{Quota, RateLimiter};
use log::{info, LevelFilter};
use warp::reply::Reply;
use warp::Filter;
use xtra::prelude::*;
use xtra::Disconnected;

use client::ActiveSession;
use database::Database;
use vertex::prelude::*;

use crate::client::{session::WebSocketMessage, Authenticator};
use crate::community::{Community, CommunityActor};
use crate::config::Config;
use crate::database::{DbResult, MalformedInviteCode};
use clap::{App, Arg};

mod auth;
mod client;
mod community;
mod config;
mod database;

#[derive(Clone)]
pub struct Global {
    pub database: Database,
    pub config: Arc<Config>,
    pub ratelimiter: ArcSwap<RateLimiter<DeviceId, DashMapStateStore<DeviceId>, DefaultClock>>,
}

/// Marker trait for `vertex_common` structs that are actor messages too
trait VertexActorMessage: Send + 'static {
    type Result: Send;
}

impl VertexActorMessage for ClientSentMessage {
    type Result = MessageConfirmation;
}

impl VertexActorMessage for Edit {
    type Result = ();
}

struct IdentifiedMessage<T: VertexActorMessage> {
    user: UserId,
    device: DeviceId,
    message: T,
}

impl<T> xtra::Message for IdentifiedMessage<T>
where
    T: VertexActorMessage,
    T::Result: 'static,
{
    type Result = Result<T::Result, Error>;
}

fn new_ratelimiter() -> RateLimiter<DeviceId, DashMapStateStore<DeviceId>, DefaultClock> {
    RateLimiter::dashmap(Quota::per_minute(NonZeroU32::new(90u32).unwrap()))
}

async fn refresh_ratelimiter(
    rl: ArcSwap<RateLimiter<DeviceId, DashMapStateStore<DeviceId>, DefaultClock>>,
) {
    use tokio::time::Instant;
    let duration = Duration::from_secs(60 * 60); // 1/hr
    let mut timer = tokio::time::interval_at(Instant::now() + duration, duration);

    loop {
        timer.tick().await;
        rl.store(Arc::new(new_ratelimiter()));
    }
}

fn handle_disconnected(actor_name: &'static str) -> impl Fn(Disconnected) -> Error {
    move |_| {
        log::warn!(
            "{} actor disconnected. This may be a timing anomaly.",
            actor_name
        );
        Error::Internal
    }
}

fn setup_logging(config: &Config) {
    let dirs = ProjectDirs::from("", "vertex_chat", "vertex_server")
        .expect("Error getting project directories");
    let dir = dirs.data_dir().join("logs");

    fs::create_dir_all(&dir)
        .unwrap_or_else(|_| panic!("Error creating log dirs ({})", dir.to_string_lossy()));

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
    let args = App::new("Vertex server")
        .version("0.1")
        .author("Restioson <restiosondev@gmail.com>")
        .about("Server for the Vertex chat application https://github.com/Restioson/vertex")
        .arg(
            Arg::with_name("add-admin")
                .short("A")
                .long("add-admin")
                .value_name("USERNAME")
                .help("Adds an admin with all permissions")
                .takes_value(true),
        )
        .get_matches();

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

    for name in args.values_of("add-admin").into_iter().flat_map(|x| x) {
        let id = database
            .get_user_by_name(name.to_string())
            .await
            .expect("Error promoting user to admin")
            .expect(&format!("Invalid username {} to add as admin", name))
            .id;

        database
            .promote_to_admin(id, AdminPermissionFlags::ALL)
            .await
            .expect(&format!("Error promoting user {} to admin", name))
            .expect(&format!("Error promoting user {} to admin", name));

        info!(
            "User {} successfully promoted to admin with all permissions!",
            name
        );
    }

    load_communities(database.clone()).await;

    let config = Arc::new(config);
    let global = Global {
        database,
        config: config.clone(),
        ratelimiter: ArcSwap::from_pointee(new_ratelimiter()),
    };

    tokio::spawn(refresh_ratelimiter(global.ratelimiter.clone()));

    let global = warp::any().map(move || global.clone());

    let authenticate = warp::path("authenticate")
        .and(global.clone())
        .and(warp::query())
        .and(warp::ws())
        .and_then(
            |global: Global, authenticate, ws: warp::ws::Ws| async move {
                let response: Box<dyn warp::Reply> =
                    match self::login(global.clone(), ws, authenticate).await {
                        Ok(response) => Box::new(response),
                        Err(e) => return reply_err(e),
                    };
                Ok(response)
            },
        );

    let register = warp::path("register")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(
            |global, bytes| async move { reply_protobuf(self::register(global, bytes).await) },
        );

    let create_token = warp::path("create")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(|global, bytes| async move {
            reply_protobuf(self::create_token(global, bytes).await)
        });

    let revoke_token = warp::path("revoke")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(|global, bytes| async move {
            reply_protobuf(self::revoke_token(global, bytes).await)
        });

    let refresh_token = warp::path("refresh")
        .and(global.clone())
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(|global, bytes| async move {
            reply_protobuf(self::refresh_token(global, bytes).await)
        });

    let invite = warp::path!("invite" / String)
        //  .and(warp::header::<String>("host")) // https://github.com/seanmonstar/warp/issues/432
        .and(global.clone())
        .and_then(|invite, global| self::invite_reply(global, invite));

    let token = warp::path("token").and(create_token.or(revoke_token).or(refresh_token));
    let client = warp::path("client").and(authenticate.or(register.or(token)));
    let routes = invite.or(client);
    let routes = warp::path("vertex").and(routes);

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
fn reply_err(err: AuthError) -> Result<Box<dyn warp::Reply>, Infallible> {
    Ok(Box::new(AuthResponse::Err(err).into(): Vec<u8>))
}

#[inline]
fn reply_protobuf(res: AuthResponse) -> Result<Box<dyn warp::Reply>, Infallible> {
    Ok(Box::new(res.into(): Vec<u8>))
}

async fn login(
    global: Global,
    ws: warp::ws::Ws,
    login: Login,
) -> Result<impl warp::Reply, AuthError> {
    let authenticator = Authenticator {
        global: global.clone(),
    };

    let details = authenticator.login(login.device, login.token).await?;
    let (user, device, perms) = details;

    match client::session::insert(global.database.clone(), user, device).await? {
        Ok(_) => {
            let upgrade = ws.on_upgrade(move |websocket| {
                let (sink, stream) = websocket.split();

                let session = ActiveSession {
                    ws: sink,
                    global,
                    heartbeat: Instant::now(),
                    user,
                    device,
                    perms,
                };
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

async fn register(global: Global, bytes: bytes::Bytes) -> AuthResponse {
    let register = match AuthRequest::from_protobuf_bytes(&bytes)? {
        AuthRequest::RegisterUser(register) => register,
        _ => return AuthResponse::Err(AuthError::WrongEndpoint),
    };

    let credentials = register.credentials;
    let display_name = register
        .display_name
        .unwrap_or_else(|| credentials.username.clone());

    let authenticator = Authenticator { global };
    authenticator.create_user(credentials, display_name).await
}

async fn create_token(global: Global, bytes: bytes::Bytes) -> AuthResponse {
    let create_token = match AuthRequest::from_protobuf_bytes(&bytes)? {
        AuthRequest::CreateToken(create) => create,
        _ => return AuthResponse::Err(AuthError::WrongEndpoint),
    };

    let authenticator = Authenticator { global };
    authenticator
        .create_token(create_token.credentials, create_token.options)
        .await
}

async fn refresh_token(global: Global, bytes: bytes::Bytes) -> AuthResponse {
    let refresh_token = match AuthRequest::from_protobuf_bytes(&bytes)? {
        AuthRequest::RefreshToken(refresh) => refresh,
        _ => return AuthResponse::Err(AuthError::WrongEndpoint),
    };

    let authenticator = Authenticator { global };
    authenticator
        .refresh_token(refresh_token.credentials, refresh_token.device)
        .await
}

async fn revoke_token(global: Global, bytes: bytes::Bytes) -> AuthResponse {
    let revoke_token = match AuthRequest::from_protobuf_bytes(&bytes)? {
        AuthRequest::RevokeToken(revoke) => revoke,
        _ => return AuthResponse::Err(AuthError::WrongEndpoint),
    };

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
                <meta property="vertex:invite_code" content="{invite_code}"/>
                <meta property="vertex:invite_name" content="{community}"/>
                <meta property="vertex:invite_description" content="{description}"/>
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
        invite_code = invite_code,
        community = community_record.name,
        description = Community::desc_or_default(&community_record.description),
    );

    Ok(Ok(html))
}
