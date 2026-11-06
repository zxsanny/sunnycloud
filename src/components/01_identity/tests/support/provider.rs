//! An identity provider that runs in this process on a local port: OIDC discovery, JWKS, token endpoint with
//! ES256-signed id tokens, and the GitHub endpoints (`/login/oauth/access_token`, `/user`, `/user/emails`).
//! It stands in for the external service (mock-idp, E-31/c13); the client under test talks to it over a real
//! socket and validates its answers for real. The "browser" step is [`FakeProvider::authorize`]: it reads the
//! authorization URL the service produced and mints the one-time code the callback carries.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use reqwest::Url;
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

const KEY_ID: &str = "fake-key-1";
const ID_TOKEN_LIFETIME_SECS: i64 = 3600;
const STALL: Duration = Duration::from_secs(3600);

/// What the provider does differently from a healthy one; each field is off by default.
#[derive(Clone, Default)]
pub struct Behaviour {
    /// The token endpoint accepts the connection and never answers.
    pub stall_token_endpoint: bool,
    /// `token_endpoint` and `jwks_uri` as the discovery document advertises them.
    pub advertised_token_endpoint: Option<String>,
    pub advertised_jwks_uri: Option<String>,
    /// The token endpoint answers 302 to this location.
    pub token_redirect: Option<String>,
    /// GitHub's `/user/emails`: `(address, primary, verified)`.
    pub github_emails: Vec<(String, bool, bool)>,
}

/// The values behind one authorization code; the fields can be changed before the callback.
#[derive(Clone)]
pub struct Grant {
    pub subject: String,
    pub nonce: String,
    pub audience: String,
    pub issuer: String,
    pub email: Option<(String, bool)>,
    code_challenge: String,
}

struct Inner {
    grants: HashMap<String, Grant>,
    access_tokens: HashMap<String, String>,
    behaviour: Behaviour,
    hits: Vec<String>,
    counter: u64,
}

struct Shared {
    base: String,
    now_secs: i64,
    key: EcdsaKeyPair,
    inner: Mutex<Inner>,
}

pub struct FakeProvider {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
}

struct Request {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct Response {
    status: u16,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, body: &Value) -> Response {
        Response {
            status,
            headers: vec![("Content-Type", "application/json".to_owned())],
            body: body.to_string().into_bytes(),
        }
    }
}

impl FakeProvider {
    /// Listens on an OS-assigned port. `now_secs` is the clock the id tokens are issued against.
    pub async fn start(now_secs: i64) -> FakeProvider {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind the fake provider");
        let addr = listener.local_addr().expect("fake provider address");
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
            .expect("generate the signing key");
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .expect("load the signing key");
        let shared = Arc::new(Shared {
            base: format!("http://{addr}"),
            now_secs,
            key,
            inner: Mutex::new(Inner {
                grants: HashMap::new(),
                access_tokens: HashMap::new(),
                behaviour: Behaviour::default(),
                hits: Vec::new(),
                counter: 0,
            }),
        });
        let task = tokio::spawn(accept_loop(listener, Arc::clone(&shared)));
        FakeProvider { shared, task }
    }

    pub fn base_url(&self) -> String {
        self.shared.base.clone()
    }

    pub fn address(&self) -> SocketAddr {
        self.shared
            .base
            .trim_start_matches("http://")
            .parse()
            .expect("fake provider address")
    }

    pub fn behave(&self, change: impl FnOnce(&mut Behaviour)) {
        change(&mut self.lock().behaviour);
    }

    /// Requests received so far whose path starts with `prefix` (`""` counts all).
    pub fn hits(&self, prefix: &str) -> usize {
        self.lock()
            .hits
            .iter()
            .filter(|path| path.starts_with(prefix))
            .count()
    }

    /// The user signs in at the provider: reads `authorization_url` (state, nonce, client id, PKCE challenge),
    /// mints a code for `subject` and returns the callback query parameters.
    pub fn authorize(&self, authorization_url: &str, subject: &str) -> Vec<(String, String)> {
        self.authorize_with(authorization_url, subject, |_| {})
    }

    pub fn authorize_with(
        &self,
        authorization_url: &str,
        subject: &str,
        change: impl FnOnce(&mut Grant),
    ) -> Vec<(String, String)> {
        let url = Url::parse(authorization_url).expect("authorization url");
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        let mut grant = Grant {
            subject: subject.to_owned(),
            nonce: query.get("nonce").cloned().unwrap_or_default(),
            audience: query
                .get("client_id")
                .cloned()
                .expect("client_id in the url"),
            issuer: self.shared.base.clone(),
            email: None,
            code_challenge: query
                .get("code_challenge")
                .cloned()
                .expect("PKCE challenge in the url"),
        };
        change(&mut grant);
        let mut inner = self.lock();
        inner.counter += 1;
        let code = format!("code-{}-{}", inner.counter, subject);
        inner.grants.insert(code.clone(), grant);
        vec![
            ("code".to_owned(), code),
            ("state".to_owned(), query["state"].clone()),
        ]
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.shared
            .inner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for FakeProvider {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn accept_loop(listener: TcpListener, shared: Arc<Shared>) {
    let mut connections = JoinSet::new();
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                connections.spawn(connection(stream, Arc::clone(&shared)));
            }
            Err(error) => eprintln!("fake provider: accept failed: {error}"),
        }
    }
}

async fn connection(mut stream: TcpStream, shared: Arc<Shared>) {
    let request = match read_request(&mut stream).await {
        Ok(request) => request,
        Err(error) => return eprintln!("fake provider: unreadable request: {error}"),
    };
    let response = shared.route(request).await;
    let mut head = format!(
        "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.body.len()
    );
    for (name, value) in &response.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    let written = async {
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(&response.body).await?;
        stream.shutdown().await
    }
    .await;
    if let Err(error) = written {
        eprintln!("fake provider: write failed: {error}");
    }
}

