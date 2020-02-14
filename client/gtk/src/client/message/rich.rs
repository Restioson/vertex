use std::collections::HashMap;

use regex::Regex;

use lazy_static::lazy_static;
use vertex::*;

use crate::client::message::{ErrorEmbed, InviteEmbed, MessageEmbed, OpenGraphEmbed};
use crate::Result;

#[derive(Debug, Clone)]
pub struct RichMessage {
    pub text: String,
    pub links: Vec<String>,
}

impl RichMessage {
    pub fn parse(content: String) -> RichMessage {
        lazy_static! {
            static ref MATCH_LINK: Regex = Regex::new("https?://[/:a-zA-Z.0-9_~-]*").unwrap();
        }

        let mut links = Vec::new();
        for link in MATCH_LINK.captures_iter(&content) {
            links.push(link[0].to_owned());
        }

        RichMessage { text: content, links }
    }

    pub async fn load_embeds(&self) -> impl Iterator<Item = MessageEmbed> {
        let links = futures::future::join_all(
            self.links.iter().cloned()
                .map(|link| async move {
                    let result = get_link_metadata(&link).await;
                    (link, result)
                })
        ).await;

        links.into_iter()
            .filter_map(|(url, result)| match result {
                Ok(metadata) => build_embed(url, metadata),
                Err(err) => {
                    println!("error trying to load embed from {}: {:?}", url, err);
                    let embed = ErrorEmbed {
                        url: url.clone(),
                        title: url.clone(),
                        // TODO: display error without debug
                        error: format!("{:?}", err),
                    };
                    Some(MessageEmbed::Error(embed))
                }
            })
    }
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
                description: og.description,
            };
            Some(MessageEmbed::OpenGraph(embed))
        }
        _ => None,
    }
}

async fn get_link_metadata(url: &str) -> Result<LinkMetadata> {
    type Connector = hyper_tls::HttpsConnector<hyper::client::HttpConnector>;

    let https = crate::https_ignore_invalid_certs();
    let client: hyper::Client<Connector, hyper::Body> = hyper::Client::builder()
        .build(https);

    // TODO: request gzip + max size + timeout
    let response = client.get(url.parse::<hyper::Uri>()?).await?;

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

    match (props.remove("vertex:invite_code"), props.remove("vertex:invite_name")) {
        (Some(code), Some(name)) => {
            let code = InviteCode(code);
            metadata.invite = Some(InviteMeta { code, name })
        }
        _ => (),
    }

    match (props.remove("og:title"), props.remove("og:description")) {
        (Some(title), Some(description)) => {
            metadata.opengraph = Some(OpenGraphMeta { title, description })
        }
        _ => (),
    }

    metadata
}

fn collect_metadata_props(html: scraper::Html) -> HashMap<String, String> {
    let mut map = HashMap::new();

    let select_meta = scraper::Selector::parse("head meta").unwrap();
    for element in html.select(&select_meta) {
        let element = element.value();
        match (element.attr("property"), element.attr("content")) {
            (Some(property), Some(content)) => {
                map.insert(property.to_owned(), content.to_owned());
            }
            _ => (),
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
    description: String,
}

#[derive(Debug, Clone)]
struct InviteMeta {
    code: InviteCode,
    name: String,
}
