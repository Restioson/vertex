use std::collections::HashMap;
use std::time::Duration;
use tokio::time;
use hyper::body::Bytes;
use vertex::prelude::*;
use hyper::header;
use url::Url;
use scraper::{Selector, Html};
use crate::{Error, Result};

// TODO: drop old entries
#[derive(Clone)]
pub struct EmbedCache {
    cache: HashMap<Url, Option<MessageEmbed>>,
}

impl Default for EmbedCache {
    fn default() -> EmbedCache {
        EmbedCache::new()
    }
}

impl EmbedCache {
    pub fn new() -> EmbedCache {
        EmbedCache { cache: HashMap::new() }
    }

    pub async fn get(&mut self, url: &Url) -> Option<&MessageEmbed> {
        if self.cache.get(url).is_none() {
            let embed = self.load(url).await;
            self.cache.insert(url.clone(), embed);
        }

        self.cache.get(url).and_then(|x| x.as_ref())
    }

    async fn load(&self, url: &Url) -> Option<MessageEmbed> {
        match get_link_metadata(url).await {
            Ok(metadata) => build_embed(metadata),
            Err(err) => {
                log::warn!("error trying to load embed from {}: {:?}", url, err);
                None
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MessageEmbed {
    OpenGraph(OpenGraphEmbed),
    Invite(InviteEmbed),
}

#[derive(Debug, Clone)]
pub struct OpenGraphEmbed {
    pub title: String,
    pub description: String,
    pub image: Option<OpenGraphImage>,
}

#[derive(Debug, Clone)]
pub struct OpenGraphImage {
    pub data: Bytes,
    pub preferred_dimensions: Option<(usize, usize)>,
    pub alt: Option<String>,
}

#[derive(Debug, Clone)]
pub struct InviteEmbed {
    pub code: InviteCode,
    pub name: String,
    pub description: String,
}

fn build_embed(metadata: LinkMetadata) -> Option<MessageEmbed> {
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
                title: og.title,
                description: og.description.unwrap_or_default(),
                image: og.image,
            };
            Some(MessageEmbed::OpenGraph(embed))
        }
        _ => None,
    }
}

pub async fn get_link_metadata(url: &Url) -> Result<LinkMetadata> {
    let response = req(url, 400 * 1024).await?;
    let body = String::from_utf8_lossy(&response);
    let props = collect_metadata_props(Html::parse_document(&body));
    Ok(build_link_metadata(props).await)
}

async fn req(url: &Url, max_size: usize) -> Result<Bytes> {
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
    let img_url = props.remove("og:image").or(props.remove("og:image:url"))?;
    let alt = props.remove("og:image:alt");
    let data = req(&img_url.parse().ok()?, 4 * 1024 * 1024).await.ok()?;

    let preferred_dimensions: Option<(usize, usize)> = props.remove("og:image:width")
        .zip(props.remove("og:image:height"))
        .and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)));

    Some(OpenGraphImage {
        data,
        preferred_dimensions,
        alt,
    })
}

fn collect_metadata_props(html: Html) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let select_meta = Selector::parse("meta").unwrap();
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
