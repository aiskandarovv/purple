use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

use super::{Provider, ProviderError, ProviderHost};

pub struct Aws {
    pub regions: Vec<String>,
    pub profile: String,
}

/// All commonly available AWS regions with display names.
/// Single source of truth. AWS_REGION_GROUPS references slices of this array.
pub const AWS_REGIONS: &[(&str, &str)] = &[
    // Americas (0..8)
    ("us-east-1", "N. Virginia"),
    ("us-east-2", "Ohio"),
    ("us-west-1", "N. California"),
    ("us-west-2", "Oregon"),
    ("ca-central-1", "Canada Central"),
    ("ca-west-1", "Canada West"),
    ("mx-central-1", "Mexico Central"),
    ("sa-east-1", "Sao Paulo"),
    // Europe (8..16)
    ("eu-west-1", "Ireland"),
    ("eu-west-2", "London"),
    ("eu-west-3", "Paris"),
    ("eu-central-1", "Frankfurt"),
    ("eu-central-2", "Zurich"),
    ("eu-south-1", "Milan"),
    ("eu-south-2", "Spain"),
    ("eu-north-1", "Stockholm"),
    // Asia Pacific (16..30)
    ("ap-northeast-1", "Tokyo"),
    ("ap-northeast-2", "Seoul"),
    ("ap-northeast-3", "Osaka"),
    ("ap-southeast-1", "Singapore"),
    ("ap-southeast-2", "Sydney"),
    ("ap-southeast-3", "Jakarta"),
    ("ap-southeast-4", "Melbourne"),
    ("ap-southeast-5", "Malaysia"),
    ("ap-southeast-6", "New Zealand"),
    ("ap-southeast-7", "Thailand"),
    ("ap-east-1", "Hong Kong"),
    ("ap-east-2", "Taipei"),
    ("ap-south-1", "Mumbai"),
    ("ap-south-2", "Hyderabad"),
    // Middle East / Africa (30..34)
    ("me-south-1", "Bahrain"),
    ("me-central-1", "UAE"),
    ("il-central-1", "Tel Aviv"),
    ("af-south-1", "Cape Town"),
];

/// Region group labels with start..end indices into AWS_REGIONS.
pub const AWS_REGION_GROUPS: &[(&str, usize, usize)] = &[
    ("Americas", 0, 8),
    ("Europe", 8, 16),
    ("Asia Pacific", 16, 30),
    ("Middle East / Africa", 30, 34),
];

// --- Credentials ---

struct AwsCredentials {
    access_key: String,
    secret_key: String,
}

fn resolve_credentials(token: &str, profile: &str) -> Result<AwsCredentials, ProviderError> {
    // Profile takes priority: read from ~/.aws/credentials
    if !profile.is_empty() {
        return read_credentials_file(profile);
    }
    // Token field: ACCESS_KEY_ID:SECRET_ACCESS_KEY
    if let Some((ak, sk)) = token.split_once(':') {
        if !ak.is_empty() && !sk.is_empty() {
            return Ok(AwsCredentials {
                access_key: ak.to_string(),
                secret_key: sk.to_string(),
            });
        }
    }
    // Environment variables
    if let (Ok(ak), Ok(sk)) = (
        std::env::var("AWS_ACCESS_KEY_ID"),
        std::env::var("AWS_SECRET_ACCESS_KEY"),
    ) {
        if !ak.is_empty() && !sk.is_empty() {
            return Ok(AwsCredentials {
                access_key: ak,
                secret_key: sk,
            });
        }
    }
    Err(ProviderError::AuthFailed)
}

/// Parse AWS credentials from INI content (testable without filesystem).
fn parse_credentials(content: &str, profile: &str) -> Option<AwsCredentials> {
    let header = format!("[{}]", profile);
    let mut in_section = false;
    let mut access_key = String::new();
    let mut secret_key = String::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_section = trimmed == header;
            continue;
        }
        if !in_section {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            match key.trim() {
                "aws_access_key_id" => access_key = value.trim().to_string(),
                "aws_secret_access_key" => secret_key = value.trim().to_string(),
                _ => {}
            }
        }
    }

    if access_key.is_empty() || secret_key.is_empty() {
        None
    } else {
        Some(AwsCredentials {
            access_key,
            secret_key,
        })
    }
}

fn read_credentials_file(profile: &str) -> Result<AwsCredentials, ProviderError> {
    let path = dirs::home_dir()
        .ok_or(ProviderError::AuthFailed)?
        .join(".aws")
        .join("credentials");
    let content = std::fs::read_to_string(&path).map_err(|_| ProviderError::AuthFailed)?;
    parse_credentials(&content, profile).ok_or(ProviderError::AuthFailed)
}

