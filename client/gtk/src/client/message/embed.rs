use vertex::*;

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