async fn read_request(stream: &mut TcpStream) -> std::io::Result<Request> {
    let mut buffer = Vec::new();
    let head_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position;
        }
        let mut chunk = [0u8; 2048];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buffer.extend_from_slice(&chunk[..read]);
    };
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut start = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (
        start.next().unwrap_or_default().to_owned(),
        start.next().unwrap_or_default().to_owned(),
    );
    let headers: HashMap<String, String> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < length {
        let mut chunk = [0u8; 2048];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        body.extend_from_slice(&chunk[..read]);
    }
    let path = target.split('?').next().unwrap_or_default().to_owned();
    Ok(Request {
        method,
        path,
        headers,
        body,
    })
}

impl Shared {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    async fn route(&self, request: Request) -> Response {
        let behaviour = {
            let mut inner = self.lock();
            inner.hits.push(request.path.clone());
            inner.behaviour.clone()
        };
        match (request.method.as_str(), request.path.as_str()) {
            ("GET", "/.well-known/openid-configuration") => self.discovery(&behaviour),
            ("GET", "/jwks") => self.jwks(),
            ("POST", "/token") => self.token(&request, &behaviour, true).await,
            ("POST", "/login/oauth/access_token") => self.token(&request, &behaviour, false).await,
            ("GET", "/user") => self.github_user(&request),
            ("GET", "/user/emails") => self.github_emails(&behaviour),
            _ => Response::json(404, &json!({"error": "not_found"})),
        }
    }

    fn discovery(&self, behaviour: &Behaviour) -> Response {
        let base = &self.base;
        Response::json(
            200,
            &json!({
                "issuer": base,
                "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": behaviour.advertised_token_endpoint.clone()
                    .unwrap_or_else(|| format!("{base}/token")),
                "jwks_uri": behaviour.advertised_jwks_uri.clone()
                    .unwrap_or_else(|| format!("{base}/jwks")),
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["ES256"],
            }),
        )
    }

    fn jwks(&self) -> Response {
        let point = self.key.public_key().as_ref();
        Response::json(
            200,
            &json!({"keys": [{
                "kty": "EC", "crv": "P-256", "use": "sig", "alg": "ES256", "kid": KEY_ID,
                "x": URL_SAFE_NO_PAD.encode(&point[1..33]),
                "y": URL_SAFE_NO_PAD.encode(&point[33..65]),
            }]}),
        )
    }

    async fn token(&self, request: &Request, behaviour: &Behaviour, oidc: bool) -> Response {
        if behaviour.stall_token_endpoint {
            tokio::time::sleep(STALL).await;
        }
        if let Some(location) = &behaviour.token_redirect {
            return Response {
                status: 302,
                headers: vec![("Location", location.clone())],
                body: Vec::new(),
            };
        }
        let form: HashMap<String, String> = url_form(&request.body);
        let invalid_grant = || Response::json(400, &json!({"error": "invalid_grant"}));
        let mut inner = self.lock();
        let Some(grant) = form
            .get("code")
            .and_then(|code| inner.grants.remove(code.as_str()))
        else {
            return invalid_grant();
        };
        let verifier = form.get("code_verifier").map_or("", String::as_str);
        let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()));
        if challenge != grant.code_challenge {
            return invalid_grant();
        }
        let access_token = format!("access-{}", grant.subject);
        inner
            .access_tokens
            .insert(access_token.clone(), grant.subject.clone());
        let mut body = json!({"access_token": access_token, "token_type": "bearer"});
        if oidc {
            body["token_type"] = json!("Bearer");
            body["expires_in"] = json!(ID_TOKEN_LIFETIME_SECS);
            body["id_token"] = json!(self.id_token(&grant));
        }
        Response::json(200, &body)
    }

    fn id_token(&self, grant: &Grant) -> String {
        let header = json!({"alg": "ES256", "typ": "JWT", "kid": KEY_ID});
        let mut claims = json!({
            "iss": grant.issuer,
            "sub": grant.subject,
            "aud": grant.audience,
            "iat": self.now_secs,
            "exp": self.now_secs + ID_TOKEN_LIFETIME_SECS,
            "nonce": grant.nonce,
        });
        if let Some((address, verified)) = &grant.email {
            claims["email"] = json!(address);
            claims["email_verified"] = json!(verified);
        }
        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        let signature = self
            .key
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .expect("sign the id token");
        format!(
            "{signing_input}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        )
    }

    fn github_user(&self, request: &Request) -> Response {
        let subject = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| self.lock().access_tokens.get(token).cloned());
        match subject.and_then(|subject| subject.parse::<u64>().ok()) {
            Some(id) => {
                Response::json(200, &json!({"id": id, "login": "octo", "name": "Octo Cat"}))
            }
            None => Response::json(401, &json!({"message": "Bad credentials"})),
        }
    }

    fn github_emails(&self, behaviour: &Behaviour) -> Response {
        let entries: Vec<Value> = behaviour
            .github_emails
            .iter()
            .map(|(email, primary, verified)| {
                json!({"email": email, "primary": primary, "verified": verified})
            })
            .collect();
        Response::json(200, &Value::Array(entries))
    }
}

fn url_form(body: &[u8]) -> HashMap<String, String> {
    let text = String::from_utf8_lossy(body);
    Url::parse(&format!("http://form.invalid/?{text}"))
        .expect("form body")
        .query_pairs()
        .into_owned()
        .collect()
}