// --- SigV4 signing ---

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hasher.finalize().to_vec()
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC accepts any key size");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// RFC 3986 URI encoding.
fn uri_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Format epoch seconds as (timestamp, datestamp) for SigV4.
fn format_utc(epoch_secs: u64) -> (String, String) {
    let secs_per_day = 86400u64;
    let mut remaining_days = epoch_secs / secs_per_day;
    let day_secs = epoch_secs % secs_per_day;
    let hours = day_secs / 3600;
    let minutes = (day_secs % 3600) / 60;
    let seconds = day_secs % 60;

    let mut year = 1970u64;
    loop {
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_per_month: [u64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0usize;
    while month < 12 && remaining_days >= days_per_month[month] {
        remaining_days -= days_per_month[month];
        month += 1;
    }
    let day = remaining_days + 1;

    let timestamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        year,
        month + 1,
        day,
        hours,
        minutes,
        seconds,
    );
    let datestamp = format!("{:04}{:02}{:02}", year, month + 1, day);
    (timestamp, datestamp)
}

/// Build the SigV4 Authorization header value.
fn sign_request(
    creds: &AwsCredentials,
    region: &str,
    host: &str,
    query_string: &str,
    timestamp: &str,
    datestamp: &str,
) -> String {
    let payload_hash = hex_encode(&sha256_hash(b""));
    let canonical_headers = format!("host:{}\nx-amz-date:{}\n", host, timestamp);
    let signed_headers = "host;x-amz-date";

    let canonical_request = format!(
        "GET\n/\n{}\n{}\n{}\n{}",
        query_string, canonical_headers, signed_headers, payload_hash
    );

    let scope = format!("{}/{}/ec2/aws4_request", datestamp, region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        timestamp,
        scope,
        hex_encode(&sha256_hash(canonical_request.as_bytes())),
    );

    let k_date = hmac_sha256(
        format!("AWS4{}", creds.secret_key).as_bytes(),
        datestamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, region.as_bytes());
    let k_service = hmac_sha256(&k_region, b"ec2");
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex_encode(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        creds.access_key, scope, signed_headers, signature
    )
}

// --- XML response structs ---

/// Generic wrapper for AWS XML lists that use repeated <item> elements.
#[derive(serde::Deserialize, Debug)]
#[serde(bound(deserialize = "T: serde::Deserialize<'de>"))]
struct ItemList<T> {
    #[serde(rename = "item", default = "Vec::new")]
    item: Vec<T>,
}

impl<T> Default for ItemList<T> {
    fn default() -> Self {
        Self { item: Vec::new() }
    }
}

#[derive(serde::Deserialize, Debug)]
struct DescribeInstancesResponse {
    #[serde(rename = "reservationSet", default)]
    reservation_set: ItemList<Reservation>,
    #[serde(rename = "nextToken", default)]
    next_token: Option<String>,
}

#[derive(serde::Deserialize, Debug)]
struct Reservation {
    #[serde(rename = "instancesSet", default)]
    instances_set: ItemList<Ec2Instance>,
}

#[derive(serde::Deserialize, Debug)]
struct Ec2Instance {
    #[serde(rename = "instanceId", default)]
    instance_id: String,
    #[serde(rename = "imageId", default)]
    image_id: String,
    #[serde(rename = "instanceState", default)]
    instance_state: InstanceState,
    #[serde(rename = "instanceType", default)]
    instance_type: String,
    #[serde(rename = "tagSet", default)]
    tag_set: ItemList<Ec2Tag>,
    #[serde(rename = "ipAddress", default)]
    ip_address: Option<String>,
    #[serde(rename = "privateIpAddress", default)]
    private_ip_address: Option<String>,
}

#[derive(serde::Deserialize, Debug, Default)]
struct InstanceState {
    #[serde(default)]
    name: String,
}

#[derive(serde::Deserialize, Debug)]
struct Ec2Tag {
    #[serde(default)]
    key: String,
    #[serde(default)]
    value: String,
}

#[derive(serde::Deserialize, Debug)]
struct DescribeImagesResponse {
    #[serde(rename = "imagesSet", default)]
    images_set: ItemList<ImageInfo>,
}

#[derive(serde::Deserialize, Debug)]
struct ImageInfo {
    #[serde(rename = "imageId", default)]
    image_id: String,
    #[serde(default)]
    name: String,
}

// --- EC2 API ---

fn param(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

/// Make a signed GET request to the EC2 API.
fn ec2_get(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    params: Vec<(String, String)>,
) -> Result<String, ProviderError> {
    let host = format!("ec2.{}.amazonaws.com", region);
    let epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (timestamp, datestamp) = format_utc(epoch);

    // Build sorted, URI-encoded query string (SigV4 requires sorted params)
    let mut sorted: Vec<(String, String)> = params
        .into_iter()
        .map(|(k, v)| (uri_encode(&k), uri_encode(&v)))
        .collect();
    sorted.sort();
    let query_string: String = sorted
        .iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect::<Vec<_>>()
        .join("&");

    let auth = sign_request(creds, region, &host, &query_string, &timestamp, &datestamp);
    let url = format!("https://{}/?{}", host, query_string);

    let resp = agent
        .get(&url)
        .set("Authorization", &auth)
        .set("x-amz-date", &timestamp)
        .call()
        .map_err(super::map_ureq_error)?;

    resp.into_string()
        .map_err(|e| ProviderError::Parse(e.to_string()))
}

/// Fetch all non-terminated instances in a region (handles pagination).
fn describe_instances(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    cancel: &AtomicBool,
) -> Result<Vec<Ec2Instance>, ProviderError> {
    let mut all = Vec::new();
    let mut next_token: Option<String> = None;
    let mut page = 0usize;

    loop {
        page += 1;
        if page > 500 {
            break;
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ProviderError::Cancelled);
        }

        let mut params = vec![
            param("Action", "DescribeInstances"),
            param("Version", "2016-11-15"),
        ];
        if let Some(ref token) = next_token {
            params.push(param("NextToken", token));
        }

        let body = ec2_get(agent, creds, region, params)?;
        let resp: DescribeInstancesResponse = quick_xml::de::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("{}: {}", region, e)))?;

        for reservation in resp.reservation_set.item {
            for instance in reservation.instances_set.item {
                if instance.instance_state.name != "terminated"
                    && instance.instance_state.name != "shutting-down"
                {
                    all.push(instance);
                }
            }
        }

        match resp.next_token {
            Some(t) if !t.is_empty() => next_token = Some(t),
            _ => break,
        }
    }

    Ok(all)
}

