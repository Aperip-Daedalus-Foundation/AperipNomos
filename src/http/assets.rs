use axum::{
    body::Body,
    http::header,
    response::{IntoResponse, Response},
};

const CSP: &str = "default-src 'self'; base-uri 'none'; connect-src 'self'; font-src 'self'; form-action 'self'; frame-ancestors 'none'; img-src 'self'; object-src 'none'; script-src 'self'; style-src 'self'";
const HTML_CONTENT_TYPE: &str = "text/html; charset=utf-8";
const CSS_CONTENT_TYPE: &str = "text/css; charset=utf-8";
const JAVASCRIPT_CONTENT_TYPE: &str = "text/javascript; charset=utf-8";
const SVG_CONTENT_TYPE: &str = "image/svg+xml";
const HTML_CACHE_CONTROL: &str = "no-store";
const STATIC_CACHE_CONTROL: &str = "public, max-age=3600";

pub(super) struct EmbeddedAsset {
    body: &'static str,
    content_type: &'static str,
    cache_control: &'static str,
}

impl EmbeddedAsset {
    const fn html(body: &'static str) -> Self {
        Self {
            body,
            content_type: HTML_CONTENT_TYPE,
            cache_control: HTML_CACHE_CONTROL,
        }
    }

    const fn static_asset(body: &'static str, content_type: &'static str) -> Self {
        Self {
            body,
            content_type,
            cache_control: STATIC_CACHE_CONTROL,
        }
    }
}

impl IntoResponse for EmbeddedAsset {
    fn into_response(self) -> Response {
        Response::builder()
            .header(header::CONTENT_TYPE, self.content_type)
            .header(header::CACHE_CONTROL, self.cache_control)
            .header("content-security-policy", CSP)
            .header("x-content-type-options", "nosniff")
            .header("referrer-policy", "no-referrer")
            .header("x-frame-options", "DENY")
            .body(Body::from(self.body))
            .expect("embedded asset response headers must be valid")
    }
}

pub(super) async fn public_home() -> EmbeddedAsset {
    EmbeddedAsset::html(include_str!("assets/public.html"))
}

pub(super) async fn public_detail() -> EmbeddedAsset {
    EmbeddedAsset::html(include_str!("assets/detail.html"))
}

pub(super) async fn admin_home() -> EmbeddedAsset {
    EmbeddedAsset::html(include_str!("assets/admin.html"))
}

pub(super) async fn stylesheet() -> EmbeddedAsset {
    EmbeddedAsset::static_asset(include_str!("assets/app.css"), CSS_CONTENT_TYPE)
}

pub(super) async fn public_script() -> EmbeddedAsset {
    EmbeddedAsset::static_asset(include_str!("assets/public.js"), JAVASCRIPT_CONTENT_TYPE)
}

pub(super) async fn detail_script() -> EmbeddedAsset {
    EmbeddedAsset::static_asset(include_str!("assets/detail.js"), JAVASCRIPT_CONTENT_TYPE)
}

pub(super) async fn admin_script() -> EmbeddedAsset {
    EmbeddedAsset::static_asset(include_str!("assets/admin.js"), JAVASCRIPT_CONTENT_TYPE)
}

pub(super) async fn favicon() -> EmbeddedAsset {
    EmbeddedAsset::static_asset(include_str!("assets/favicon.svg"), SVG_CONTENT_TYPE)
}
