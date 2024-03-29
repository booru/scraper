use crate::scraper::{ScrapeResult, ScrapeResultData};
use crate::{scraper::ScrapeImage, Configuration};
use anyhow::Result;
use url::Url;

lazy_static::lazy_static! {
    static ref MIME_TYPES: Vec<String> = Vec::from([
        "image/gif",
        "image/jpeg",
        "image/png",
        "image/svg",
        "image/svg+xml",
        "video/webm",
    ]).iter().map(|x| x.to_string()).collect();
}

#[tracing::instrument(skip(config))]
pub async fn is_raw(url: &Url, config: &Configuration) -> Result<bool> {
    let client = crate::scraper::client(config)?;
    let res = client.head(url.clone()).send().await?;
    if res.status() == 200 {
        let content_type = res.headers()["content-type"].to_str()?;
        Ok(MIME_TYPES.contains(&content_type.to_string()))
    } else {
        Ok(false)
    }
}

#[tracing::instrument(skip(config))]
pub async fn raw_scrape(config: &Configuration, url: &Url) -> Result<Option<ScrapeResult>> {
    Ok(Some(ScrapeResult::Ok(ScrapeResultData {
        source_url: Some(url.clone()),
        author_name: None,
        additional_tags: None,
        description: None,
        images: Vec::from([ScrapeImage {
            url: url.clone(),
            camo_url: crate::camo::camo_url(config, url)?,
        }]),
    })))
}

#[cfg(test)]
mod test {
    use crate::scraper::scrape;
    use crate::State;

    use super::*;
    use std::str::FromStr;
    use test_log::test;

    #[cfg(feature = "net-tests")]
    #[test]
    fn test_raw_scraper() -> Result<()> {
        let url = r#"https://static.manebooru.art/img/view/2021/3/20/4010154.png"#;
        let config = Configuration::default();
        let state = State::new(config.clone())?;
        let scrape = tokio_test::block_on(scrape(&config, &state, url));
        let scrape = match scrape {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let scrape = match scrape {
            Some(s) => s,
            None => anyhow::bail!("got none response from scraper"),
        };
        let expected_result = ScrapeResult::Ok(ScrapeResultData {
            source_url: Some(url::Url::from_str(url)?),
            author_name: None,
            additional_tags: None,
            description: None,
            images: Vec::from([ScrapeImage {
                url: url::Url::from_str(url)?,
                camo_url: url::Url::from_str(url)?,
            }]),
        });
        assert_eq!(expected_result, scrape);
        Ok(())
    }
}
