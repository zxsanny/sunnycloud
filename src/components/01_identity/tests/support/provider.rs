//! An identity provider that runs in this process on a local port: OIDC discovery, JWKS, token endpoint with
//! ES256-signed id tokens, and the GitHub endpoints (`/login/oauth/access_token`, `/user`, `/user/emails`).
//! Under `/apple` it answers as Sign in with Apple does: no PKCE, a client-secret JWT checked by a hook the test
//! installs, `email_verified` as text. It stands in for the external service (mock-idp, E-31/c13); the client under test talks to it over a real
//! socket and validates its answers for real. The "browser" step is [`FakeProvider::authorize`]: it reads the
//! authorization URL the service produced and mints the one-time code the callback carries.

#[path = "mini_http.rs"]
mod mini_http;

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
use tokio::net::{TcpListener, TcpStream};
use tokio::task::{JoinHandle, JoinSet};

use mini_http::{Request, Response, read_request, url_form, write_response};

const KEY_ID: &str = "fake-key-1";
const ID_TOKEN_LIFETIME_SECS: i64 = 3600;
const STALL: Duration = Duration::from_secs(3600);
const APPLE: &str = "/apple";

/// Judges the `client_secret` of an Apple token request: `Err` is the refusal's reason. Installed by the test.
pub type ClientSecretCheck = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// What the provider does differently from a healthy one; each field is off by default.
#[derive(Clone, Default)]
pub struct Behaviour {
    /// The token endpoint accepts the connection and never answers.
    pub stall_token_endpoint: bool,
    /// `token_endpoint` and `jwks_uri` as the discovery document advertises them.
    pub advertised_token_endpoint: Option<String>,
    pub advertised_jwks_uri: Option<String>,
    /// The key set endpoint answers 302 to this location.
    pub jwks_redirect: Option<String>,
    /// The id tokens are signed, then their signature is altered.
    pub corrupt_id_token_signature: bool,
    /// The token endpoint answers 302 to this location.
    pub token_redirect: Option<String>,
    /// GitHub's `/user/emails`: `(address, primary, verified)`.
    pub github_emails: Vec<(String, bool, bool)>,
    /// GitHub's `/user` answers `"name": null`, leaving only the `login`.
    pub github_no_name: bool,
}

/// The values behind one authorization code; the fields can be changed before the callback.
#[derive(Clone)]
pub struct Grant {
    pub subject: String,
    pub nonce: String,
    pub audience: String,
    pub issuer: String,
    pub email: Option<(String, bool)>,
    /// The id token's `name` and `preferred_username` claims; absent by default.
    pub name: Option<String>,
    pub preferred_username: Option<String>,
    /// `None` for an Apple grant: its authorization URL carries no PKCE challenge.
    code_challenge: Option<String>,
}

struct Inner {
    grants: HashMap<String, Grant>,
    access_tokens: HashMap<String, String>,
    behaviour: Behaviour,
    hits: Vec<String>,
    counter: u64,
    client_secret_check: Option<ClientSecretCheck>,
    client_secrets: Vec<String>,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Github,
    Oidc,
    Apple,
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
                client_secret_check: None,
                client_secrets: Vec::new(),
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

    /// Apple's token endpoint refuses a `client_secret` the check refuses; every secret it receives is recorded.
    pub fn check_client_secrets(&self, check: ClientSecretCheck) {
        self.lock().client_secret_check = Some(check);
    }

    /// The `client_secret` of every Apple token request so far, in order.
    pub fn client_secrets(&self) -> Vec<String> {
        self.lock().client_secrets.clone()
    }

