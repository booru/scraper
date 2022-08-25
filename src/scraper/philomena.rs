use std::str::FromStr;

use itertools::Itertools;
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware as Client;

use crate::camo::camo_url;
use crate::scraper::philomena::derpibooru::is_derpibooru;
use crate::scraper::{ScrapeImage, ScrapeResult, ScrapeResultData};
use crate::Configuration;
use anyhow::{Context, Result};
use tracing::{debug, trace};

mod derpibooru;

#[tracing::instrument]
pub async fn is_philomena(url: &Url) -> Result<bool> {
    is_derpibooru(url).await
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct PhilomenaApiResponse {
    image: PhilomenaApiImageResponse,
}

#[derive(serde::Deserialize, serde::Serialize, Clone)]
struct PhilomenaApiImageResponse {
    tags: Vec<String>,
    source_url: Option<String>,
    uploader: Option<String>,
    description: Option<String>,
    view_url: String,
}

#[tracing::instrument(skip(config))]
pub async fn philomena_scrape(config: &Configuration, url: &Url) -> Result<Option<ScrapeResult>> {
    trace!("converting philo url to api url");
    let api_url = if is_derpibooru(url).await? {
        derpibooru::url_to_api(url)?
    } else {
        anyhow::bail!("Tried URL that isn't known philomena")
    };
    let api_url = match api_url {
        None => anyhow::bail!("URL did not match and returned empty"),
        Some(v) => v.to_string(),
    };
    let client = crate::scraper::client(config)?;
    let resp: PhilomenaApiResponse = make_philomena_api_request(&client, &api_url).await?;
    let image = resp.image;
    let image_view = Url::from_str(&image.view_url)?;
    let description = image.description;
    let description = if description.clone().unwrap_or_default().trim().is_empty() {
        None
    } else {
        description
    };
    debug!("source_url: {:?}", image.source_url);
    let source_url = image.source_url.clone();
    let source_url = if source_url.clone().unwrap_or_default().trim().is_empty() {
        None
    } else {
        source_url
    };
    let source_url = source_url
        .map(|x| Url::from_str(&x))
        .transpose()
        .context(format!("source url: {:?}", &image.source_url))?;
    Ok(Some(ScrapeResult::Ok(ScrapeResultData {
        source_url,
        author_name: image
            .tags
            .iter()
            .find(|x| x.starts_with("artist:"))
            .cloned()
            .map(|x| x.strip_prefix("artist:").unwrap().to_string()),
        additional_tags: {
            let add_tags = image
                .tags
                .iter()
                .filter(|x| !x.starts_with("artist:"))
                .cloned()
                .sorted()
                .collect_vec();
            if add_tags.is_empty() {
                None
            } else {
                Some(add_tags)
            }
        },
        description,
        images: vec![ScrapeImage {
            camo_url: camo_url(config, &image_view)?,
            url: image_view,
        }],
    })))
}

#[tracing::instrument(skip(client))]
async fn make_philomena_api_request(
    client: &Client,
    api_url: &str,
) -> Result<PhilomenaApiResponse> {
    debug!("running api request");
    client
        .get(api_url)
        .send()
        .await
        .context("request to philomena failed")?
        .error_for_status()
        .context("philomena returned error code")?
        .json()
        .await
        .context("could not parse philomena")
}

#[cfg(test)]
mod test {
    use crate::scraper::{scrape, ScrapeResultData};
    use crate::State;

    use super::*;
    use test_log::test;

    #[test]
    fn test_derpibooru_scraper() -> Result<()> {
        let urls = vec![
            (
                r#"https://derpibooru.org/images/1426211"#,
                ScrapeResultData {
                    source_url: Some(Url::parse("http://brunomilan13.deviantart.com/art/Starlight-Glimmer-Season-6-by-Zacatron94-678047433").unwrap()),
                    author_name: Some("zacatron94".to_string()),
                    additional_tags: Some(vec![
                        "blue eyes", "female", "horn", "mare", "multicolored mane",
                        "multicolored tail", "pony", "safe", "simple background",
                        "smiling", "solo", "standing", "starlight glimmer", "tail",
                        "transparent background", "unicorn", "vector",
                    ].into_iter().map(str::to_string).sorted().collect_vec()),
                    description: None,
                    images: vec![
                        ScrapeImage {
                            url: Url::parse("https://derpicdn.net/img/view/2017/5/1/1426211").unwrap(),
                            camo_url: Url::parse("https://derpicdn.net/img/view/2017/5/1/1426211").unwrap(),
                        },
                    ],
                },
            ),
            (
                r#"https://derpibooru.org/1426211"#,
                ScrapeResultData {
                    source_url: Some(Url::parse("http://brunomilan13.deviantart.com/art/Starlight-Glimmer-Season-6-by-Zacatron94-678047433").unwrap()),
                    author_name: Some("zacatron94".to_string()),
                    additional_tags: Some(vec![
                        "blue eyes", "female", "horn", "mare", "multicolored mane",
                        "multicolored tail", "pony", "safe", "simple background",
                        "smiling", "solo", "standing", "starlight glimmer", "tail",
                        "transparent background", "unicorn", "vector",
                    ].into_iter().map(str::to_string).sorted().collect_vec()),
                    description: None,
                    images: vec![
                        ScrapeImage {
                            url: Url::parse("https://derpicdn.net/img/view/2017/5/1/1426211").unwrap(),
                            camo_url: Url::parse("https://derpicdn.net/img/view/2017/5/1/1426211").unwrap(),
                        },
                    ],
                },
            ),
            (
                r#"https://derpibooru.org/images/1"#,
                ScrapeResultData {
                    source_url: Some(Url::parse("https://www.deviantart.com/speccysy/art/Afternoon-Flight-215193985").unwrap()),
                    author_name: Some("speccysy".to_string()),
                    additional_tags: Some(vec!["2011", "artifact", "cloud", "cloudy", "crepuscular rays", "cute", "derpibooru legacy", "eyes closed", "female", "first fluttershy picture on derpibooru", "fluttershy", "flying", "g4", "happy", "index get", "long hair", "mammal", "mare", "messy mane", "milestone", "one of the first", "outdoors", "pegasus", "pony", "safe", "shyabetes", "signature", "sky", "smiling", "solo", "spread wings", "stretching", "sunlight", "sunshine", "sweet dreams fuel", "upside down", "weapons-grade cute", "wings"]
                    .into_iter().map(str::to_string).sorted().collect_vec()),
                    description: None,
                    images: vec![
                        ScrapeImage {
                            url: Url::parse("https://derpicdn.net/img/view/2012/1/2/1").unwrap(),
                            camo_url: Url::parse("https://derpicdn.net/img/view/2012/1/2/1").unwrap(),
                        },
                    ],
                },
            ),
            (
                r#"https://derpibooru.org/1"#,
                ScrapeResultData {
                    source_url: Some(Url::parse("https://www.deviantart.com/speccysy/art/Afternoon-Flight-215193985").unwrap()),
                    author_name: Some("speccysy".to_string()),
                    additional_tags: Some(vec!["2011", "artifact", "cloud", "cloudy", "crepuscular rays", "cute", "derpibooru legacy", "eyes closed", "female", "first fluttershy picture on derpibooru", "fluttershy", "flying", "g4", "happy", "index get", "long hair", "mammal", "mare", "messy mane", "milestone", "one of the first", "outdoors", "pegasus", "pony", "safe", "shyabetes", "signature", "sky", "smiling", "solo", "spread wings", "stretching", "sunlight", "sunshine", "sweet dreams fuel", "upside down", "weapons-grade cute", "wings"].into_iter().map(str::to_string).sorted().collect_vec()),
                    description: None,
                    images: vec![
                        ScrapeImage {
                            url: Url::parse("https://derpicdn.net/img/view/2012/1/2/1").unwrap(),
                            camo_url: Url::parse("https://derpicdn.net/img/view/2012/1/2/1").unwrap(),
                        },
                    ],
                },
            ),
            (
                r#"https://derpibooru.org/images/17368"#,
                ScrapeResultData {
                    source_url: None,
                    author_name: None,
                    additional_tags: Some(vec![
                        "bathtub", "female", "irl", "mare", "pegasus", "photo", "ponies in real life", "pony", "rainbow dash",
                        "safe", "shower", "solo", "surprised", "toilet", "vector"
                    ].into_iter().map(str::to_string).sorted().collect_vec()),
                    description: Some("Dash, how'd you get in my(hit by shampoo bottle)".to_string()),
                    images: vec![
                        ScrapeImage {
                            url: Url::parse("https://derpicdn.net/img/view/2012/6/23/17368").unwrap(),
                            camo_url: Url::parse("https://derpicdn.net/img/view/2012/6/23/17368").unwrap(),
                        },
                    ],
                },
            )
        ];
        let config = Configuration::default();
        let state = State::new(config.clone())?;
        for (url, expected_result) in urls {
            let scrape = tokio_test::block_on(scrape(&config, &state, url));
            let scrape = match scrape {
                Ok(s) => s,
                Err(e) => return Err(e),
            };
            let mut scrape = match scrape {
                Some(s) => s,
                None => anyhow::bail!("got none response from scraper"),
            };
            match &mut scrape {
                ScrapeResult::Ok(ref mut scrape) => {
                    scrape.images.iter_mut().for_each(|x: &mut ScrapeImage| {
                        x.url
                            .set_path(x.url.path().to_string().split_once("__").unwrap().0);
                        x.camo_url
                            .set_path(x.camo_url.path().to_string().split_once("__").unwrap().0);
                    })
                }
                _ => panic!(),
            }
            let expected_result = ScrapeResult::Ok(expected_result);
            assert_eq!(expected_result, scrape, "Failed on URL {url:?}");
        }
        Ok(())
    }
}
