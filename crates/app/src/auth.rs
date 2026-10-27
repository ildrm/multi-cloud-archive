use crate::{Error, Result};
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl, basic::BasicClient,
};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub(crate) struct Tokens {
    pub access: String,
    pub refresh: Option<String>,
    pub client_id: String,
    pub desktop_client_value: Option<String>,
    pub expires_at: u64,
}
pub(crate) fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn endpoints(provider: &str) -> Result<(&'static str, &'static str)> {
    match provider {
        "google" => Ok((
            "https://accounts.google.com/o/oauth2/v2/auth",
            "https://oauth2.googleapis.com/token",
        )),
        "dropbox" => Ok((
            "https://www.dropbox.com/oauth2/authorize",
            "https://api.dropboxapi.com/oauth2/token",
        )),
        _ => Err(Error::Invalid),
    }
}

pub(crate) fn connect(
    provider: &str,
    client_id: String,
    desktop_client_value: Option<String>,
    http: &Client,
) -> Result<Tokens> {
    if client_id.trim().is_empty() || client_id.len() > 1024 {
        return Err(Error::Invalid);
    }
    let (auth_url, token_url) = endpoints(provider)?;
    // Dropbox requires this exact URI to be registered. Google's desktop app
    // registration accepts an ephemeral loopback port. Neither flow is Android.
    let listener = TcpListener::bind(if provider == "google" {
        "127.0.0.1:0"
    } else {
        "127.0.0.1:53682"
    })?;
    listener.set_nonblocking(true)?;
    let redirect = format!(
        "http://127.0.0.1:{}/oauth/callback",
        listener.local_addr()?.port()
    );
    let mut client = BasicClient::new(ClientId::new(client_id.clone()))
        .set_auth_uri(AuthUrl::new(auth_url.into()).map_err(|_| Error::Invalid)?)
        .set_token_uri(TokenUrl::new(token_url.into()).map_err(|_| Error::Invalid)?)
        .set_auth_type(AuthType::RequestBody)
        .set_redirect_uri(RedirectUrl::new(redirect.clone()).map_err(|_| Error::Invalid)?);
    if let Some(value) = &desktop_client_value {
        client = client.set_client_secret(ClientSecret::new(value.clone()));
    }
    let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
    let mut request = client
        .authorize_url(CsrfToken::new_random)
        .set_pkce_challenge(challenge);
    if provider == "google" {
        request = request
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/drive.file".into(),
            ))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent select_account");
    } else {
        request = request
            .add_scope(Scope::new(
                "account_info.read files.content.read files.content.write".into(),
            ))
            .add_extra_param("token_access_type", "offline")
            .add_extra_param("force_reapprove", "true");
    }
    let (authorize_url, expected_state) = request.url();
    webbrowser::open(authorize_url.as_str()).map_err(|_| Error::Authorization)?;
    let code = receive_callback(listener, &redirect, &expected_state)?;
    let response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(verifier)
        .request(http)
        .map_err(|_| Error::Authorization)?;
    Ok(Tokens {
        access: response.access_token().secret().clone(),
        refresh: response.refresh_token().map(|t| t.secret().clone()),
        client_id,
        desktop_client_value,
        expires_at: now().saturating_add(
            response
                .expires_in()
                .unwrap_or(Duration::from_secs(300))
                .as_secs(),
        ),
    })
}

pub(crate) fn refresh(provider: &str, tokens: &mut Tokens, http: &Client) -> Result<()> {
    if tokens.expires_at > now().saturating_add(60) {
        return Ok(());
    }
    let (_, endpoint) = endpoints(provider)?;
    let mut client = BasicClient::new(ClientId::new(tokens.client_id.clone()))
        .set_token_uri(TokenUrl::new(endpoint.into()).map_err(|_| Error::Invalid)?)
        .set_auth_type(AuthType::RequestBody);
    if let Some(value) = &tokens.desktop_client_value {
        client = client.set_client_secret(ClientSecret::new(value.clone()));
    }
    let refresh = RefreshToken::new(tokens.refresh.as_ref().ok_or(Error::Authorization)?.clone());
    let response = client
        .exchange_refresh_token(&refresh)
        .request(http)
        .map_err(|_| Error::Authorization)?;
    tokens.access.zeroize();
    tokens.access = response.access_token().secret().clone();
    if let Some(next) = response.refresh_token() {
        tokens.refresh.zeroize();
        tokens.refresh = Some(next.secret().clone());
    }
    tokens.expires_at = now().saturating_add(
        response
            .expires_in()
            .unwrap_or(Duration::from_secs(300))
            .as_secs(),
    );
    Ok(())
}

