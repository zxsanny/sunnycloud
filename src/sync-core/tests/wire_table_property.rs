//! AC-5 of AZ-3958: the mapping is total and no `Display` or `Debug` text carries a planted secret.

use sc_error::ErrorCode;
use serde_json::{Value, json};
use sync_core::{Mapped, map_problem};

const SECRETS: [&str; 4] = [
    "Bearer sk-live-PLANTED",
    "holiday-photos.jpg",
    "/home/alice/secret/notes.txt",
    "C:\\Users\\alice\\payroll.xlsx",
];

struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n).expect("small bound")).expect("fits usize")
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

fn assert_no_secret(text: &str, context: &str) {
    for secret in SECRETS {
        assert!(
            !text.contains(secret),
            "{context}: leaked {secret} in {text}"
        );
    }
}

fn assert_clean(mapped: &Mapped, context: &str) {
    assert_no_secret(&format!("{mapped:?}"), context);
    if let Mapped::Error(error) = mapped {
        assert_no_secret(&error.to_string(), context);
    }
}

fn status_for(rng: &mut XorShift) -> u16 {
    *rng.pick(&[
        0, 99, 200, 400, 401, 404, 409, 410, 413, 422, 426, 429, 500, 503, 507, 999,
    ])
}

#[test]
fn random_bytes_never_panic_and_stay_clean() {
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15);
    for iteration in 0..5000 {
        let len = rng.below(160);
        let body: Vec<u8> = (0..len).map(|_| rng.next().to_le_bytes()[0]).collect();
        let header = rng.pick(&SECRETS);
        let headers = [("x-request-id", *header), ("retry-after", *header)];

        let mapped = map_problem(status_for(&mut rng), &headers, &body);

        assert_clean(&mapped, &format!("random bytes, iteration {iteration}"));
    }
}

fn planted_body(rng: &mut XorShift) -> Vec<u8> {
    let mut codes: Vec<&str> = ErrorCode::ALL.iter().map(|code| code.as_str()).collect();
    codes.extend(["weird_code", "", "UPPER", "a b", "x/y"]);
    let code = *rng.pick(&codes);
    let secret = *rng.pick(&SECRETS);
    let mut body = json!({
        "type": format!("urn:sunnycloud:error:{secret}"),
        "title": if code == "upgrade_required" { "Upgrade required" } else { secret },
        "status": 400,
        "code": code,
        "request_id": rng.pick(&["r-1", "0192-x", secret]),
        "retry_after": rng.pick(&[json!(3), json!(secret), json!(-1)]).clone(),
        "need_blocks": rng.pick(&[json!([1, 2]), json!([secret]), json!("no")]).clone(),
        "reason": rng.pick(&[json!("epoch_changed"), json!(secret)]).clone(),
        "min_version": rng.pick(&[json!("1.2.3"), json!(secret)]).clone(),
        "update_url": rng.pick(&[json!("https://sunnycloud.test/get"), json!(secret)]).clone(),
        "problems": [{"field": secret, "problem": secret}],
        "extra": {"path": secret},
    });
    // The 426 `detail` is the user-facing upgrade text, shown on purpose; every other code's detail must stay hidden.
    if code != "upgrade_required" {
        body["detail"] = Value::from(secret);
    }
    serde_json::to_vec(&body).expect("json")
}

#[test]
fn planted_secrets_in_structured_bodies_and_headers_never_surface() {
    let mut rng = XorShift(0xD1B5_4A32_D192_ED03);
    for iteration in 0..5000 {
        let mut body = planted_body(&mut rng);
        if iteration % 3 == 0 && !body.is_empty() {
            let at = rng.below(body.len());
            body[at] = rng.next().to_le_bytes()[0];
        }
        let secret = *rng.pick(&SECRETS);
        let headers = [
            ("Authorization", secret),
            ("Set-Cookie", secret),
            ("Location", secret),
            ("X-Request-Id", secret),
            ("Retry-After", secret),
        ];

        let mapped = map_problem(status_for(&mut rng), &headers, &body);

        assert_clean(&mapped, &format!("structured body, iteration {iteration}"));
    }
}
