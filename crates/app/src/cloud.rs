use crate::{AccountSummary, Error, Result, Service};
use cloudweave_engine::{ObjectLocator, ObjectStore, Origin};
use reqwest::{
    StatusCode,
    blocking::{Client, Response},
    header::{LOCATION, RETRY_AFTER},
};
use serde_json::{Value, json};
use std::{sync::Mutex, time::Duration};

pub(crate) fn http() -> Result<Client> {
    Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(120))
        .user_agent("CloudWeave/0.1 experimental")
        .build()
        .map_err(|_| Error::Network("TLS setup"))
}
fn checked(response: Response) -> Result<Response> {
    if !response.status().is_success() {
        return Err(Error::Http(response.status().as_u16()));
    }
    Ok(response)
}
fn json_response(response: Response) -> Result<Value> {
    let bytes = cloudweave_engine::read_stream_bounded(checked(response)?, 1024 * 1024)?;
    serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)
}
fn text(value: &Value, key: &str) -> Result<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 4096)
        .map(str::to_owned)
        .ok_or(Error::Invalid)
}

pub(crate) struct Identity {
    pub subject: String,
    pub label: String,
    pub free_bytes: Option<u64>,
}
pub(crate) fn identity(provider: &str, token: &str, http: &Client) -> Result<Identity> {
    match provider {
        "google" => {
            let value = json_response(
                http.get("https://www.googleapis.com/drive/v3/about")
                    .bearer_auth(token)
                    .query(&[(
                        "fields",
                        "user(permissionId,displayName,emailAddress),storageQuota(limit,usage)",
                    )])
                    .send()
                    .map_err(|_| Error::Network("identity"))?,
            )?;
            let subject = text(&value["user"], "permissionId")?;
            let label = text(&value["user"], "emailAddress")
                .or_else(|_| text(&value["user"], "displayName"))?;
            let quota = &value["storageQuota"];
            let free_bytes = quota["limit"]
                .as_str()
                .and_then(|v| v.parse::<u64>().ok())
                .zip(quota["usage"].as_str().and_then(|v| v.parse::<u64>().ok()))
                .map(|(limit, usage)| limit.saturating_sub(usage));
            Ok(Identity {
                subject,
                label,
                free_bytes,
            })
        }
        "dropbox" => {
            let value = json_response(
                http.post("https://api.dropboxapi.com/2/users/get_current_account")
                    .bearer_auth(token)
                    .header("Content-Type", "application/json")
                    .body("null")
                    .send()
                    .map_err(|_| Error::Network("identity"))?,
            )?;
            let subject = text(&value, "account_id")?;
            let label = text(&value, "email").or_else(|_| text(&value["name"], "display_name"))?;
            let quota = json_response(
                http.post("https://api.dropboxapi.com/2/users/get_space_usage")
                    .bearer_auth(token)
                    .header("Content-Type", "application/json")
                    .body("null")
                    .send()
                    .map_err(|_| Error::Network("quota"))?,
            )?;
            // Team quotas can be shared; never imply a separate allocation per identity.
            let free_bytes = if quota["allocation"][".tag"] == "individual" {
                quota["allocation"]["allocated"]
                    .as_u64()
                    .zip(quota["used"].as_u64())
                    .map(|(limit, used)| limit.saturating_sub(used))
            } else {
                None
            };
            Ok(Identity {
                subject,
                label,
                free_bytes,
            })
        }
        _ => Err(Error::Invalid),
    }
}

