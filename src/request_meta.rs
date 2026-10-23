use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::request::Parts,
};
use loco_rs::prelude::*;

use crate::settings::AppSettings;

/// Who is calling and which request this is. Stored with every audit entry.
#[derive(Debug, Clone, Default)]
pub struct RequestMeta {
    pub ip: Option<String>,
    pub request_id: Option<String>,
}

impl FromRequestParts<AppContext> for RequestMeta {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut Parts,
        ctx: &AppContext,
    ) -> std::result::Result<Self, Self::Rejection> {
        let request_id = parts
            .headers
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        // The socket address cannot be faked. The header can, so it is only believed when the app
        // sits behind our own reverse proxy (`trust_proxy`), and then only its LAST entry, which is
        // the one that proxy added itself.
        let mut ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string());
        if AppSettings::from_ctx(ctx).trust_proxy {
            if let Some(forwarded) = parts
                .headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.rsplit(',').next())
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
            {
                ip = Some(forwarded);
            }
        }
        Ok(Self { ip, request_id })
    }
}
