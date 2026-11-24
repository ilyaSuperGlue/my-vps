use anyhow::Context;
use reqwest::Error;
use scraper::{Html, Selector};
use std::{collections::HashSet, sync::OnceLock, time::Duration};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JobPosting {
    pub title: String,
    pub company: String,
    pub location: String,
    pub url: String,
    pub posted_time: String,
    pub description: Option<String>,
}

const LINKEDIN_GUEST_JOBS_URL: &str =
    "https://www.linkedin.com/jobs-guest/jobs/api/seeMoreJobPostings/search";
const JOBS_PER_PAGE: usize = 10;
const MAX_PAGES: usize = 4;

fn http_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(reqwest::Client::new)
}

pub async fn fetch_linkedin_jobs_in_location_page(
    search_query: &str,
    max_results: usize,
    location: Option<&str>,
    hours_old: Option<u32>,
    page: u32,
) -> anyhow::Result<Vec<JobPosting>> {
    let posted_within = hours_old.map(|hours| format!("r{}", u64::from(hours) * 3600));
    let client = http_client();
    let mut jobs = Vec::new();
    let mut seen = HashSet::new();
    let mut start = page.saturating_sub(1) as usize * JOBS_PER_PAGE;
    let pages_needed = max_results.div_ceil(JOBS_PER_PAGE).min(MAX_PAGES);

    for _ in 0..pages_needed {
        if jobs.len() >= max_results {
            break;
        }
        if start > 0 {
            tokio::time::sleep(Duration::from_secs(3)).await;
        }
        let offset = start.to_string();
        let mut query = vec![("keywords", search_query), ("start", offset.as_str())];
        if let Some(location) = location {
            query.push(("location", location));
        }
        if let Some(window) = posted_within.as_deref() {
            query.extend([("f_TPR", window), ("sortBy", "DD")]);
        }
        let response = client
            .get(LINKEDIN_GUEST_JOBS_URL)
            .query(&query)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(Error::without_url)?;
        let status = response.status();
        tracing::debug!(start, %status, "Received LinkedIn guest jobs page");
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && !jobs.is_empty() {
            tracing::warn!(
                start,
                "LinkedIn rate-limited pagination; returning partial results"
            );
            break;
        }
        check_linkedin_status(status)?;
        let base_url = response.url().clone();
        let body = response.text().await.map_err(Error::without_url)?;
        let (page, card_count) = parse_jobs(&body, &base_url, usize::MAX);
        let previous_count = jobs.len();
        jobs.extend(
            page.into_iter()
                .filter(|job| seen.insert(job_key(&job.url).to_owned()))
                .take(max_results - previous_count),
        );
        if card_count < JOBS_PER_PAGE || jobs.len() == previous_count {
            break;
        }
        start += card_count;
    }
    Ok(jobs)
}

fn check_linkedin_status(status: reqwest::StatusCode) -> anyhow::Result<()> {
    match status.as_u16() {
        999 => anyhow::bail!(
            "LinkedIn rejected the automated jobs request (HTTP 999). Use an authorized job data source."
        ),
        429 => anyhow::bail!("LinkedIn rate-limited the guest jobs request (HTTP 429)"),
        _ if status.is_success() => Ok(()),
        code => anyhow::bail!("LinkedIn returned HTTP {code}"),
    }
}

pub fn linkedin_job_id(url: &str) -> Option<&str> {
    let path = url.split(['?', '#']).next().unwrap_or(url);
    if let Some(slug) = path
        .split_once("/jobs/view/")
        .map(|(_, slug)| slug.trim_end_matches('/'))
    {
        let id = slug.rsplit('-').next().unwrap_or(slug);
        if !id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit()) {
            return Some(id);
        }
    }
    None
}

fn job_key(url: &str) -> &str {
    linkedin_job_id(url).unwrap_or_else(|| url.split(['?', '#']).next().unwrap_or(url))
}

pub async fn fetch_linkedin_job_detail(id: &str) -> anyhow::Result<String> {
    let response = http_client()
        .get(format!(
            "https://www.linkedin.com/jobs-guest/jobs/api/jobPosting/{id}"
        ))
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(Error::without_url)?;
    check_linkedin_status(response.status())?;
    let body = response.text().await.map_err(Error::without_url)?;
    parse_job_description(&body).context("LinkedIn job page did not contain a description")
}

