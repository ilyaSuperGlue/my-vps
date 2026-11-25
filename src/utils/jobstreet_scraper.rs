use anyhow::Context;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

const SEARCH_PATH: &str = "/api/jobsearch/v5/search";
const GRAPHQL_PATH: &str = "/graphql";

const MAX_PAGE_SIZE: u32 = 40;

#[derive(Debug, Clone, Serialize, serde::Deserialize, utoipa::ToSchema)]
pub struct JobStreetPosting {
    pub id: String,
    pub title: String,
    pub company: String,
    pub company_id: Option<String>,
    pub company_logo: Option<String>,
    pub location: String,
    pub locations: Vec<Value>,
    pub url: String,
    pub posted_time: String,
    pub listed_at: Option<String>,
    pub description: Option<String>,
    pub bullet_points: Vec<String>,
    pub salary: Option<String>,
    pub work_types: Vec<String>,
    pub work_arrangements: Vec<String>,
    pub classifications: Vec<Value>,
    pub advertiser: Option<Value>,
    pub recruiter: Option<Value>,
    pub role_id: Option<String>,
    pub is_featured: bool,
    pub is_sponsored: bool,
}

#[derive(Debug, Clone)]
pub struct JobStreetPage {
    pub jobs: Vec<JobStreetPosting>,
    pub total_count: u64,
}

pub async fn fetch_jobs(
    search: &str,
    country: &str,
    page: u32,
    limit: u32,
    hours_old: u32,
) -> anyhow::Result<JobStreetPage> {
    let (_country_code, host, locale, site_key, country_name) = country_config(country)?;
    let date_range = std::cmp::max(1, hours_old.div_ceil(24));
    let client = reqwest::Client::new();
    let response = client
        .get(format!("https://{host}{SEARCH_PATH}"))
        .header("accept", "application/json, text/plain, */*")
        .header("user-agent", "my-vps-job-api/1.0")
        .query(&[
            ("siteKey", site_key),
            ("sourcesystem", "houston"),
            ("locale", locale),
            ("keywords", search),
            ("where", country_name),
            ("page", &page.to_string()),
            ("pageSize", &limit.to_string()),
            ("daterange", &date_range.to_string()),
        ])
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("JobStreet request failed")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read JobStreet response")?;
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("JobStreet rejected the public API request (HTTP {status})");
    }
    if !status.is_success() {
        anyhow::bail!("JobStreet returned HTTP {status}");
    }

    let payload: Value =
        serde_json::from_str(&body).context("JobStreet returned a non-JSON response")?;
    let jobs = payload
        .get("data")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| parse_job(item, host))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let total_count = payload
        .get("totalCount")
        .and_then(Value::as_u64)
        .unwrap_or(jobs.len() as u64);

    Ok(JobStreetPage { jobs, total_count })
}

