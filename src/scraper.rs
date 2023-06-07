mod buzzly;
mod deviantart;
mod nitter;
mod philomena;
mod raw;
mod tumblr;
mod twitter;
mod twitterv2;

use std::{collections::BTreeMap, sync::Arc};

use anyhow::{Context, Result};
use itertools::Itertools;
use sentry::integrations::anyhow::capture_anyhow;
use serde::{Deserialize, Serialize};
use tracing::debug;
use url::Url;

use crate::{Configuration, State};

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(untagged)]
pub enum ScrapeResult {
    Err(ScrapeResultError),
    Ok(ScrapeResultData),
    None,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ScrapeResultError {
    errors: Vec<String>,
}

impl From<String> for ScrapeResultError {
    fn from(f: String) -> Self {
        Self { errors: vec![f] }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
pub struct ScrapeResultData {
    source_url: Option<Url>,
    author_name: Option<String>,
    additional_tags: Option<Vec<String>>,
    description: Option<String>,
    images: Vec<ScrapeImage>,
}

impl Default for ScrapeResult {
    fn default() -> Self {
        Self::None
    }
}

impl ScrapeResult {
    pub fn from_err(e: Arc<anyhow::Error>) -> ScrapeResult {
        ScrapeResult::Err(ScrapeResultError {
            errors: {
                let mut errors = Vec::new();
                debug!("request error: {}", e);
                for e in e.chain() {
                    if !e.is::<reqwest::Error>() {
                        debug!("request error chain {}: {}", errors.len(), e);
                        errors.push(e)
                    }
                }
                errors.iter().map(|e| format!("{}", e)).collect()
            },
        })
    }
}

#[derive(Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ScrapeImage {
    url: Url,
    camo_url: Url,
}

impl std::fmt::Debug for ScrapeImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapeImage")
            .field("url", &self.url.to_string())
            .field("camo_url", &self.camo_url.to_string())
            .finish()
    }
}

#[tracing::instrument(skip(config))]
pub fn client(config: &Configuration) -> Result<reqwest_middleware::ClientWithMiddleware> {
    client_with_redir_limit(config, reqwest::redirect::Policy::none())
}

#[tracing::instrument(skip(config))]
pub fn client_with_redir_limit(
    config: &Configuration,
    redir_policy: reqwest::redirect::Policy,
) -> Result<reqwest_middleware::ClientWithMiddleware> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(5000))
        .connect_timeout(std::time::Duration::from_millis(2500))
        .user_agent("curl/7.83.1")
        .cookie_store(true)
        .redirect(redir_policy);
    let client = match config.proxy_url.clone() {
        None => client,
        Some(proxy_url) => {
            use reqwest::Proxy;
            use std::str::FromStr;
            let proxy_url = url::Url::from_str(&proxy_url)?;
            let proxy = match proxy_url.scheme() {
                "http" => Proxy::all(proxy_url)?,
                "https" => Proxy::all(proxy_url)?,
                "socks" => Proxy::all(proxy_url)?,
                "socks5" => Proxy::all(proxy_url)?,
                _ => anyhow::bail!(
                    "unknown client proxy protocol, specify http, https, socks or socks5"
                ),
            };
            client.proxy(proxy)
        }
    };
    Ok(reqwest_middleware::ClientBuilder::new(client.build()?)
        .with(reqwest_tracing::TracingMiddleware::default())
        .build())
    //Ok(client.build()?)
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Scraper {
    Twitter,
    Nitter,
    Tumblr,
    DeviantArt,
    Philomena,
    Buzzly,
    Raw,
}

impl ToString for Scraper {
    fn to_string(&self) -> String {
        match self {
            Scraper::Twitter => "twitter",
            Scraper::Nitter => "nitter",
            Scraper::Tumblr => "tumblr",
            Scraper::DeviantArt => "deviantart",
            Scraper::Philomena => "philomena",
            Scraper::Buzzly => "buzzly",
            Scraper::Raw => "raw",
        }
        .to_string()
    }
}