/// Maximum AMI IDs per DescribeImages request to stay within AWS query limits.
const AMI_BATCH_SIZE: usize = 100;

/// Fetch AMI ID to name mapping (best effort, returns empty map on failure).
/// Batches requests to stay within AWS API limits.
fn fetch_image_names(
    agent: &ureq::Agent,
    creds: &AwsCredentials,
    region: &str,
    image_ids: &[String],
) -> Result<HashMap<String, String>, ProviderError> {
    if image_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut map = HashMap::new();
    for chunk in image_ids.chunks(AMI_BATCH_SIZE) {
        let mut params = vec![
            param("Action", "DescribeImages"),
            param("Version", "2016-11-15"),
        ];
        for (i, id) in chunk.iter().enumerate() {
            params.push(param(&format!("ImageId.{}", i + 1), id));
        }

        let body = ec2_get(agent, creds, region, params)?;
        let resp: DescribeImagesResponse = quick_xml::de::from_str(&body)
            .map_err(|e| ProviderError::Parse(format!("{}: {}", region, e)))?;

        for image in resp.images_set.item {
            if !image.name.is_empty() {
                map.insert(image.image_id, image.name);
            }
        }
    }
    Ok(map)
}

/// Extract Name tag value and user tags from an instance's tag set.
/// Filters out aws:* tags. Returns (name, tags) where tags are values only.
fn extract_tags(tag_set: &[Ec2Tag]) -> (String, Vec<String>) {
    let mut name = String::new();
    let mut tags = Vec::new();
    for tag in tag_set {
        if tag.key == "Name" {
            name = tag.value.clone();
        } else if !tag.key.starts_with("aws:") && !tag.value.is_empty() {
            tags.push(tag.value.clone());
        }
    }
    tags.sort();
    (name, tags)
}

// --- Provider trait ---

impl Provider for Aws {
    fn name(&self) -> &str {
        "aws"
    }

    fn short_label(&self) -> &str {
        "aws"
    }

    fn fetch_hosts_cancellable(
        &self,
        token: &str,
        cancel: &AtomicBool,
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        self.fetch_hosts_with_progress(token, cancel, &|_| {})
    }

