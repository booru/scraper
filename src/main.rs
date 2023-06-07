use std::sync::Arc;

use anyhow::Result;
use axum::{routing::get, Extension};
use envconfig::Envconfig;
use tracing::{info, trace, Level};

mod camo;
mod scraper;
mod web;

#[derive(Envconfig, Clone, securefmt::Debug)]
pub struct Configuration {
    #[envconfig(from = "LISTEN_ON", default = "127.0.0.1:8080")]
    #[sensitive]
    bind_to: std::net::SocketAddr,
    #[envconfig(from = "ALLOWED_ORIGINS", default = "localhost,localhost:8080")]
    #[sensitive]
    allowed_origins: String,
    #[envconfig(from = "CHECK_CSRF_PRESENCE", default = "false")]
    check_csrf_presence: bool,
    #[envconfig(from = "TUMBLR_API_KEY")]
    #[sensitive]
    tumblr_api_key: Option<String>,
    #[envconfig(from = "HTTP_PROXY")]
    #[sensitive]
    proxy_url: Option<String>,
    #[envconfig(from = "CAMO_KEY")]
    #[sensitive]
    camo_key: Option<String>,
    #[envconfig(from = "CAMO_HOST")]
    camo_host: Option<String>,
    #[envconfig(from = "ENABLE_GET_REQUEST", default = "false")]
    enable_get_request: bool,
    #[envconfig(from = "PREFERRED_NITTER_INSTANCE_HOST")]
    preferred_nitter_instance_host: Option<String>,
    #[envconfig(from = "LOG_LEVEL", default = "INFO")]
    log_level: Level,
    #[envconfig(from = "ALLOW_EMPTY_ORIGIN", default = "false")]
    allow_empty_origin: bool,
    #[envconfig(from = "SENTRY_URL")]
    sentry_url: Option<url::Url>,
    #[envconfig(from = "TWITTER_USE_V2", default = "false")]
    twitter_use_v2: bool,
    #[envconfig(from = "TWITTER_API_KEY")]
    twitter_api_key: Option<String>,
    #[envconfig(from = "TWITTER_API_KEY_SECRET")]
    twitter_api_key_secret: Option<String>,
    #[envconfig(from = "TWITTER_API_BEARER")]
    twitter_api_key_bearer: Option<String>,
}

#[derive(Clone)]
pub struct State {
    config: Configuration,
    parsed_allowed_origins: Vec<String>,
    result_cache: ResultCache,
    tumblr_dns_cache: TumblrDnsCache,
}

pub type ResultCache = moka::future::Cache<String, Option<scraper::ScrapeResult>>;
pub type TumblrDnsCache = moka::future::Cache<String, bool>;

impl State {
    fn new(config: Configuration) -> Result<Self> {
        Ok(Self {
            parsed_allowed_origins: config
                .allowed_origins
                .split(',')
                .filter(|x| !x.is_empty())
                .map(|x| x.to_string())
                .collect(),
            config,
            result_cache: moka::future::CacheBuilder::new(1000)
                .initial_capacity(1000)
                .support_invalidation_closures()
                .time_to_idle(std::time::Duration::from_secs(10 * 60))
                .time_to_live(std::time::Duration::from_secs(100 * 60))
                .build(),
            tumblr_dns_cache: moka::future::CacheBuilder::new(1000)
                .initial_capacity(1000)
                .support_invalidation_closures()
                .time_to_idle(std::time::Duration::from_secs(10 * 60))
                .time_to_live(std::time::Duration::from_secs(100 * 60))
                .build(),
        })
    }
    pub fn is_allowed_origin(&self, origin: Option<&str>) -> bool {
        match origin {
            Some(origin) => {
                let mut allowed = false;
                for host in &self.parsed_allowed_origins {
                    if host == origin {
                        allowed = true;
                    }
                }
                allowed || self.parsed_allowed_origins.is_empty()
            }
            None => self.config.allow_empty_origin,
        }
    }
}

