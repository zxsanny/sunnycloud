//! The development `ProviderLogin` of `admin dev-seed` (AZ-3795 AC-4): the user's step at the identity provider, done
//! over HTTP against the mock identity provider (`tests/mocks/idp`). It opens the authorization URL with the seed's
//! subject as `login_hint`, does not follow the redirect, and hands the `code` and `state` of the `Location` back.
//! Plain HTTP only: the mock listens on loopback, and a real provider needs a person.

use std::time::Duration;

use contracts::{CallbackParams, Fault, async_trait};
use sunnycloud_admin::ProviderLogin;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const PLAIN_HTTP: &str = "http://";
const REDIRECT: u16 = 302;
const ANSWER_LIMIT: u64 = 64 * 1024;
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Error)]
enum LoginError {
    #[error(
        "the authorization URL is not plain http (the dev seed signs in at the mock identity provider only)"
    )]
    NotPlainHttp,
    #[error("the subject `{0}` is not a plain name")]
    BadSubject(String),
    #[error("the identity provider step failed: {0}")]
    Step(String),
    #[error("the identity provider answered {0}, not a redirect")]
    NotRedirect(u16),
    #[error("the redirect carries no `code` and `state`")]
    NoCallback,
}

pub(crate) struct HttpProviderLogin;

#[async_trait]
impl ProviderLogin for HttpProviderLogin {
    async fn authorize(
        &self,
        authorization_url: &str,
        subject: &str,
    ) -> Result<CallbackParams, Fault> {
        let location =
            tokio::time::timeout(STEP_TIMEOUT, redirect_location(authorization_url, subject))
                .await
                .map_err(|_| LoginError::Step("timed out".to_owned()))??;
        Ok(callback(&location)?)
    }
}

async fn redirect_location(url: &str, subject: &str) -> Result<String, LoginError> {
    if !subject
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(LoginError::BadSubject(subject.to_owned()));
    }
    let rest = url
        .strip_prefix(PLAIN_HTTP)
        .ok_or(LoginError::NotPlainHttp)?;
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let separator = if path.contains('?') { '&' } else { '?' };
    let request = format!(
        "GET /{path}{separator}login_hint={subject} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    );
    let step = |error: std::io::Error| LoginError::Step(error.to_string());
    let mut stream = TcpStream::connect(authority).await.map_err(step)?;
    stream.write_all(request.as_bytes()).await.map_err(step)?;
    let mut head = Vec::new();
    stream
        .take(ANSWER_LIMIT)
        .read_to_end(&mut head)
        .await
        .map_err(step)?;
    let text = String::from_utf8_lossy(&head);
    let mut lines = text.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| LoginError::Step("the answer is not an HTTP status line".to_owned()))?;
    if status != REDIRECT {
        return Err(LoginError::NotRedirect(status));
    }
    lines
        .take_while(|line| !line.is_empty())
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("location")
                .then(|| value.trim().to_owned())
        })
        .ok_or(LoginError::NoCallback)
}

fn callback(location: &str) -> Result<CallbackParams, LoginError> {
    let query = location.split_once('?').map_or("", |(_, query)| query);
    let query = query.split('#').next().unwrap_or("");
    let params: Vec<(String, String)> = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (decode(name), decode(value)))
        .collect();
    let has = |name: &str| params.iter().any(|(key, _)| key == name);
    if has("code") && has("state") {
        Ok(CallbackParams(params))
    } else {
        Err(LoginError::NoCallback)
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => out.push(b' '),
            b'%' if index + 2 < bytes.len()
                && hex(bytes[index + 1]).is_some()
                && hex(bytes[index + 2]).is_some() =>
            {
                out.push(
                    hex(bytes[index + 1]).unwrap_or(0) * 16 + hex(bytes[index + 2]).unwrap_or(0),
                );
                index += 2;
            }
            other => out.push(other),
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

#[cfg(test)]
mod tests {
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    use super::*;

    /// A listener that answers one request with `response` and sends the request line back through the channel.
    async fn serve(response: String) -> (String, tokio::sync::oneshot::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let address = listener.local_addr().expect("address").to_string();
        let (sender, receiver) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).await.expect("read");
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            drop(sender.send(request.lines().next().unwrap_or("").to_owned()));
            drop(stream.write_all(response.as_bytes()).await);
        });
        (address, receiver)
    }

    fn redirect(location: &str) -> String {
        format!(
            "HTTP/1.1 302 Found\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )
    }

    #[tokio::test]
    async fn redirect_gives_code_and_state_and_the_subject_goes_as_login_hint() {
        // Arrange
        let (address, request) = serve(redirect("http://app/cb?code=abc_1&state=s%2Ft+u")).await;
        let url = format!("http://{address}/authorize?client_id=c&state=s");

        // Act
        let params = HttpProviderLogin
            .authorize(&url, "dev-seed-admin")
            .await
            .expect("login");

        // Assert
        assert_eq!(
            params.0,
            vec![
                ("code".to_owned(), "abc_1".to_owned()),
                ("state".to_owned(), "s/t u".to_owned())
            ]
        );
        assert_eq!(
            request.await.expect("request line"),
            "GET /authorize?client_id=c&state=s&login_hint=dev-seed-admin HTTP/1.1"
        );
    }

    #[tokio::test]
    async fn an_answer_that_is_not_a_redirect_is_named() {
        // Arrange
        let (address, _request) =
            serve("HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n".to_owned()).await;

        // Act
        let result = HttpProviderLogin
            .authorize(
                &format!("http://{address}/authorize?x=1"),
                "dev-seed-user-1",
            )
            .await;

        // Assert
        let error = result.expect_err("a 400 is refused");
        assert!(error.to_string().contains("400"), "{error}");
    }

    #[tokio::test]
    async fn a_redirect_without_a_code_is_refused() {
        // Arrange
        let (address, _request) =
            serve(redirect("http://app/cb?error=access_denied&state=s")).await;

        // Act
        let result = HttpProviderLogin
            .authorize(
                &format!("http://{address}/authorize?x=1"),
                "dev-seed-user-1",
            )
            .await;

        // Assert
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn https_and_odd_subjects_are_refused_before_any_connection() {
        // Arrange and Act
        let https = HttpProviderLogin
            .authorize("https://idp.example/authorize", "a")
            .await;
        let odd = HttpProviderLogin
            .authorize("http://127.0.0.1:1/a", "a&b=c")
            .await;

        // Assert
        assert!(https.expect_err("https").to_string().contains("plain http"));
        assert!(
            odd.expect_err("odd subject")
                .to_string()
                .contains("plain name")
        );
    }

    #[test]
    fn a_malformed_escape_stays_as_written() {
        // Arrange
        let texts = ["a%zzb", "100%", "x%4"];

        // Act
        let decoded = texts.map(decode);

        // Assert
        assert_eq!(decoded, ["a%zzb", "100%", "x%4"]);
    }
}