    fn fetch_hosts_with_progress(
        &self,
        token: &str,
        cancel: &AtomicBool,
        progress: &dyn Fn(&str),
    ) -> Result<Vec<ProviderHost>, ProviderError> {
        if self.regions.is_empty() {
            return Err(ProviderError::Http(
                "No AWS regions configured. Add regions in the provider settings.".to_string(),
            ));
        }

        let valid_codes: HashSet<&str> = AWS_REGIONS.iter().map(|(c, _)| *c).collect();
        for region in &self.regions {
            if !valid_codes.contains(region.as_str()) {
                return Err(ProviderError::Http(format!(
                    "Unknown AWS region '{}'. Check your provider settings.",
                    region
                )));
            }
        }

        let creds = resolve_credentials(token, &self.profile)?;
        let agent = super::http_agent();
        let total_regions = self.regions.len();
        let mut all_hosts = Vec::new();
        let mut failed_regions = 0usize;

        for (i, region) in self.regions.iter().enumerate() {
            if cancel.load(Ordering::Relaxed) {
                return Err(ProviderError::Cancelled);
            }

            progress(&format!(
                "Fetching {} ({}/{})...",
                region,
                i + 1,
                total_regions
            ));

            let instances = match describe_instances(&agent, &creds, region, cancel) {
                Ok(instances) => instances,
                Err(ProviderError::Cancelled) => return Err(ProviderError::Cancelled),
                Err(ProviderError::AuthFailed) => return Err(ProviderError::AuthFailed),
                Err(ProviderError::RateLimited) => return Err(ProviderError::RateLimited),
                Err(_) => {
                    failed_regions += 1;
                    continue;
                }
            };

            // Collect unique AMI IDs for OS metadata lookup
            let ami_ids: Vec<String> = {
                let mut set = HashSet::new();
                for inst in &instances {
                    if !inst.image_id.is_empty() {
                        set.insert(inst.image_id.clone());
                    }
                }
                set.into_iter().collect()
            };

            // Fetch AMI names (best effort)
            let ami_names = if !ami_ids.is_empty() {
                progress(&format!("Resolving AMIs for {}...", region));
                fetch_image_names(&agent, &creds, region, &ami_ids).unwrap_or_default()
            } else {
                HashMap::new()
            };

            for instance in instances {
                let ip = match instance.ip_address {
                    Some(ref ip) if !ip.is_empty() => ip.clone(),
                    _ => match instance.private_ip_address {
                        Some(ref ip) if !ip.is_empty() => ip.clone(),
                        _ => continue,
                    },
                };

                let (name, tags) = extract_tags(&instance.tag_set.item);
                let name = if name.is_empty() {
                    instance.instance_id.clone()
                } else {
                    name
                };

                let mut metadata = Vec::new();
                metadata.push(("region".to_string(), region.clone()));
                if !instance.instance_type.is_empty() {
                    metadata.push(("instance".to_string(), instance.instance_type.clone()));
                }
                if let Some(os_name) = ami_names.get(&instance.image_id) {
                    metadata.push(("os".to_string(), os_name.clone()));
                }
                if !instance.instance_state.name.is_empty() {
                    metadata.push(("status".to_string(), instance.instance_state.name.clone()));
                }

                all_hosts.push(ProviderHost {
                    server_id: instance.instance_id,
                    name,
                    ip,
                    tags,
                    metadata,
                });
            }
        }

        // Summary
        let mut parts = vec![format!("{} instances", all_hosts.len())];
        if failed_regions > 0 {
            parts.push(format!(
                "{} of {} regions failed",
                failed_regions, total_regions
            ));
        }
        progress(&parts.join(", "));

        if failed_regions > 0 {
            if all_hosts.is_empty() {
                return Err(ProviderError::Http(format!(
                    "All {} regions failed. Check your credentials and region configuration.",
                    total_regions,
                )));
            }
            return Err(ProviderError::PartialResult {
                hosts: all_hosts,
                failures: failed_regions,
                total: total_regions,
            });
        }

        Ok(all_hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // =========================================================================
    // format_utc
    // =========================================================================

    #[test]
    fn test_format_utc_epoch_zero() {
        let (ts, ds) = format_utc(0);
        assert_eq!(ts, "19700101T000000Z");
        assert_eq!(ds, "19700101");
    }

    #[test]
    fn test_format_utc_known_date() {
        // 2024-01-15 12:30:45 UTC = 1705321845
        let (ts, ds) = format_utc(1705321845);
        assert_eq!(ts, "20240115T123045Z");
        assert_eq!(ds, "20240115");
    }

    #[test]
    fn test_format_utc_leap_year() {
        // 2024-02-29 00:00:00 UTC = 1709164800
        let (ts, ds) = format_utc(1709164800);
        assert_eq!(ts, "20240229T000000Z");
        assert_eq!(ds, "20240229");
    }

    #[test]
    fn test_format_utc_end_of_year() {
        // 2023-12-31 23:59:59 UTC = 1704067199
        let (ts, ds) = format_utc(1704067199);
        assert_eq!(ts, "20231231T235959Z");
        assert_eq!(ds, "20231231");
    }

    #[test]
    fn test_format_utc_year_2000() {
        // 2000-03-01 00:00:00 UTC = 951868800
        let (ts, ds) = format_utc(951868800);
        assert_eq!(ts, "20000301T000000Z");
        assert_eq!(ds, "20000301");
    }

    // =========================================================================
    // uri_encode
    // =========================================================================

    #[test]
    fn test_uri_encode_passthrough() {
        assert_eq!(uri_encode("abc123-_.~"), "abc123-_.~");
    }

    #[test]
    fn test_uri_encode_special_chars() {
        assert_eq!(uri_encode("hello world"), "hello%20world");
        assert_eq!(uri_encode("a=b&c"), "a%3Db%26c");
        assert_eq!(uri_encode("/path"), "%2Fpath");
    }

    #[test]
    fn test_uri_encode_empty() {
        assert_eq!(uri_encode(""), "");
    }

    // =========================================================================
    // hex_encode
    // =========================================================================

    #[test]
    fn test_hex_encode() {
        assert_eq!(hex_encode(&[0x00, 0xff, 0xab]), "00ffab");
        assert_eq!(hex_encode(&[]), "");
    }

    // =========================================================================
    // sha256_hash
    // =========================================================================

    #[test]
    fn test_sha256_empty() {
        let hash = hex_encode(&sha256_hash(b""));
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_known() {
        let hash = hex_encode(&sha256_hash(b"hello"));
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    // =========================================================================
    // hmac_sha256
    // =========================================================================

    #[test]
    fn test_hmac_sha256_known() {
        // HMAC-SHA256("key", "message") is a well-known test vector
        let result = hex_encode(&hmac_sha256(
            b"key",
            b"The quick brown fox jumps over the lazy dog",
        ));
        assert_eq!(
            result,
            "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8"
        );
    }

    // =========================================================================
    // sign_request (SigV4)
    // =========================================================================

    #[test]
    fn test_sign_request_format() {
        let creds = AwsCredentials {
            access_key: "AKIDEXAMPLE".to_string(),
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".to_string(),
        };
        let auth = sign_request(
            &creds,
            "us-east-1",
            "ec2.us-east-1.amazonaws.com",
            "Action=DescribeInstances&Version=2016-11-15",
            "20150830T123600Z",
            "20150830",
        );
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/ec2/aws4_request, SignedHeaders=host;x-amz-date, Signature="));
        // Signature should be a 64-char hex string
        let sig = auth.rsplit("Signature=").next().unwrap();
        assert_eq!(sig.len(), 64);
        assert!(sig.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_sign_request_deterministic() {
        let creds = AwsCredentials {
            access_key: "AK".to_string(),
            secret_key: "SK".to_string(),
        };
        let a = sign_request(
            &creds,
            "us-east-1",
            "ec2.us-east-1.amazonaws.com",
            "Action=DescribeInstances",
            "20240101T000000Z",
            "20240101",
        );
        let b = sign_request(
            &creds,
            "us-east-1",
            "ec2.us-east-1.amazonaws.com",
            "Action=DescribeInstances",
            "20240101T000000Z",
            "20240101",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn test_sign_request_different_regions() {
        let creds = AwsCredentials {
            access_key: "AK".to_string(),
            secret_key: "SK".to_string(),
        };
        let a = sign_request(
            &creds,
            "us-east-1",
            "ec2.us-east-1.amazonaws.com",
            "Action=DescribeInstances",
            "20240101T000000Z",
            "20240101",
        );
        let b = sign_request(
            &creds,
            "eu-west-1",
            "ec2.eu-west-1.amazonaws.com",
            "Action=DescribeInstances",
            "20240101T000000Z",
            "20240101",
        );
        assert_ne!(a, b);
    }

    // =========================================================================
    // parse_credentials
    // =========================================================================

    #[test]
    fn test_parse_credentials_default_profile() {
        let content = "[default]\naws_access_key_id = AKID123\naws_secret_access_key = SECRET456\n";
        let creds = parse_credentials(content, "default").unwrap();
        assert_eq!(creds.access_key, "AKID123");
        assert_eq!(creds.secret_key, "SECRET456");
    }

    #[test]
    fn test_parse_credentials_named_profile() {
        let content = "[default]\naws_access_key_id = DEFAULT\naws_secret_access_key = DEFSECRET\n\n[prod]\naws_access_key_id = PRODAK\naws_secret_access_key = PRODSK\n";
        let creds = parse_credentials(content, "prod").unwrap();
        assert_eq!(creds.access_key, "PRODAK");
        assert_eq!(creds.secret_key, "PRODSK");
    }

    #[test]
    fn test_parse_credentials_missing_profile() {
        let content = "[default]\naws_access_key_id = AK\naws_secret_access_key = SK\n";
        assert!(parse_credentials(content, "nonexistent").is_none());
    }

    #[test]
    fn test_parse_credentials_incomplete_profile() {
        let content = "[incomplete]\naws_access_key_id = AK\n";
        assert!(parse_credentials(content, "incomplete").is_none());
    }

    #[test]
    fn test_parse_credentials_whitespace_handling() {
        let content =
            "[default]\n  aws_access_key_id  =  AKID  \n  aws_secret_access_key  =  SECRET  \n";
        let creds = parse_credentials(content, "default").unwrap();
        assert_eq!(creds.access_key, "AKID");
        assert_eq!(creds.secret_key, "SECRET");
    }

    #[test]
    fn test_parse_credentials_extra_keys_ignored() {
        let content = "[default]\naws_access_key_id = AK\naws_secret_access_key = SK\naws_session_token = TOKEN\nregion = us-east-1\n";
        let creds = parse_credentials(content, "default").unwrap();
        assert_eq!(creds.access_key, "AK");
        assert_eq!(creds.secret_key, "SK");
    }

    #[test]
    fn test_parse_credentials_empty_content() {
        assert!(parse_credentials("", "default").is_none());
    }

    // =========================================================================
    // resolve_credentials (token parsing)
    // =========================================================================

    #[test]
    fn test_resolve_credentials_token_format() {
        let creds = resolve_credentials("AKID:SECRET", "").unwrap();
        assert_eq!(creds.access_key, "AKID");
        assert_eq!(creds.secret_key, "SECRET");
    }

    #[test]
    fn test_resolve_credentials_empty_parts() {
        // Empty access key
        assert!(resolve_credentials(":SECRET", "").is_err());
        // Empty secret key
        assert!(resolve_credentials("AKID:", "").is_err());
    }

    #[test]
    fn test_resolve_credentials_no_colon() {
        // No colon in token: split_once fails, falls through to env vars
        // Token-only (no colon) should not produce valid credentials from token path
        let result = resolve_credentials("just-a-token", "");
        // Result depends on env vars. Verify token path was skipped by
        // confirming credentials (if any) don't contain the raw token string.
        if let Ok(ref creds) = result {
            assert_ne!(creds.access_key, "just-a-token");
            assert_ne!(creds.secret_key, "just-a-token");
        }
    }

    // =========================================================================
    // XML parsing: DescribeInstances
    // =========================================================================

    #[test]
    fn test_parse_describe_instances_basic() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <requestId>abc123</requestId>
    <reservationSet>
        <item>
            <reservationId>r-12345</reservationId>
            <instancesSet>
                <item>
                    <instanceId>i-abc123</instanceId>
                    <imageId>ami-12345</imageId>
                    <instanceState><name>running</name></instanceState>
                    <instanceType>t3.micro</instanceType>
                    <ipAddress>1.2.3.4</ipAddress>
                    <placement><availabilityZone>us-east-1a</availabilityZone></placement>
                    <tagSet>
                        <item><key>Name</key><value>web-01</value></item>
                        <item><key>Environment</key><value>prod</value></item>
                    </tagSet>
                </item>
            </instancesSet>
        </item>
    </reservationSet>
</DescribeInstancesResponse>"#;

        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.reservation_set.item.len(), 1);
        let instance = &resp.reservation_set.item[0].instances_set.item[0];
        assert_eq!(instance.instance_id, "i-abc123");
        assert_eq!(instance.image_id, "ami-12345");
        assert_eq!(instance.instance_state.name, "running");
        assert_eq!(instance.instance_type, "t3.micro");
        assert_eq!(instance.ip_address.as_deref(), Some("1.2.3.4"));
        assert_eq!(instance.tag_set.item.len(), 2);
    }

    #[test]
    fn test_parse_describe_instances_no_public_ip() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet>
        <item>
            <instancesSet>
                <item>
                    <instanceId>i-noip</instanceId>
                    <instanceState><name>running</name></instanceState>
                    <tagSet/>
                </item>
            </instancesSet>
        </item>
    </reservationSet>
</DescribeInstancesResponse>"#;

        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        let instance = &resp.reservation_set.item[0].instances_set.item[0];
        assert!(instance.ip_address.is_none());
    }

    #[test]
    fn test_parse_describe_instances_empty() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet/>
</DescribeInstancesResponse>"#;

        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        assert!(resp.reservation_set.item.is_empty());
    }

    #[test]
    fn test_parse_describe_instances_with_next_token() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet/>
    <nextToken>eyJ0b2tlbiI6ICJ0ZXN0In0=</nextToken>
</DescribeInstancesResponse>"#;

        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.next_token.as_deref(), Some("eyJ0b2tlbiI6ICJ0ZXN0In0="));
    }

    #[test]
    fn test_parse_describe_instances_multiple_reservations() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet>
        <item>
            <instancesSet>
                <item>
                    <instanceId>i-001</instanceId>
                    <instanceState><name>running</name></instanceState>
                    <ipAddress>1.1.1.1</ipAddress>
                </item>
            </instancesSet>
        </item>
        <item>
            <instancesSet>
                <item>
                    <instanceId>i-002</instanceId>
                    <instanceState><name>running</name></instanceState>
                    <ipAddress>2.2.2.2</ipAddress>
                </item>
            </instancesSet>
        </item>
    </reservationSet>
</DescribeInstancesResponse>"#;

        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.reservation_set.item.len(), 2);
        assert_eq!(
            resp.reservation_set.item[0].instances_set.item[0].instance_id,
            "i-001"
        );
        assert_eq!(
            resp.reservation_set.item[1].instances_set.item[0].instance_id,
            "i-002"
        );
    }