impl Default for Configuration {
    fn default() -> Self {
        let s = Self {
            bind_to: std::net::ToSocketAddrs::to_socket_addrs("localhost:8080")
                .unwrap()
                .next()
                .unwrap(),
            allowed_origins: "".to_string(),
            check_csrf_presence: false,
            tumblr_api_key: std::env::var("TUMBLR_API_KEY").ok(),
            proxy_url: None,
            camo_host: None,
            camo_key: None,
            enable_get_request: false,
            preferred_nitter_instance_host: None,
            log_level: Level::INFO,
            allow_empty_origin: false,
            sentry_url: None,
            twitter_use_v2: false,
            twitter_api_key: None,
            twitter_api_key_bearer: None,
            twitter_api_key_secret: None,
        };
        trace!("created config: {:?}", s);
        s
    }
}

fn main() -> Result<()> {
    better_panic::install();
    if let Err(e) = kankyo::load(false) {
        info!("couldn't load .env file: {}, this is probably fine", e);
    }
    use tokio::runtime::Builder;
    let runtime = Builder::new_multi_thread()
        .worker_threads(16)
        .max_blocking_threads(64)
        .on_thread_stop(|| {
            tracing::trace!("thread stopping");
        })
        .on_thread_start(|| {
            tracing::trace!("thread started");
        })
        .thread_name_fn(|| {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
            let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
            format!("philomena-scraper-{}", id)
        })
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async move { tokio::spawn(async move { main_start().await }).await? })?;
    runtime.shutdown_timeout(std::time::Duration::from_secs(10));
    Ok(())
}

async fn main_start() -> Result<()> {
    let config = Configuration::init_from_env();
    let config = match config {
        Err(e) => {
            tracing::error!("could not load config: {}", e);
            Configuration::default()
        }
        Ok(v) => v,
    };
    use tracing_subscriber::prelude::*;
    let fmt_layer = tracing_subscriber::fmt::layer().with_filter(
        tracing_subscriber::filter::LevelFilter::from_level(config.log_level),
    );
    tracing_subscriber::Registry::default()
        .with(sentry::integrations::tracing::layer())
        .with(fmt_layer)
        .init();
    tracing::info!("log level is now {}", config.log_level);
    let _sentry = config.sentry_url.as_ref().map(|url| {
        tracing::info!("Enabling Sentry tracing for {}", env!("CARGO_BIN_NAME"));
        let opts = sentry::ClientOptions {
            release: Some(std::borrow::Cow::Borrowed(env!("CARGO_BIN_NAME"))),
            traces_sample_rate: 1.0,
            send_default_pii: false,
            in_app_include: vec!["scraper"],
            before_send: Some(Arc::new(|mut event: sentry::types::protocol::v7::Event| {
                // Modify event here
                event.request = event.request.map(|mut f| {
                    f.cookies = None;
                    // TODO: keep some important headers
                    f.headers.clear();
                    f
                });
                event.server_name = None; // Don't send server name
                Some(event)
            })),
            ..Default::default()
        };
        sentry::init((url.to_string(), opts))
    });
    let state = Arc::new(State::new(config.clone())?);
    let app = axum::Router::new()
        .route("/images/scrape", get(web::scrape).post(web::scrape_post))
        .layer(Extension(state.clone()))
        .layer(axum::middleware::from_fn(move |a, b| {
            let state = state.clone();
            web::origin_check(a, state, b)
        }))
        .layer(axum::middleware::from_fn(web::latency));
    let app = match config.sentry_url {
        None => app,
        Some(ref _v) => app
            .layer(sentry_tower::NewSentryLayer::new_from_top())
            .layer(sentry_tower::SentryHttpLayer::with_transaction()),
    };
    axum::Server::bind(&config.bind_to)
        .serve(app.into_make_service())
        .await
        .unwrap();
    // close sentry
    if let Some(s) = _sentry.as_ref() {
        s.flush(Some(std::time::Duration::from_millis(5000)));
    }
    drop(_sentry);
    Ok(())
}