/// Fetches the full job advertisement from JobStreet's public GraphQL API.
/// This is the same API used by the public job page and does not require login.
pub async fn fetch_job_detail(id: &str, country: &str) -> anyhow::Result<Value> {
    let (_country_code, host, locale, _site_key, _country_name) = country_config(country)?;
    let query = r#"query jobDetails($jobId: ID!, $zone: Zone!, $locale: Locale!, $languageCode: LanguageCodeIso!, $countryCode: CountryCodeIso2!, $timezone: Timezone!, $visitorId: UUID!) {
      jobDetails(id: $jobId) {
        job {
          id title phoneNumber isExpired isLinkOut isProfileApply isVerified status source abstract content(platform: WEB)
          url(zone: $zone, locale: $locale)
          createdAt { dateTimeUtc }
          updatedAt { dateTimeUtc }
          listedAt { label(context: JOB_POSTED, length: SHORT, timezone: $timezone, locale: $locale) dateTimeUtc }
          expiresAt { dateTimeUtc }
          expiredAt { dateTimeUtc }
          contactMatches { type value }
          salary { label currencyLabel(zone: $zone) }
          workTypes { label(locale: $locale) }
          classifications { label(languageCode: $languageCode) }
          products {
            branding { id cover { url } logo { url } }
            bullets
            questionnaire { questions }
            video { url position }
            displayTags { label(locale: $locale) }
          }
          advertiser { id name(locale: $locale) isVerified registrationDate { dateTimeUtc } }
          location { label(locale: $locale, type: LONG) }
        }
        insights {
          message(locale: $locale)
          ... on ApplicantCount { count countLabel(locale: $locale) volumeLabel(locale: $locale) }
          ... on ApplicantsWithResumePercentage { percentage }
          ... on ApplicantsWithCoverLetterPercentage { percentage }
        }
        badges(visitorId: $visitorId, platform: WEB, locale: $locale) { badges { badge } }
        companyTags { key value }
        companySearchUrl(zone: $zone, languageCode: $languageCode)
        gfjInfo { location { countryCode country(locale: $locale) suburb(locale: $locale) region(locale: $locale) state(locale: $locale) postcode } }
        seoInfo { normalisedRoleTitle workType classification subClassification where(zone: $zone) broaderLocationName(locale: $locale) normalisedOrganisationName }
        restrictedApplication(countryCode: $countryCode) { label(locale: $locale) }
        companyProfile(zone: $zone) {
          id organisationId name companyNameSlug shouldDisplayReviews
          branding { logo }
          overview { description { paragraphs } industry size { description } website { url } }
          reviewsSummary { overallRating { numberOfReviews { value } value } }
          perksAndBenefits { title }
        }
        sourcrRecruiter { name jobTitle overallRating reviewCount avatarImageUrl agencyImageUrl contactUrl agencyName }
      }
    }"#;
    let variables = serde_json::json!({
        "jobId": id,
        "zone": country_zone(country),
        "locale": locale,
        "languageCode": language_code(country),
        "countryCode": country,
        "timezone": country_timezone(country),
        "visitorId": uuid_like_visitor_id(),
    });
    let response = reqwest::Client::new()
        .post(format!("https://{host}{GRAPHQL_PATH}"))
        .header("accept", "application/json, text/plain, */*")
        .header("content-type", "application/json")
        .header("seek-request-brand", "jobstreet")
        .header("seek-request-country", country)
        .header("x-seek-site", "chalice")
        .header("origin", format!("https://{host}"))
        .header("referer", format!("https://{host}/"))
        .json(&serde_json::json!({
            "operationName": "jobDetails",
            "variables": variables,
            "query": query,
        }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .context("JobStreet detail request failed")?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("Failed to read JobStreet detail response")?;
    if status == StatusCode::NOT_FOUND {
        anyhow::bail!("JobStreet job {id} was not found");
    }
    if status == StatusCode::FORBIDDEN || status == StatusCode::TOO_MANY_REQUESTS {
        anyhow::bail!("JobStreet rejected the detail request (HTTP {status})");
    }
    if !status.is_success() {
        anyhow::bail!("JobStreet returned HTTP {status}");
    }
    let payload: Value =
        serde_json::from_str(&body).context("JobStreet detail returned non-JSON")?;
    if let Some(errors) = payload.get("errors") {
        if payload
            .get("data")
            .and_then(|data| data.get("jobDetails"))
            .is_none()
        {
            anyhow::bail!("JobStreet detail failed: {errors}");
        }
    }
    payload
        .get("data")
        .and_then(|data| data.get("jobDetails"))
        .filter(|detail| !detail.is_null())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("JobStreet job {id} was not found"))
}

fn country_zone(country: &str) -> &'static str {
    match country {
        "SG" => "asia-7",
        "ID" => "asia-4",
        "PH" => "asia-6",
        _ => "asia-5",
    }
}

fn language_code(country: &str) -> &'static str {
    match country {
        "ID" => "id",
        _ => "en",
    }
}