impl Scraper {
    #[tracing::instrument(skip(config, state))]
    async fn get_scraper(
        config: &Configuration,
        state: &State,
        url: &url::Url,
    ) -> Result<Option<Self>> {
        use futures::future::FutureExt;
        let (r0, r1, r2, r3, r4, r5) =
            tokio::try_join!(
                twitter::is_twitter(url).map(|matf| matf.map(|mat| if mat {
                    Some(Self::Twitter)
                } else {
                    None
                })),
                nitter::is_nitter(url).map(|matf| matf.map(|mat| if mat {
                    Some(Self::Nitter)
                } else {
                    None
                })),
                tumblr::is_tumblr(state.tumblr_dns_cache.clone(), url)
                    .map(|matf| matf.map(|mat| if mat { Some(Self::Tumblr) } else { None })),
                deviantart::is_deviantart(url).map(|matf| matf.map(|mat| {
                    if mat {
                        Some(Self::DeviantArt)
                    } else {
                        None
                    }
                })),
                philomena::is_philomena(url).map(|matf| matf.map(|mat| {
                    if mat {
                        Some(Self::Philomena)
                    } else {
                        None
                    }
                })),
                buzzly::is_buzzlyart(url).map(|matf| matf.map(|mat| if mat {
                    Some(Self::Buzzly)
                } else {
                    None
                })),
            )?;
        let res = vec![r0, r1, r2, r3, r4, r5];
        let res: Vec<Scraper> = res.into_iter().flatten().collect_vec();
        Ok(if res.is_empty() {
            // raw is a slow check due to network request, do it last
            if raw::is_raw(url, config).await? {
                Some(Self::Raw)
            } else {
                None
            }
        } else if res.len() == 1 {
            Some(res[0])
        } else if res.len() > 1 {
            let mut res = res;
            res.sort();
            Some(res[0])
        } else {
            unreachable!("res must be empty but is {:?}", res);
        })
    }

    #[tracing::instrument(skip(config), fields(self))]
    async fn execute_scrape(
        self,
        config: &Configuration,
        url: &url::Url,
    ) -> Result<Option<ScrapeResult>> {
        sentry::configure_scope(|scope| {
            let mut map = BTreeMap::new();
            map.insert("url".to_string(), url.to_string().into());
            map.insert("scraper".to_string(), self.to_string().into());
            scope.set_context("scraper", sentry::protocol::Context::Other(map));
        });
        match self {
            Scraper::Twitter => Ok(twitter::twitter_scrape(config, url)
                .await
                .context("Twitter parser failed")?),
            Scraper::Nitter => Ok(nitter::nitter_scrape(config, url)
                .await
                .context("Nitter parser failed")?),
            Scraper::Tumblr => Ok(tumblr::tumblr_scrape(config, url)
                .await
                .context("Tumblr parser failed")?),
            Scraper::DeviantArt => Ok(deviantart::deviantart_scrape(config, url)
                .await
                .context("DeviantArt parser failed")?),
            Scraper::Philomena => Ok(philomena::philomena_scrape(config, url)
                .await
                .context("Philomena parser failed")?),
            Scraper::Buzzly => Ok(buzzly::buzzlyart_scrape(config, url)
                .await
                .context("Buzzly parser failed")?),
            Scraper::Raw => Ok(raw::raw_scrape(config, url)
                .await
                .context("Raw parser failed")?),
        }
    }
}

#[tracing::instrument(skip(config, state))]
pub async fn scrape(
    config: &Configuration,
    state: &State,
    url: &str,
) -> Result<Option<ScrapeResult>> {
    use std::str::FromStr;
    let url = url::Url::from_str(url).context("could not parse URL for scraper")?;
    let check = Scraper::get_scraper(config, state, &url).await;
    let check = check.map_err(|e| {
        capture_anyhow(&e);
        e
    })?;
    match check {
        Some(scraper) => scraper.execute_scrape(config, &url).await.map_err(|e| {
            capture_anyhow(&e);
            e
        }),
        None => Ok(None),
    }
}