    // =========================================================================
    // XML parsing: DescribeImages
    // =========================================================================

    #[test]
    fn test_parse_describe_images() {
        let xml = r#"<DescribeImagesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <imagesSet>
        <item>
            <imageId>ami-12345</imageId>
            <name>ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-20240101</name>
        </item>
        <item>
            <imageId>ami-67890</imageId>
            <name>amzn2-ami-hvm-2.0.20240101.0-x86_64-gp2</name>
        </item>
    </imagesSet>
</DescribeImagesResponse>"#;

        let resp: DescribeImagesResponse = quick_xml::de::from_str(xml).unwrap();
        assert_eq!(resp.images_set.item.len(), 2);
        assert_eq!(resp.images_set.item[0].image_id, "ami-12345");
        assert!(resp.images_set.item[0].name.contains("ubuntu"));
        assert_eq!(resp.images_set.item[1].image_id, "ami-67890");
    }

    #[test]
    fn test_parse_describe_images_empty() {
        let xml = r#"<DescribeImagesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <imagesSet/>
</DescribeImagesResponse>"#;

        let resp: DescribeImagesResponse = quick_xml::de::from_str(xml).unwrap();
        assert!(resp.images_set.item.is_empty());
    }

    // =========================================================================
    // extract_tags
    // =========================================================================

