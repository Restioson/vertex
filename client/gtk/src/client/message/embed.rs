use std::collections::HashMap;
use std::time::Duration;

use tokio::time;

use vertex::prelude::*;

use crate::{Error, Result};
use crate::SharedMut;

type EmbedKey = String;

// TODO: drop old entries
#[derive(Clone)]
pub struct EmbedCache {
    cache: SharedMut<HashMap<EmbedKey, Option<MessageEmbed>>>,
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
                println!("error trying to load embed from {}: {:?}", url, err);
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
}

#[derive(Debug, Clone)]
pub struct InviteEmbed {
    pub code: InviteCode,
    pub name: String,
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
            };
            Some(MessageEmbed::Invite(embed))
        }
        (_, Some(og)) => {
            let embed = OpenGraphEmbed {
                url,
                title: og.title,
                description: og.description.unwrap_or_default(),
            };
            Some(MessageEmbed::OpenGraph(embed))
        }
        _ => None,
    }
}

async fn get_link_metadata(url: &str) -> Result<LinkMetadata> {
    type Connector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

    let https = hyper_tls::HttpsConnector::new();
    let client: hyper::Client<Connector, hyper::Body> = hyper::Client::builder()
        .build(https);

    // TODO: request gzip + max size
    let response = time::timeout(
        Duration::from_secs(5),
        client.get(url.parse::<hyper::Uri>()?),
    )
        .await
        .map_err(|_| Error::Timeout)??;

    let body = response.into_body();
    let body = hyper::body::to_bytes(body).await?;
    let body = String::from_utf8_lossy(&body);

    let html = scraper::Html::parse_document(&body);
    let props = collect_metadata_props(html);

    Ok(parse_link_metadata(props))
}

fn parse_link_metadata(mut props: HashMap<String, String>) -> LinkMetadata {
    let mut metadata = LinkMetadata {
        opengraph: None,
        invite: None,
    };

    let invite_details = (props.remove("vertex:invite_code"), props.remove("vertex:invite_name"));
    if let (Some(code), Some(name)) = invite_details {
        let code = InviteCode(code);
        metadata.invite = Some(InviteMeta { code, name })
    }

    if let Some(title) = props.remove("og:title") {
        let description = props.remove("og:description");
        metadata.opengraph = Some(OpenGraphMeta { title, description })
    }

    metadata
}

fn collect_metadata_props(html: scraper::Html) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let select_meta = scraper::Selector::parse("head meta").unwrap();
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
struct LinkMetadata {
    opengraph: Option<OpenGraphMeta>,
    invite: Option<InviteMeta>,
}

#[derive(Debug, Clone)]
struct OpenGraphMeta {
    title: String,
    description: Option<String>,
}

#[derive(Debug, Clone)]
struct InviteMeta {
    code: InviteCode,
    name: String,
}
