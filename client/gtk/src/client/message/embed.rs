use std::collections::HashMap;
use std::time::Duration;
use tokio::time;
use hyper::body::Bytes;
use gdk_pixbuf::InterpType;
use gio::Cancellable;
use vertex::prelude::*;
use crate::{Error, Result};
use crate::SharedMut;
use hyper::header;

type EmbedKey = String;

// TODO: drop old entries
#[derive(Clone)]
pub struct EmbedCache {
    cache: SharedMut<HashMap<EmbedKey, Option<MessageEmbed>>>,
}

impl Default for EmbedCache {
    fn default() -> EmbedCache {
        EmbedCache::new()
    }
}

impl EmbedCache {
    pub fn new() -> EmbedCache {
        EmbedCache {
            cache: SharedMut::new(HashMap::new())
        }
    }

    pub async fn get(&self, url: String) -> Option<MessageEmbed> {
        if let Some(embed) = self.get_existing(url.clone()).await {
            return embed;
        }

        let embed = self.load(url.clone()).await;

        let mut cache = self.cache.write().await;
        cache.insert(url, embed.clone());

        embed
    }

    async fn get_existing(&self, url: String) -> Option<Option<MessageEmbed>> {
        let cache = self.cache.read().await;
        cache.get(&url).map(|e| e.as_ref().cloned())
    }

    async fn load(&self, url: String) -> Option<MessageEmbed> {
        match get_link_metadata(&url).await {
            Ok(metadata) => build_embed(url, metadata),
            Err(err) => {
                log::warn!("error trying to load embed from {}: {:?}", url, err);
                let embed = ErrorEmbed {
                    url: url.clone(),
                    title: url,
                    // TODO: display error without debug
                    error: format!("{:?}", err),
                };
                Some(MessageEmbed::Error(embed))
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MessageEmbed {
    OpenGraph(OpenGraphEmbed),
    Invite(InviteEmbed),
    Error(ErrorEmbed),
}

#[derive(Debug, Clone)]
pub struct OpenGraphEmbed {
    pub url: String,
    pub title: String,
    pub description: String,
    pub image: Option<OpenGraphImage>,
}

#[derive(Debug, Clone)]
pub struct OpenGraphImage {
    pub pixbuf: gdk_pixbuf::Pixbuf,
    pub alt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InviteEmbed {
    pub code: InviteCode,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ErrorEmbed {
    pub url: String,
    pub title: String,
    pub error: String,
}

fn build_embed(url: String, metadata: LinkMetadata) -> Option<MessageEmbed> {
    match (metadata.invite, metadata.opengraph) {
        (Some(invite), _) => {
            let embed = InviteEmbed {
                code: invite.code,
                name: invite.name,
                description: invite.description,
            };
            Some(MessageEmbed::Invite(embed))
        }
        (_, Some(og)) => {
            let embed = OpenGraphEmbed {
                url,
                title: og.title,
                description: og.description.unwrap_or_default(),
                image: og.image,
            };
            Some(MessageEmbed::OpenGraph(embed))
        }
        _ => None,
    }
}

pub async fn get_link_metadata(url: &str) -> Result<LinkMetadata> {
    let response = req(url, 400 * 1024).await?;
    let body = String::from_utf8_lossy(&response);

    let html = scraper::Html::parse_document(&body);
    let props = collect_metadata_props(html);

    Ok(build_link_metadata(props).await)
}

async fn req(url: &str, max_size: usize) -> Result<Bytes> {
    type Connector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

    let https = hyper_tls::HttpsConnector::new();
    let client: hyper::Client<Connector, hyper::Body> = hyper::Client::builder()
        .http1_max_buf_size(max_size)
        .build(https);

    let mut url = url.to_string();

    for _ in 0..3 {
        let response = time::timeout(
                Duration::from_secs(5),
                client.get(url.parse::<hyper::Uri>()?),
            )
            .await
            .map_err(|_| Error::Timeout)??;

        match response.headers().get(header::LOCATION) {
            Some(loc) if response.status().is_redirection() => {
                url = loc.to_str().map_err(|_| Error::InvalidUrl)?.to_string();
            },
            _ => {
                let body = response.into_body();
                return Ok(hyper::body::to_bytes(body).await?);
            }
        }
    }

    Err(Error::Timeout)
}

async fn build_link_metadata(mut props: HashMap<String, String>) -> LinkMetadata {
    let mut metadata = LinkMetadata {
        opengraph: None,
        invite: None,
    };

    let invite_details = (
        props.remove("vertex:invite_code"),
        props.remove("vertex:invite_name"),
        props.remove("vertex:invite_description"),
    );
    if let (Some(code), Some(name), Some(description)) = invite_details {
        let code = InviteCode(code);
        metadata.invite = Some(InviteMeta { code, name, description })
    }

    if let Some(title) = props.remove("og:title") {
        let description = props.remove("og:description");
        let image = get_image(props).await;
        metadata.opengraph = Some(OpenGraphMeta { title, description, image })
    }

    metadata
}

async fn get_image(mut props: HashMap<String, String>) -> Option<OpenGraphImage> {
    const MAX_DIM: i32 = 500;

    let img_url = props.remove("og:image").or(props.remove("og:image:url"))?;
    let alt = props.remove("og:image:alt");
    let bytes = req(&img_url, 4 * 1024 * 1024).await.ok()?;
    let bytes = glib::Bytes::from_owned(bytes);
    let input_stream = gio::MemoryInputStream::new_from_bytes(&bytes);
    let pixbuf = gdk_pixbuf::Pixbuf::new_from_stream(&input_stream, None::<&Cancellable>).ok()?;

    let preferred: Option<(i32, i32)> = props.remove("og:image:width")
        .zip(props.remove("og:image:height"))
        .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));
    let pixbuf_dims = (pixbuf.get_width(), pixbuf.get_height());

    let dims = if preferred.map(|d| d.0 <= MAX_DIM && d.1 <= MAX_DIM).unwrap_or(false) {
        preferred.unwrap()
    } else if pixbuf_dims.0 <= MAX_DIM && pixbuf_dims.1 <= MAX_DIM {
        pixbuf_dims
    } else {
        let bigger_side = std::cmp::max(pixbuf_dims.0, pixbuf_dims.1);
        let scale_factor = bigger_side as f64 / MAX_DIM as f64;

        (
            (pixbuf_dims.0 as f64 / scale_factor).round() as i32,
            (pixbuf_dims.1 as f64 / scale_factor).round() as i32
        )
    };

    let pixbuf = pixbuf.scale_simple(dims.0, dims.1, InterpType::Bilinear)?;
    Some(OpenGraphImage { pixbuf, alt, })
}

fn collect_metadata_props(html: scraper::Html) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let select_meta = scraper::Selector::parse("meta").unwrap();
    for element in html.select(&select_meta) {
        let element = element.value();
        let meta_info = (element.attr("property"), element.attr("content"));

        if let (Some(property), Some(content)) = meta_info {
            map.insert(property.to_owned(), content.to_owned());
        }
    }

    map
}

#[derive(Debug, Clone)]
pub struct LinkMetadata {
    opengraph: Option<OpenGraphMeta>,
    pub invite: Option<InviteMeta>,
}

#[derive(Debug, Clone)]
struct OpenGraphMeta {
    title: String,
    description: Option<String>,
    image: Option<OpenGraphImage>,
}

#[derive(Debug, Clone)]
pub struct InviteMeta {
    pub code: InviteCode,
    name: String,
    description: String,
}