fn receive_callback(
    listener: TcpListener,
    redirect: &str,
    state: &CsrfToken,
) -> Result<Zeroizing<String>> {
    let deadline = Instant::now() + Duration::from_secs(180);
    while Instant::now() < deadline {
        let (mut stream, address) = match listener.accept() {
            Ok(value) => value,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        if !address.ip().is_loopback() {
            continue;
        }
        stream.set_read_timeout(Some(Duration::from_secs(1)))?;
        stream.set_write_timeout(Some(Duration::from_secs(1)))?;
        let mut bytes = Zeroizing::new(Vec::new());
        let mut one = [0u8; 1];
        let connection_deadline = deadline.min(Instant::now() + Duration::from_secs(5));
        while bytes.len() < 8192 {
            if Instant::now() >= connection_deadline {
                break;
            }
            stream.set_read_timeout(Some(
                connection_deadline
                    .saturating_duration_since(Instant::now())
                    .min(Duration::from_secs(1))
                    .max(Duration::from_millis(1)),
            ))?;
            if stream.read(&mut one).unwrap_or(0) == 0 {
                break;
            }
            bytes.push(one[0]);
            if bytes.ends_with(b"\r\n\r\n") {
                break;
            }
        }
        let result = if bytes.ends_with(b"\r\n\r\n") {
            std::str::from_utf8(&bytes)
                .ok()
                .and_then(|request| validate_callback(request, redirect, state))
        } else {
            None
        };
        let success = result.is_some();
        let body = if success {
            "Authorization received. You can return to CloudWeave."
        } else {
            "Invalid authorization response. Return to CloudWeave and try again."
        };
        let response = format!(
            "HTTP/1.1 {}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\n{body}",
            if success { "200 OK" } else { "400 Bad Request" },
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        if let Some(code) = result {
            return Ok(Zeroizing::new(code));
        }
    }
    Err(Error::Authorization)
}
fn validate_callback(request: &str, redirect: &str, state: &CsrfToken) -> Option<String> {
    let mut lines = request.split("\r\n");
    let mut words = lines.next()?.split_whitespace();
    if words.next()? != "GET" {
        return None;
    }
    let target = words.next()?;
    if !target.starts_with('/') || target.starts_with("//") {
        return None;
    }
    let expected = Url::parse(redirect).ok()?;
    let expected_host = format!("127.0.0.1:{}", expected.port()?);
    let hosts: Vec<_> = lines
        .filter_map(|line| line.split_once(':'))
        .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        .collect();
    if hosts.len() != 1 || hosts[0].1.trim() != expected_host {
        return None;
    }
    let url = expected.join(target).ok()?;
    if url.origin() != expected.origin() || url.path() != expected.path() {
        return None;
    }
    let values: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = values.iter().filter(|(key, _)| key == "state").collect();
    let codes: Vec<_> = values.iter().filter(|(key, _)| key == "code").collect();
    if states.len() != 1
        || codes.len() != 1
        || codes[0].1.is_empty()
        || values.iter().any(|(key, _)| key == "error")
    {
        return None;
    }
    if CsrfToken::new(states[0].1.to_string()) != *state {
        return None;
    }
    Some(codes[0].1.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loopback_response_requires_exact_state_target_and_host() {
        let expected = "http://127.0.0.1:1234/oauth/callback";
        let state = CsrfToken::new("nonce".into());
        let valid =
            "GET /oauth/callback?code=abc&state=nonce HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n\r\n";
        assert_eq!(
            validate_callback(valid, expected, &state).as_deref(),
            Some("abc")
        );
        for bad in [
            valid.replace("nonce", "wrong"),
            valid.replace("Host:", "Other:"),
            valid.replace("/oauth/callback?", "/other?"),
            valid.replace("code=abc", "code=abc&code=def"),
            valid.replace("GET /", "GET http://evil/"),
        ] {
            assert!(validate_callback(&bad, expected, &state).is_none());
        }
    }
}