    /// Where the Apple provider's issuer is (its discovery document and endpoints live under it).
    pub fn apple_issuer(&self) -> String {
        format!("{}{APPLE}", self.shared.base)
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
        let apple = url.path().starts_with(APPLE);
        let code_challenge = if apple {
            assert!(!query.contains_key("code_challenge"), "{authorization_url}");
            None
        } else {
            Some(
                query
                    .get("code_challenge")
                    .cloned()
                    .expect("PKCE challenge in the url"),
            )
        };
        let mut grant = Grant {
            subject: subject.to_owned(),
            nonce: query.get("nonce").cloned().unwrap_or_default(),
            audience: query
                .get("client_id")
                .cloned()
                .expect("client_id in the url"),
            issuer: if apple {
                self.apple_issuer()
            } else {
                self.shared.base.clone()
            },
            email: None,
            name: None,
            preferred_username: None,
            code_challenge,
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
    write_response(&mut stream, &response).await;
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
        let (apple, path) = match request.path.strip_prefix(APPLE) {
            Some(rest) if rest.starts_with('/') => (true, rest),
            _ => (false, request.path.as_str()),
        };
        match (request.method.as_str(), path) {
            ("GET", "/.well-known/openid-configuration") => self.discovery(&behaviour, apple),
            ("GET", "/jwks") => self.jwks(&behaviour),
            ("POST", "/token") => {
                let kind = if apple { Kind::Apple } else { Kind::Oidc };
                self.token(&request, &behaviour, kind).await
            }
            ("POST" | "GET", "/login/oauth/access_token") if !apple => {
                self.token(&request, &behaviour, Kind::Github).await
            }
            ("GET", "/user") => self.github_user(&request, &behaviour),
            ("GET", "/user/emails") => self.github_emails(&behaviour),
            _ => Response::json(404, &json!({"error": "not_found"})),
        }
    }

    fn discovery(&self, behaviour: &Behaviour, apple: bool) -> Response {
        let base = &if apple {
            format!("{}{APPLE}", self.base)
        } else {
            self.base.clone()
        };
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

    fn jwks(&self, behaviour: &Behaviour) -> Response {
        if let Some(location) = &behaviour.jwks_redirect {
            return Response {
                status: 302,
                headers: vec![("Location", location.clone())],
                body: Vec::new(),
            };
        }
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

    async fn token(&self, request: &Request, behaviour: &Behaviour, kind: Kind) -> Response {
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
        if kind == Kind::Apple {
            let secret = form.get("client_secret").map_or("", String::as_str);
            inner.client_secrets.push(secret.to_owned());
            let verdict = inner
                .client_secret_check
                .as_ref()
                .map_or(Ok(()), |check| check(secret));
            if verdict.is_err() {
                return Response::json(400, &json!({"error": "invalid_client"}));
            }
        }
        let Some(grant) = form
            .get("code")
            .and_then(|code| inner.grants.remove(code.as_str()))
        else {
            return invalid_grant();
        };
        if let Some(expected) = &grant.code_challenge {
            let verifier = form.get("code_verifier").map_or("", String::as_str);
            let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, verifier.as_bytes()));
            if &challenge != expected {
                return invalid_grant();
            }
        }
        let access_token = format!("access-{}", grant.subject);
        inner
            .access_tokens
            .insert(access_token.clone(), grant.subject.clone());
        let mut body = json!({"access_token": access_token, "token_type": "bearer"});
        if kind != Kind::Github {
            body["token_type"] = json!("Bearer");
            body["expires_in"] = json!(ID_TOKEN_LIFETIME_SECS);
            body["id_token"] = json!(self.id_token(&grant, kind == Kind::Apple, behaviour));
        }
        Response::json(200, &body)
    }

    fn id_token(&self, grant: &Grant, apple: bool, behaviour: &Behaviour) -> String {
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
            claims["email_verified"] = if apple {
                json!(verified.to_string())
            } else {
                json!(verified)
            };
        }
        if let Some(name) = &grant.name {
            claims["name"] = json!(name);
        }
        if let Some(username) = &grant.preferred_username {
            claims["preferred_username"] = json!(username);
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
        let mut bytes = signature.as_ref().to_vec();
        if behaviour.corrupt_id_token_signature {
            bytes[0] ^= 0xff;
        }
        format!("{signing_input}.{}", URL_SAFE_NO_PAD.encode(bytes))
    }

    fn github_user(&self, request: &Request, behaviour: &Behaviour) -> Response {
        let subject = request
            .headers
            .get("authorization")
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|token| self.lock().access_tokens.get(token).cloned());
        match subject.and_then(|subject| subject.parse::<u64>().ok()) {
            Some(id) => {
                let name = (!behaviour.github_no_name).then_some("Octo Cat");
                Response::json(200, &json!({"id": id, "login": "octo", "name": name}))
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