    #[test]
    fn test_extract_tags_name_and_values() {
        let tags = vec![
            Ec2Tag {
                key: "Name".to_string(),
                value: "web-01".to_string(),
            },
            Ec2Tag {
                key: "Environment".to_string(),
                value: "prod".to_string(),
            },
            Ec2Tag {
                key: "Team".to_string(),
                value: "backend".to_string(),
            },
        ];
        let (name, extracted) = extract_tags(&tags);
        assert_eq!(name, "web-01");
        assert_eq!(extracted, vec!["backend", "prod"]); // sorted
    }

    #[test]
    fn test_extract_tags_filters_aws_prefix() {
        let tags = vec![
            Ec2Tag {
                key: "Name".to_string(),
                value: "srv".to_string(),
            },
            Ec2Tag {
                key: "aws:cloudformation:stack-name".to_string(),
                value: "my-stack".to_string(),
            },
            Ec2Tag {
                key: "aws:autoscaling:groupName".to_string(),
                value: "my-asg".to_string(),
            },
            Ec2Tag {
                key: "custom".to_string(),
                value: "val".to_string(),
            },
        ];
        let (name, extracted) = extract_tags(&tags);
        assert_eq!(name, "srv");
        assert_eq!(extracted, vec!["val"]);
    }

    #[test]
    fn test_extract_tags_no_name() {
        let tags = vec![Ec2Tag {
            key: "Environment".to_string(),
            value: "dev".to_string(),
        }];
        let (name, extracted) = extract_tags(&tags);
        assert!(name.is_empty());
        assert_eq!(extracted, vec!["dev"]);
    }

