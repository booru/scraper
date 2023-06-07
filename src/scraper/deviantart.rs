use crate::scraper::client;
use crate::scraper::ScrapeResultData;
use crate::{
    scraper::{ScrapeImage, ScrapeResult},
    Configuration,
};
use anyhow::Context;
use anyhow::Result;
use regex::{Captures, Regex};
use std::str::FromStr;
use tracing::trace;
use url::Url;

lazy_static::lazy_static! {
    static ref IMAGE_REGEX: Regex = Regex::from_str(r#"data-rh="true" rel="preload" href="([^"]*)" as="image""#).expect("failure in setting up essential regex");
    static ref SOURCE_REGEX: Regex = Regex::from_str(r#"rel="canonical" href="([^"]*)""#).expect("failure in setting up essential regex");
    static ref ARTIST_REGEX: Regex = Regex::from_str(r#"https://www.deviantart.com/([^/]*)/art"#).expect("failure in setting up essential regex");
    static ref SERIAL_REGEX: Regex = Regex::from_str(r#"https://www.deviantart.com/(?:.*?)-(\d+)\z"#).expect("failure in setting up essential regex");
    static ref CDNINT_REGEX: Regex = Regex::from_str(r#"(https://images-wixmp-[0-9a-f]+.wixmp.com)(?:/intermediary)?/f/([^/]*)/([^/?]*)"#).expect("failure in setting up essential regex");
    static ref PNG_REGEX: Regex = Regex::from_str(r#"(https://[0-9a-z\-\.]+(?:/intermediary)?/f/[0-9a-f\-]+/[0-9a-z\-]+\.png/v1/fill/[0-9a-z_,]+/[0-9a-z_\-]+)(\.png)(.*)"#).expect("failure in setting up essential regex");
    static ref JPG_REGEX: Regex = Regex::from_str(r#"(https://[0-9a-z\-\.]+(?:/intermediary)?/f/[0-9a-f\-]+/[0-9a-z\-]+\.jpg/v1/fill/w_[0-9]+,h_[0-9]+,q_)([0-9]+)(,[a-z]+\/[a-z0-6_\-]+\.jpe?g.*)"#).expect("failure in setting up essential regex");
}

#[tracing::instrument]
pub async fn is_deviantart(url: &Url) -> Result<bool> {
    match url.host_str() {
        Some(url) => Ok(url.ends_with(".deviantart.com") || url == "deviantart.com"),
        None => Ok(false),
    }
}

#[tracing::instrument(skip(config))]
pub async fn get_deviantart_page(config: &Configuration, url: &Url) -> Result<String> {
    let client = crate::scraper::client(config)?;
    client
        .get(url.to_owned())
        .send()
        .await
        .context("image request failed")?
        .text()
        .await
        .context("could not read response")
}

#[tracing::instrument(skip(config))]
pub async fn deviantart_scrape(config: &Configuration, url: &Url) -> Result<Option<ScrapeResult>> {
    let body = get_deviantart_page(config, url).await?;
    let extract_data = extract_data(config, &body)
        .await
        .context("could not extract DA page data")?;

    match extract_data {
        None => Ok(None),
        Some((extract_data, camo)) => match extract_data {
            ScrapeResult::Ok(mut v) => {
                let images = try_new_hires(v.images).await?;
                let images = try_intermediary_hires(config, images).await?;
                let source_url = match &v.source_url {
                    Some(v) => v,
                    None => anyhow::bail!("had no source url"),
                };
                let images = try_old_hires(config, source_url, images, &camo)
                    .await
                    .context("old_hires conversion failed")?;

                v.images = images;

                Ok(Some(ScrapeResult::Ok(v.clone())))
            }
            ScrapeResult::None => Ok(None),
            ScrapeResult::Err(v) => Ok(Some(ScrapeResult::Err(v))),
        },
    }
}

#[tracing::instrument(skip(config))]
async fn extract_data(config: &Configuration, body: &str) -> Result<Option<(ScrapeResult, Url)>> {
    let image = &IMAGE_REGEX.captures(body);
    let image = match image {
        None => anyhow::bail!("no image found"),
        Some(image) => &image[1],
    };
    let source = &SOURCE_REGEX.captures(body);
    let source = match source {
        None => anyhow::bail!("no source found"),
        Some(source) => &source[1],
    };
    let artist = &ARTIST_REGEX.captures(source);
    let artist = match artist {
        None => anyhow::bail!("no artist found"),
        Some(artist) => &artist[1],
    };
    trace!("deviant capture: {} {} {}", image, source, artist);

    let camo = crate::camo::camo_url(
        config,
        &Url::parse(image).context("could not parse image URL")?,
    )
    .context("could not camo URL")?;

    trace!("camo_url: {}", camo);

    Ok(Some((
        ScrapeResult::Ok(ScrapeResultData {
            source_url: Some(Url::parse(source).context("source URL not valid URL")?),
            author_name: Some(artist.to_string()),
            additional_tags: None,
            description: None,
            images: vec![ScrapeImage {
                url: Url::parse(image).context("image URL not valid URL")?,
                camo_url: camo.clone(),
            }],
        }),
        camo,
    )))
}

#[tracing::instrument(skip(config))]
async fn try_intermediary_hires(
    config: &Configuration,
    mut images: Vec<ScrapeImage>,
) -> Result<Vec<ScrapeImage>> {
    for image in images.clone() {
        let (domain, object_uuid, object_name) = {
            let caps = CDNINT_REGEX.captures(image.url.as_str());
            let caps = match caps {
                None => continue,
                Some(caps) => caps,
            };
            let domain: &str = &caps[1];
            let object_uuid: &str = &caps[2];
            let object_name: &str = &caps[3];
            (
                domain.to_string(),
                object_uuid.to_string(),
                object_name.to_string(),
            )
        };
        let built_url = format!(
            "{domain}/intermediary/{object_uuid}/{object_name}",
            domain = domain,
            object_uuid = object_uuid,
            object_name = object_name
        );
        let built_url = Url::from_str(&built_url)?;
        let client = client(config)?;
        if client
            .head(built_url.clone())
            .send()
            .await
            .context("HEAD request to DA URL failed")?
            .status()
            == 200
        {
            let built_url = built_url;
            images.push(ScrapeImage {
                url: built_url,
                camo_url: image.camo_url,
            })
        }
    }
    Ok(images)
}

#[tracing::instrument]
async fn try_new_hires(mut images: Vec<ScrapeImage>) -> Result<Vec<ScrapeImage>> {
    for image in images.clone() {
        let old_url = image.url.to_string();
        if PNG_REGEX.is_match(&old_url) {
            let new_url = PNG_REGEX.replace(&old_url, |caps: &Captures| {
                format!("{}.png{}", &caps[1], &caps[3])
            });
            let new_url = Url::from_str(&new_url).context("could not parse png url")?;
            images.push(ScrapeImage {
                url: new_url,
                camo_url: image.camo_url.clone(),
            })
        }
        if JPG_REGEX.is_match(&old_url) {
            let new_url = JPG_REGEX.replace(&old_url, |caps: &Captures| {
                format!("{}100{}", &caps[1], &caps[3])
            });
            let new_url = Url::from_str(&new_url).context("could not parse jpeg url")?;
            images.push(ScrapeImage {
                url: new_url,
                camo_url: image.camo_url.clone(),
            })
        }
    }
    Ok(images)
}

#[tracing::instrument(skip(config, camo))]
async fn try_old_hires(
    config: &Configuration,
    source_url: &Url,
    mut images: Vec<ScrapeImage>,
    camo: &Url,
) -> Result<Vec<ScrapeImage>> {
    let serial = &SERIAL_REGEX.captures(source_url.as_str());
    let serial = match serial {
        None => anyhow::bail!("no serial captured"),
        Some(serial) => &serial[1],
    };
    let base36 = radix_fmt::radix(
        serial
            .parse::<i64>()
            .context("integer could not be parsed")?,
        36,
    )
    .to_string()
    .to_lowercase();

    let built_url = format!(
        "http://orig01.deviantart.net/x_by_x-d{base36}.png",
        base36 = base36
    );

    let client = crate::scraper::client_with_redir_limit(config, reqwest::redirect::Policy::none())
        .context("could not create DA scraping agent")?;
    let resp = client
        .get(built_url)
        .send()
        .await
        .context("old hires request failed")?;
    if let Some((_, loc)) = resp
        .headers()
        .iter()
        .find(|(name, _value)| name.as_str().to_lowercase() == "location")
    {
        let loc = loc.to_str().context("location not valid string")?;
        images.push(ScrapeImage {
            url: Url::parse(loc).context("new old_hires location is not valid URL")?,
            camo_url: camo.clone(),
        });
        return Ok(images);
    }
    Ok(images)
}

#[cfg(test)]
mod test {

    use crate::scraper::scrape;
    use crate::State;

    use super::*;
    use test_log::test;

    #[cfg(feature = "net-tests")]
    #[test]
    fn test_deviantart_scraper() -> Result<()> {
        let url = r#"https://www.deviantart.com/the-park/art/Comm-Baseball-cap-derpy-833396912"#;
        let config = Configuration::default();
        let state = State::new(config.clone())?;
        let scrape = tokio_test::block_on(scrape(&config, &state, url));
        let scrape = match scrape {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let mut scrape = match scrape {
            Some(s) => s,
            None => anyhow::bail!("got none response from scraper"),
        };
        {
            // remove token from URL
            if let ScrapeResult::Ok(result) = &mut scrape {
                for image in result.images.iter_mut() {
                    let fixup = &mut image.url;
                    fixup.query_pairs_mut().clear();
                    let fixup = &mut image.camo_url;
                    fixup.query_pairs_mut().clear();
                }
            }
        }
        let expected_result = ScrapeResult::Ok(ScrapeResultData{
            source_url: Some(Url::parse("https://www.deviantart.com/the-park/art/Comm-Baseball-cap-derpy-833396912").unwrap()),
            author_name: Some("the-park".to_string()),
            additional_tags: None,
            description: None,
            images: vec![
                ScrapeImage{
                    url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/39da62f1-b049-4f7a-b10b-4cc5167cb9a2/dds6l68-3084d503-abbf-4f6d-bd82-7a36298e0106.png?").unwrap(),
                    camo_url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/39da62f1-b049-4f7a-b10b-4cc5167cb9a2/dds6l68-3084d503-abbf-4f6d-bd82-7a36298e0106.png?").unwrap(),
                }
            ],
        });
        assert_eq!(expected_result, scrape);
        Ok(())
    }

    #[cfg(feature = "net-tests")]
    #[test]
    fn test_deviantart_scraper_failed_scrape_220825() -> Result<()> {
        let url = r#"https://www.deviantart.com/joellethenose/art/Luna-378433727"#;
        let config = Configuration::default();
        let state = State::new(config.clone())?;
        let scrape = tokio_test::block_on(scrape(&config, &state, url));
        let scrape = match scrape {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let mut scrape = match scrape {
            Some(s) => s,
            None => anyhow::bail!("got none response from scraper"),
        };
        {
            // remove token from URL
            if let ScrapeResult::Ok(result) = &mut scrape {
                for image in result.images.iter_mut() {
                    let fixup = &mut image.url;
                    fixup.query_pairs_mut().clear();
                    let fixup = &mut image.camo_url;
                    fixup.query_pairs_mut().clear();
                }
            }
        }
        let expected_result = ScrapeResult::Ok(ScrapeResultData{
            source_url: Some(Url::parse("https://www.deviantart.com/joellethenose/art/Luna-378433727").unwrap()),
            author_name: Some("joellethenose".to_string()),
            additional_tags: None,
            description: None,
            images: vec![
                ScrapeImage{
                    url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/86a8f3ea-88f8-434f-b821-a0d48ce59131/d69b5bz-9498b591-38b2-4b7b-8a48-92cee122f131.jpg/v1/fill/w_1280,h_931,q_75,strp/luna_by_joellethenose_d69b5bz-fullview.jpg?").unwrap(),
                    camo_url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/86a8f3ea-88f8-434f-b821-a0d48ce59131/d69b5bz-9498b591-38b2-4b7b-8a48-92cee122f131.jpg/v1/fill/w_1280,h_931,q_75,strp/luna_by_joellethenose_d69b5bz-fullview.jpg?").unwrap(),
                }
            ],
        });
        assert_eq!(expected_result, scrape);
        Ok(())
    }

    #[cfg(feature = "net-tests")]
    #[test]
    fn test_deviantart_scraper_failed_scrape_230607() -> Result<()> {
        let url = r#"https://www.deviantart.com/aztrial/art/MLP-G5-Ruby-Jubilee-962914035"#;
        let config = Configuration::default();
        let state = State::new(config.clone())?;
        let scrape = tokio_test::block_on(scrape(&config, &state, url));
        let scrape = match scrape {
            Ok(s) => s,
            Err(e) => return Err(e),
        };
        let mut scrape = match scrape {
            Some(s) => s,
            None => anyhow::bail!("got none response from scraper"),
        };
        {
            // remove token from URL
            if let ScrapeResult::Ok(result) = &mut scrape {
                for image in result.images.iter_mut() {
                    let fixup = &mut image.url;
                    fixup.query_pairs_mut().clear();
                    let fixup = &mut image.camo_url;
                    fixup.query_pairs_mut().clear();
                }
            }
        }
        let expected_result = ScrapeResult::Ok(ScrapeResultData{
            source_url: Some(Url::parse("https://www.deviantart.com/aztrial/art/MLP-G5-Ruby-Jubilee-962914035").unwrap()),
            author_name: Some("aztrial".to_string()),
            additional_tags: None,
            description: None,
            images: vec![
                ScrapeImage{
                    url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/2f871ec7-c49f-4d50-a83d-4a775e89b234/dfxal83-0996fb96-92cb-458d-a83c-2a2a03a8d7c4.png/v1/fill/w_1280,h_1586,q_80,strp/mlp_g5__ruby_jubilee___by_aztrial_dfxal83-fullview.jpg?").unwrap(),
                    camo_url: Url::parse("https://images-wixmp-ed30a86b8c4ca887773594c2.wixmp.com/f/2f871ec7-c49f-4d50-a83d-4a775e89b234/dfxal83-0996fb96-92cb-458d-a83c-2a2a03a8d7c4.png/v1/fill/w_1280,h_1586,q_80,strp/mlp_g5__ruby_jubilee___by_aztrial_dfxal83-fullview.jpg?").unwrap(),
                }
            ],
        });
        assert_eq!(expected_result, scrape);
        Ok(())
    }
}
