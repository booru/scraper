use crate::{
    scraper::{self, ScrapeResult},
    Configuration, ResultCache, State,
};
use anyhow::Result;
use axum::{
    extract::Query,
    http::{self, Request},
    middleware::Next,
    response::{self, IntoResponse},
    Json,
};
use std::sync::Arc;
use tokio::time::Instant;
use tracing::debug;

#[derive(serde::Deserialize, Clone, Debug)]
pub struct ScrapeRequest {
    url: String,
    #[serde(alias = "_method")]
    _method: Option<String>,
}

#[allow(clippy::let_with_type_underscore)]
#[tracing::instrument(skip(req, next))]
pub async fn latency(req: Request<axum::body::Body>, next: Next) -> impl IntoResponse {
    let uri = req.uri().clone();
    debug!("Incoming Request {}", uri);
    let start = Instant::now();

    let mut res = next.run(req).await;

    let time_taken = start.elapsed();
    let time_taken = format!("{:1.4}ms", time_taken.as_secs_f32() * 1000.0);

    debug!("Request {} handled in {}", uri, time_taken);

    res.headers_mut().append(
        "x-time-taken",
        axum::http::HeaderValue::from_str(&time_taken).unwrap(),
    );

    res
}

#[tracing::instrument(skip(req, state, next))]
pub async fn origin_check(
    req: Request<axum::body::Body>,
    state: Arc<State>,
    next: Next,
) -> std::result::Result<impl response::IntoResponse, http::StatusCode> {
    let origin = req.headers().get("Origin").map(|x| x.to_str()).transpose();
    match origin {
        Ok(origin) => {
            if state.is_allowed_origin(origin) {
                Ok(next.run(req).await)
            } else {
                Err(http::StatusCode::NOT_FOUND)
            }
        }
        Err(_) => Err(http::StatusCode::INTERNAL_SERVER_ERROR),
    }
}

#[tracing::instrument(skip(state))]
pub async fn scrape_post(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Json(scrape_req): Json<ScrapeRequest>,
) -> response::Response<String> {
    match scrape_inner(
        &state.config,
        state.result_cache.clone(),
        &state.clone(),
        scrape_req,
    )
    .await
    {
        Ok(v) => v,
        Err(_) => todo!(),
    }
}

#[tracing::instrument(skip(state))]
pub async fn scrape(
    axum::extract::State(state): axum::extract::State<Arc<State>>,
    Query(scrape_req): Query<ScrapeRequest>,
) -> response::Response<String> {
    match scrape_inner(
        &state.config,
        state.result_cache.clone(),
        &state.clone(),
        scrape_req,
    )
    .await
    {
        Ok(v) => v,
        Err(_) => todo!(),
    }
}

#[tracing::instrument(skip(request_cache, state, config))]
pub async fn scrape_inner(
    config: &Configuration,
    request_cache: ResultCache,
    state: &State,
    scrape_req: ScrapeRequest,
) -> Result<response::Response<String>> {
    let url = scrape_req.url.clone();
    let res: std::result::Result<Option<ScrapeResult>, Arc<anyhow::Error>> = request_cache
        .try_get_with(scrape_req.url, scraper::scrape(config, state, &url))
        .await;
    let res = match res {
        Ok(r) => r,
        Err(e) => {
            sentry::integrations::anyhow::capture_anyhow(&e);
            let e = ScrapeResult::from_err(e);
            return Ok(response::Response::builder()
                .status(http::StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_string(&e)?)?);
        }
    };
    let res = match res {
        Some(res) => res,
        None => {
            return Ok(response::Response::builder()
                .status(http::StatusCode::OK)
                .header(http::header::CONTENT_TYPE, "application/json")
                .body(serde_json::to_string(&ScrapeResult::Err(
                    "URL invalid".to_string().into(),
                ))?)?);
        }
    };
    Ok(response::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, "application/json")
        .body(serde_json::to_string(&res)?)?)
}
