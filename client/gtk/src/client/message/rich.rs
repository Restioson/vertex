use linkify::{LinkFinder, LinkKind};

use crate::client::EmbedCache;
use crate::client::message::MessageEmbed;

#[derive(Debug, Clone)]
pub struct RichMessage {
    pub text: String,
    pub links: Vec<String>,
}

impl RichMessage {
    pub fn parse(content: String) -> RichMessage {
        let finder = LinkFinder::new();
        let links = finder
            .links(&content)
            .filter(|link| *link.kind() == LinkKind::Url)
            .map(|link| link.as_str().to_string())
            .collect();

        RichMessage { text: content, links }
    }

    pub fn has_embeds(&self) -> bool {
        !self.links.is_empty()
    }

    pub async fn load_embeds(&self, cache: &EmbedCache) -> impl Iterator<Item = MessageEmbed> {
        let embeds = self.links.iter().cloned()
            .map(|url| {
                let cache = cache.clone();
                async move {
                    cache.get(url).await
                }
            });

        futures::future::join_all(embeds).await
            .into_iter()
            .filter_map(|e| e)
    }
}
