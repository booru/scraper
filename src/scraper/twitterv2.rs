use std::str::FromStr;

use crate::scraper::ScrapeResult;
use crate::scraper::ScrapeResultData;
use crate::{scraper::ScrapeImage, Configuration};
use anyhow::Result;
use itertools::Itertools;
use twitter_v2::authorization::BearerToken;
use twitter_v2::id::NumericId;
use twitter_v2::query::{MediaField, TweetExpansion, TweetField, UserField};
use twitter_v2::TwitterApi;
use url::Url;

use super::twitter::URL_REGEX;

#[tracing::instrument(skip(config))]
pub async fn twitter_v2_scrape(config: &Configuration, url: &Url) -> Result<Option<ScrapeResult>> {
    let auth = BearerToken::new(
        config
            .twitter_api_key_bearer
            .as_ref()
            .expect("must have configured v2 api key"),
    );
    let (_user, status_id) = {
        let caps = URL_REGEX.captures(url.as_str());
        let caps = match caps {
            Some(caps) => caps,
            None => anyhow::bail!("could not parse tweet url"),
        };
        (&caps[1].to_string(), &caps[2].to_string())
    };
    let tweet = TwitterApi::new(auth.clone())
        .get_tweet(NumericId::from_str(status_id)?)
        .tweet_fields([
            TweetField::Text,
            TweetField::Id,
            TweetField::CreatedAt,
            TweetField::AuthorId,
            TweetField::Attachments,
        ])
        .expansions([TweetExpansion::AttachmentsMediaKeys])
        .media_fields([
            MediaField::Url,
            MediaField::PreviewImageUrl,
            MediaField::MediaKey,
        ])
        .send()
        .await?;
    let media = match tweet.includes.as_ref() {
        None => return Ok(None),
        Some(includes) => includes,
    };
    let tweet = match tweet.data.as_ref() {
        None => return Ok(None),
        Some(data) => data,
    };
    let tweet_author = match tweet.author_id {
        None => return Ok(None),
        Some(author) => author,
    };
    let user = TwitterApi::new(auth.clone())
        .get_user(tweet_author)
        .user_fields([UserField::Name, UserField::Url])
        .send()
        .await?
        .into_data();

    let user = match user {
        None => return Ok(None),
        Some(user) => user,
    };

    let images = match &media.media {
        None => vec![],
        Some(media) => media
            .iter()
            .filter_map(|image| {
                let url = match &image.url {
                    None => return None,
                    Some(v) => v.clone(),
                };
                let prev = image
                    .preview_image_url
                    .clone()
                    .unwrap_or_else(|| url.clone());
                let camo_url =
                    crate::camo::camo_url(config, &prev).expect("invalid tweet media uri");
                Some(ScrapeImage { url, camo_url })
            })
            .collect_vec(),
    };

    if images.is_empty() {
        return Ok(None);
    }

    Ok(Some(ScrapeResult::Ok(ScrapeResultData {
        source_url: Some(url.clone()),
        author_name: Some(user.username),
        additional_tags: None,
        description: Some(tweet.text.clone()),
        images,
    })))
}