    #[test]
    fn test_extract_tags_empty_value_skipped() {
        let tags = vec![Ec2Tag {
            key: "flag".to_string(),
            value: "".to_string(),
        }];
        let (_, extracted) = extract_tags(&tags);
        assert!(extracted.is_empty());
    }

    #[test]
    fn test_extract_tags_empty() {
        let (name, tags) = extract_tags(&[]);
        assert!(name.is_empty());
        assert!(tags.is_empty());
    }

    // =========================================================================
    // AWS_REGIONS constant
    // =========================================================================

    #[test]
    fn test_aws_regions_not_empty() {
        assert!(AWS_REGIONS.len() >= 20);
    }

    #[test]
    fn test_aws_region_groups_cover_all_regions() {
        let total: usize = AWS_REGION_GROUPS.iter().map(|&(_, s, e)| e - s).sum();
        assert_eq!(total, AWS_REGIONS.len());
        // Verify groups are contiguous and non-overlapping
        let mut expected_start = 0;
        for &(_, start, end) in AWS_REGION_GROUPS {
            assert_eq!(start, expected_start, "Gap or overlap in region groups");
            assert!(end > start, "Empty region group");
            expected_start = end;
        }
        assert_eq!(expected_start, AWS_REGIONS.len());
    }

    #[test]
    fn test_aws_regions_no_duplicates() {
        let mut seen = HashSet::new();
        for (code, _) in AWS_REGIONS {
            assert!(seen.insert(code), "Duplicate region: {}", code);
        }
    }