fn country_timezone(country: &str) -> &'static str {
    match country {
        "SG" => "Asia/Singapore",
        "MY" => "Asia/Kuala_Lumpur",
        "ID" => "Asia/Jakarta",
        "PH" => "Asia/Manila",
        _ => "Asia/Kuala_Lumpur",
    }
}

fn uuid_like_visitor_id() -> String {
    format!(
        "00000000-0000-4000-8000-{:012x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            & 0xffffffffffff
    )
}

pub fn validate_limit(limit: u32) -> anyhow::Result<()> {
    if (1..=MAX_PAGE_SIZE).contains(&limit) {
        Ok(())
    } else {
        anyhow::bail!("limit must be between 1 and {MAX_PAGE_SIZE}");
    }
}

fn country_config(
    country: &str,
) -> anyhow::Result<(
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
)> {
    match country {
        "MY" => Ok(("MY", "my.jobstreet.com", "en-MY", "MY-Main", "Malaysia")),
        "SG" => Ok(("SG", "sg.jobstreet.com", "en-SG", "SG-Main", "Singapore")),
        "ID" => Ok(("ID", "id.jobstreet.com", "en-ID", "ID-Main", "Indonesia")),
        "PH" => Ok(("PH", "ph.jobstreet.com", "en-PH", "PH-Main", "Philippines")),
        _ => anyhow::bail!("country must be Malaysia, Singapore, Indonesia, or Philippines"),
    }
}

fn parse_job(item: &Value, host: &str) -> Option<JobStreetPosting> {
    let id = text(item.get("id"))?;
    let title = text(item.get("title")).unwrap_or_default();
    let company = text(item.get("companyName"))
        .or_else(|| {
            item.get("employer")
                .and_then(|employer| text(employer.get("name")))
        })
        .unwrap_or_default();
    let locations = item
        .get("locations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let location = locations
        .first()
        .and_then(|location| text(location.get("label")))
        .or_else(|| text(item.get("location")))
        .unwrap_or_default();
    let posted_time = text(item.get("listingDate"))
        .or_else(|| text(item.get("listingDateDisplay")))
        .unwrap_or_default();
    let bullet_points = item
        .get("bulletPoints")
        .or_else(|| item.get("bullet_points"))
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(|item| text(Some(item))).collect())
        .unwrap_or_default();
    let work_types = string_list(item, &["workTypes", "work_types"]);
    let work_arrangements = string_list(item, &["workArrangements", "work_arrangements"]);
    let company_id = item
        .get("employer")
        .and_then(|employer| text(employer.get("companyId")))
        .or_else(|| text(item.get("companyId")));
    let company_logo = item
        .get("branding")
        .and_then(|branding| text(branding.get("serpLogoUrl")))
        .or_else(|| text(item.get("companyLogo")));

    let url = format!("https://{host}/job/{id}");

    Some(JobStreetPosting {
        id,
        title,
        company,
        company_id,
        company_logo,
        location,
        locations,
        url,
        posted_time,
        listed_at: text(item.get("listingDate")),
        description: text(item.get("teaser")),
        bullet_points,
        salary: text(item.get("salaryLabel")).or_else(|| text(item.get("salary"))),
        work_types,
        work_arrangements,
        classifications: item
            .get("classifications")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        advertiser: item.get("advertiser").cloned(),
        recruiter: item.get("recruiter").cloned(),
        role_id: text(item.get("roleId")),
        is_featured: item
            .get("isFeatured")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        is_sponsored: item
            .get("isSponsored")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

fn string_list(item: &Value, keys: &[&str]) -> Vec<String> {
    keys.iter()
        .find_map(|key| item.get(key).and_then(Value::as_array))
        .map(|items| {
            items
                .iter()
                .filter_map(|item| text(Some(item)).or_else(|| text(item.get("label"))))
                .collect()
        })
        .unwrap_or_default()
}

fn text(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(ToOwned::to_owned)
}