fn parse_job_description(body: &str) -> Option<String> {
    let document = Html::parse_document(body);
    let selector = Selector::parse(".show-more-less-html__markup").unwrap();
    let description = document
        .select(&selector)
        .next()?
        .text()
        .collect::<Vec<_>>()
        .join(" ");
    let description = description.split_whitespace().collect::<Vec<_>>().join(" ");
    (!description.is_empty()).then_some(description)
}

fn parse_jobs(body: &str, base_url: &reqwest::Url, max_results: usize) -> (Vec<JobPosting>, usize) {
    let document = Html::parse_document(body);
    let job_selector = Selector::parse(".base-search-card, li.job-search-result").unwrap();
    let title_selector =
        Selector::parse(".base-search-card__title, .job-search-card__title").unwrap();
    let company_selector =
        Selector::parse(".base-search-card__subtitle, .job-search-card__subtitle").unwrap();
    let location_selector = Selector::parse(".job-search-card__location").unwrap();
    let link_selector = Selector::parse("a.base-card__full-link, a.job-search-card__link").unwrap();
    let time_selector = Selector::parse("time[datetime]").unwrap();

    let mut jobs = Vec::new();
    let mut skipped_links = 0;

    for element in document.select(&job_selector) {
        if jobs.len() == max_results {
            break;
        }
        let card_text = |selector: &Selector| {
            element
                .select(selector)
                .next()
                .map(|field| {
                    field
                        .text()
                        .collect::<String>()
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default()
        };
        let title = card_text(&title_selector);
        let company = card_text(&company_selector);
        let location = card_text(&location_selector);

        let url = element
            .select(&link_selector)
            .next()
            .and_then(|e| e.value().attr("href"))
            .and_then(|href| base_url.join(href).ok())
            .map(|url| url.to_string());
        let Some(url) = url else {
            skipped_links += 1;
            continue;
        };

        let posted_time = element
            .select(&time_selector)
            .next()
            .and_then(|e| e.value().attr("datetime"))
            .unwrap_or("")
            .to_string();

        jobs.push(JobPosting {
            title,
            company,
            location,
            url,
            posted_time,
            description: None,
        });
    }

    let matching_cards = document.select(&job_selector).count();
    tracing::debug!(
        matching_cards,
        skipped_links,
        returned_jobs = jobs.len(),
        "Parsed LinkedIn job cards"
    );
    (jobs, matching_cards)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_linkedin_request_rejection() {
        let status = reqwest::StatusCode::from_u16(999).unwrap();
        let error = check_linkedin_status(status).unwrap_err().to_string();
        assert!(error.contains("LinkedIn rejected the automated jobs request (HTTP 999)"));
    }

    #[test]
    fn identifies_jobs_independently_of_slug_and_tracking() {
        let first = "https://www.linkedin.com/jobs/view/rust-engineer-123?trackingId=one";
        let second = "https://www.linkedin.com/jobs/view/123/?trackingId=two";
        assert_eq!(job_key(first), "123");
        assert_eq!(job_key(first), job_key(second));
    }

    #[test]
    fn parses_guest_job_description() {
        let html = r#"<div class="show-more-less-html__markup">Build <strong>Rust</strong> services.<br>Work remotely.</div>"#;
        assert_eq!(
            parse_job_description(html).as_deref(),
            Some("Build Rust services. Work remotely.")
        );
        assert_eq!(parse_job_description("<div>Unavailable</div>"), None);
    }

    #[test]
    fn parses_public_job_cards() {
        let html = r#"
            <div class="base-card base-search-card">
                <a class="base-card__full-link" href="https://www.linkedin.com/jobs/view/123"></a>
                <h3 class="base-search-card__title">
                    Rust   Developer
                </h3>
                <h4 class="base-search-card__subtitle">
                    <a>Example Co</a>
                </h4>
                <span class="job-search-card__location">
                    San Francisco Bay Area
                </span>
                <time datetime="2026-11-20">Yesterday</time>
            </div>
        "#;
        let base = reqwest::Url::parse("https://www.linkedin.com/jobs/search/").unwrap();
        let (jobs, card_count) = parse_jobs(html, &base, 20);
        assert_eq!(card_count, 1);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].title, "Rust Developer");
        assert_eq!(jobs[0].company, "Example Co");
        assert_eq!(jobs[0].location, "San Francisco Bay Area");
        assert_eq!(jobs[0].url, "https://www.linkedin.com/jobs/view/123");
    }
}