    #[test]
    fn test_aws_regions_contains_common() {
        let codes: Vec<&str> = AWS_REGIONS.iter().map(|(c, _)| *c).collect();
        assert!(codes.contains(&"us-east-1"));
        assert!(codes.contains(&"eu-west-1"));
        assert!(codes.contains(&"ap-northeast-1"));
    }

    // =========================================================================
    // Provider trait
    // =========================================================================

    #[test]
    fn test_aws_provider_name() {
        let aws = Aws {
            regions: vec![],
            profile: String::new(),
        };
        assert_eq!(aws.name(), "aws");
        assert_eq!(aws.short_label(), "aws");
    }

    #[test]
    fn test_aws_no_regions_error() {
        let aws = Aws {
            regions: vec![],
            profile: String::new(),
        };
        let result = aws.fetch_hosts("fake");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("No AWS regions")),
            other => panic!("Expected Http error, got: {:?}", other),
        }
    }

    // =========================================================================
    // param helper
    // =========================================================================

    #[test]
    fn test_param_helper() {
        let (k, v) = param("Action", "DescribeInstances");
        assert_eq!(k, "Action");
        assert_eq!(v, "DescribeInstances");
    }

    // =========================================================================
    // Region validation
    // =========================================================================

    #[test]
    fn test_aws_invalid_region_error() {
        let aws = Aws {
            regions: vec!["xx-invalid-1".to_string()],
            profile: String::new(),
        };
        let result = aws.fetch_hosts("AKID:SECRET");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("Unknown AWS region")),
            other => panic!("Expected Http error for invalid region, got: {:?}", other),
        }
    }

    #[test]
    fn test_aws_mixed_valid_invalid_region_error() {
        let aws = Aws {
            regions: vec!["us-east-1".to_string(), "xx-fake-9".to_string()],
            profile: String::new(),
        };
        let result = aws.fetch_hosts("AKID:SECRET");
        match result {
            Err(ProviderError::Http(msg)) => assert!(msg.contains("xx-fake-9")),
            other => panic!("Expected Http error for invalid region, got: {:?}", other),
        }
    }

    // =========================================================================
    // Profile credential errors return AuthFailed
    // =========================================================================

    #[test]
    fn test_resolve_credentials_bad_profile_returns_auth_failed() {
        // Non-existent profile should return AuthFailed (not Http)
        let result = read_credentials_file("nonexistent-profile-xyz");
        assert!(matches!(result, Err(ProviderError::AuthFailed)));
    }

    // =========================================================================
    // AMI batch constant
    // =========================================================================

    #[test]
    fn test_ami_batch_size_is_reasonable() {
        assert_eq!(
            AMI_BATCH_SIZE, 100,
            "AMI batch size should be 100 (AWS limit per DescribeImages call)"
        );
    }

    // =========================================================================
    // Private IP fallback
    // =========================================================================

    #[test]
    fn test_parse_private_ip_address() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet><item><instancesSet><item>
        <instanceId>i-priv</instanceId>
        <instanceState><name>running</name></instanceState>
        <privateIpAddress>10.0.1.5</privateIpAddress>
        <tagSet/>
    </item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        let inst = &resp.reservation_set.item[0].instances_set.item[0];
        assert!(inst.ip_address.is_none());
        assert_eq!(inst.private_ip_address.as_deref(), Some("10.0.1.5"));
    }

    #[test]
    fn test_public_ip_preferred_over_private() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet><item><instancesSet><item>
        <instanceId>i-both</instanceId>
        <instanceState><name>running</name></instanceState>
        <ipAddress>54.1.2.3</ipAddress>
        <privateIpAddress>10.0.1.5</privateIpAddress>
        <tagSet/>
    </item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        let inst = &resp.reservation_set.item[0].instances_set.item[0];
        assert_eq!(inst.ip_address.as_deref(), Some("54.1.2.3"));
        assert_eq!(inst.private_ip_address.as_deref(), Some("10.0.1.5"));
    }

    #[test]
    fn test_no_ip_at_all_still_parseable() {
        let xml = r#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/2016-11-15/">
    <reservationSet><item><instancesSet><item>
        <instanceId>i-noip</instanceId>
        <instanceState><name>running</name></instanceState>
        <tagSet/>
    </item></instancesSet></item></reservationSet>
</DescribeInstancesResponse>"#;
        let resp: DescribeInstancesResponse = quick_xml::de::from_str(xml).unwrap();
        let inst = &resp.reservation_set.item[0].instances_set.item[0];
        assert!(inst.ip_address.is_none());
        assert!(inst.private_ip_address.is_none());
    }
}