pub(crate) struct CloudStore {
    service: Service,
    account: AccountSummary,
    http: Client,
    google_folder: Mutex<Option<String>>,
}
impl CloudStore {
    pub fn new(service: Service, account: AccountSummary) -> Result<Self> {
        Ok(Self {
            service,
            account,
            http: http()?,
            google_folder: Mutex::new(None),
        })
    }
    fn folder(&self, token: &str) -> Result<String> {
        let mut cached = self.google_folder.lock().map_err(|_| Error::Invalid)?;
        if let Some(id) = cached.as_ref() {
            return Ok(id.clone());
        }
        let mut page = String::new();
        let mut found = Vec::new();
        for _ in 0..100 {
            let mut request = self.http.get("https://www.googleapis.com/drive/v3/files").bearer_auth(token).query(&[
                ("q", "trashed = false and mimeType = 'application/vnd.google-apps.folder' and appProperties has { key='cloudweave' and value='v1' }"),
                ("fields", "files(id),nextPageToken"), ("pageSize", "1000")]);
            if !page.is_empty() {
                request = request.query(&[("pageToken", page.as_str())]);
            }
            let value = json_response(
                request
                    .send()
                    .map_err(|_| Error::Network("folder listing"))?,
            )?;
            for file in value["files"].as_array().ok_or(Error::Invalid)? {
                found.push(text(file, "id")?);
            }
            if let Some(next) = value["nextPageToken"].as_str() {
                page = next.into();
            } else {
                page.clear();
                break;
            }
        }
        if !page.is_empty() {
            return Err(Error::Invalid);
        }
        found.sort();
        let id = if let Some(id) = found.into_iter().next() {
            id
        } else {
            let value = json_response(self.http.post("https://www.googleapis.com/drive/v3/files").bearer_auth(token).query(&[("fields","id")])
                .json(&json!({"name":"CloudWeave", "mimeType":"application/vnd.google-apps.folder", "appProperties":{"cloudweave":"v1"}})).send().map_err(|_| Error::Network("folder creation"))?)?;
            text(&value, "id")?
        };
        *cached = Some(id.clone());
        Ok(id)
    }
    fn put_cloud(&self, name: &str, data: &[u8]) -> Result<ObjectLocator> {
        if name.is_empty()
            || name.len() > 240
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || data.len() > 8 * 1024 * 1024 + 40
        {
            return Err(Error::Invalid);
        }
        self.service.with_token(&self.account, |token| {
            let value = if self.account.provider == "google" {
                let folder = self.folder(token)?;
                let metadata = json!({"name":name, "parents":[folder], "appProperties":{"cloudweave":"v1"}});
                let session = checked(self.http.post("https://www.googleapis.com/upload/drive/v3/files").bearer_auth(token).query(&[("uploadType","resumable"),("fields","id")])
                    .header("X-Upload-Content-Type","application/octet-stream").header("X-Upload-Content-Length", data.len()).json(&metadata).send().map_err(|_| Error::Network("upload session"))?)?;
                let signed = session.headers().get(LOCATION).and_then(|v|v.to_str().ok()).ok_or(Error::Invalid)?;
                let url = reqwest::Url::parse(signed).map_err(|_| Error::Invalid)?;
                // Never forward credentials to a redirect or arbitrary signed origin.
                if url.scheme() != "https" || url.host_str() != Some("www.googleapis.com") || url.port_or_known_default() != Some(443)
                    || url.path() != "/upload/drive/v3/files" || !url.username().is_empty() || url.password().is_some() { return Err(Error::Invalid); }
                json_response(self.http.put(url).bearer_auth(token).header("Content-Type","application/octet-stream").body(data.to_vec()).send().map_err(|_| Error::Network("upload"))?)?
            } else if self.account.provider == "dropbox" {
                let args = json!({"path":format!("/{name}"),"mode":"add","autorename":false,"mute":true,"strict_conflict":true});
                json_response(self.http.post("https://content.dropboxapi.com/2/files/upload").bearer_auth(token).header("Dropbox-API-Arg",args.to_string()).header("Content-Type","application/octet-stream").body(data.to_vec()).send().map_err(|_| Error::Network("upload"))?)?
            } else { return Err(Error::Invalid); };
            Ok(ObjectLocator { id: text(&value,"id")? })
        })
    }
    fn get_cloud(&self, locator: &ObjectLocator, max_bytes: usize) -> Result<Vec<u8>> {
        if locator.id.is_empty() || locator.id.len() > 4096 || max_bytes > 8 * 1024 * 1024 + 40 {
            return Err(Error::Invalid);
        }
        self.service.with_token(&self.account, |token| {
            // Only safe reads retry. An ambiguous upload is retained as an orphan,
            // never considered committed and never retried with mutable bytes.
            for attempt in 0..3 {
                let response = if self.account.provider == "google" {
                    let mut url = reqwest::Url::parse("https://www.googleapis.com/drive/v3/files/")
                        .map_err(|_| Error::Invalid)?;
                    url.path_segments_mut()
                        .map_err(|_| Error::Invalid)?
                        .pop_if_empty()
                        .push(&locator.id);
                    self.http
                        .get(url)
                        .bearer_auth(token)
                        .query(&[("alt", "media")])
                        .send()
                } else if self.account.provider == "dropbox" {
                    self.http
                        .post("https://content.dropboxapi.com/2/files/download")
                        .bearer_auth(token)
                        .header("Dropbox-API-Arg", json!({"path":locator.id}).to_string())
                        .send()
                } else {
                    return Err(Error::Invalid);
                }
                .map_err(|_| Error::Network("download"))?;
                if attempt < 2
                    && (response.status() == StatusCode::TOO_MANY_REQUESTS
                        || response.status().is_server_error())
                {
                    let retry = match response.headers().get(RETRY_AFTER) {
                        None => 1 << attempt,
                        Some(header) => {
                            let value = header
                                .to_str()
                                .map_err(|_| Error::Http(response.status().as_u16()))?;
                            if let Ok(seconds) = value.parse::<u64>() {
                                seconds
                            } else {
                                httpdate::parse_http_date(value)
                                    .map_err(|_| Error::Http(response.status().as_u16()))?
                                    .duration_since(std::time::SystemTime::now())
                                    .unwrap_or_default()
                                    .as_secs()
                                    .saturating_add(1)
                            }
                        }
                    };
                    // Long Retry-After is a pause/failure, never an early retry.
                    if retry > 30 {
                        return Err(Error::Http(response.status().as_u16()));
                    }
                    let jitter = u64::from(uuid::Uuid::new_v4().as_bytes()[0]);
                    std::thread::sleep(Duration::from_millis(retry * 1000 + jitter));
                    continue;
                }
                let response = checked(response)?;
                return Ok(cloudweave_engine::read_stream_bounded(response, max_bytes)?);
            }
            Err(Error::Network("download retry limit"))
        })
    }
}
impl ObjectStore for CloudStore {
    fn origin(&self) -> Origin {
        Origin::Cloud {
            provider: self.account.provider.clone(),
            subject: self.account.subject.clone(),
        }
    }
    fn put(&self, name: &str, data: &[u8]) -> cloudweave_engine::Result<ObjectLocator> {
        self.put_cloud(name, data)
            .map_err(|e| cloudweave_engine::Error::Provider(e.to_string()))
    }
    fn get(&self, locator: &ObjectLocator, max_bytes: usize) -> cloudweave_engine::Result<Vec<u8>> {
        self.get_cloud(locator, max_bytes)
            .map_err(|e| cloudweave_engine::Error::Provider(e.to_string()))
    }
}
