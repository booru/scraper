use std::{ops::Index, str::FromStr};

use crate::scraper::ScrapeResult;
use crate::scraper::ScrapeResultData;
use crate::{scraper::ScrapeImage, Configuration};
use anyhow::{Context, Result};
use itertools::Itertools;
use regex::Regex;
use serde_json::Value;
use tracing::{debug, trace};
use twitter_v2::TwitterApi;
use twitter_v2::authorization::BearerToken;
use twitter_v2::id::NumericId;
use twitter_v2::query::{TweetField, UserField, MediaField, TweetExpansion};
use url::Url;

use super::twitter::URL_REGEX;

#[tracing::instrument(skip(config))]
pub async fn twitter_v2_scrape(config: &Configuration, url: &Url) -> Result<Option<ScrapeResult>> {
    let auth = BearerToken::new(&config.twitter_api_key_bearer.as_ref().expect("must have configured v2 api key"));
    let (user, status_id) = {
        let caps = URL_REGEX.captures(url.as_str());
        let caps = match caps {
            Some(caps) => caps,
            None => anyhow::bail!("could not parse tweet url"),
        };
        (&caps[1].to_string(), &caps[2].to_string())
    };
    let tweet = TwitterApi::new(auth.clone()).get_tweet(NumericId::from_str(status_id)?)
        .tweet_fields([TweetField::Text,
                 TweetField::Id, TweetField::CreatedAt,
                TweetField::AuthorId, TweetField::Attachments])
        .expansions([TweetExpansion::AttachmentsMediaKeys])
        .media_fields([MediaField::Url, MediaField::PreviewImageUrl, MediaField::MediaKey])
                .send().await?;
    let media = tweet.includes.clone().unwrap();
    let tweet = tweet.data.clone().unwrap();
    let user = TwitterApi::new(auth.clone()).get_user(tweet.author_id.unwrap())
            .user_fields([UserField::Name, UserField::Url]).send().await?.into_data().unwrap();

    let images = match media.media {
        None => vec![],
        Some(media) => {
            media.iter().map(|image| {
                let url = match &image.url {
                    None => return None,
                    Some(v) => v.clone(),
                };
                let prev = image.preview_image_url.clone().unwrap_or(url.clone());
                let camo_url = crate::camo::camo_url(config, &prev).expect("invalid tweet media uri");
                Some(ScrapeImage {
                    url: super::from_url(url),
                    camo_url: super::from_url(camo_url),
                })
            }).flatten().collect_vec()
        },
    };

    if images.is_empty() {
        return Ok(None)
    }

    Ok(Some(ScrapeResult::Ok(ScrapeResultData{
        source_url:Some(super::from_url(url.clone())), 
        author_name: Some(user.username),
        additional_tags: None,
        description: Some(tweet.text),
        images,
    })))
}