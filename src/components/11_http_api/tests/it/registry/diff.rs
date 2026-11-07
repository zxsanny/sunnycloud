use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::{Value, json};
use sunnycloud_http_api::{Breaking, current_document, diff_documents};

const RELEASE: &str = include_str!("../../fixtures/openapi_release.json");

fn release() -> Value {
    serde_json::from_str(RELEASE).expect("the release fixture is JSON")
}

fn mutated(edit: impl FnOnce(&mut Value)) -> Value {
    let mut document = release();
    edit(&mut document);
    document
}

fn reasons(old: &Value, new: &Value) -> Vec<String> {
    diff_documents(old, new)
        .into_iter()
        .map(|b| b.to_string())
        .collect()
}

fn has(found: &[String], text: &str) -> bool {
    found.iter().any(|reason| reason.contains(text))
}

fn schema<'a>(document: &'a mut Value, name: &str) -> &'a mut Value {
    &mut document["components"]["schemas"][name]
}

#[test]
fn a_document_has_no_breaking_change_against_itself() {
    // Arrange
    let document = release();

    // Act
    let found = diff_documents(&document, &document);

    // Assert
    assert_eq!(found, Vec::<Breaking>::new());
    let current: Value = serde_json::from_slice(current_document().as_bytes()).expect("document");
    assert_eq!(diff_documents(&current, &current), Vec::<Breaking>::new());
}

#[test]
fn a_removed_operation_fails() {
    // Arrange
    let new = mutated(|d| {
        d["paths"]
            .as_object_mut()
            .expect("paths")
            .remove("/api/v1/nodes/{id}");
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert_eq!(found, ["GET /api/v1/nodes/{id}: operation removed"]);
}

#[test]
fn a_required_request_field_added_fails() {
    // Arrange
    let new = mutated(|d| {
        let create = schema(d, "NodeCreate");
        create["properties"]["mime"] = json!({"type": "string"});
        create["required"] = json!(["name", "parent_id", "mime"]);
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        has(&found, "request field mime became required"),
        "{found:?}"
    );
}

#[test]
fn an_enum_value_removed_fails() {
    // Arrange
    let new = mutated(|d| {
        schema(d, "NodeView")["properties"]["kind"]["enum"] = json!(["file", "folder"])
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert!(has(&found, "enum value \"symlink\" removed"), "{found:?}");
    assert!(has(&found, "POST /api/v1/nodes"), "{found:?}");
}

#[test]
fn a_response_field_removed_fails() {
    // Arrange
    let new = mutated(|d| {
        schema(d, "NodeView")["properties"]
            .as_object_mut()
            .expect("properties")
            .remove("size");
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert!(has(&found, "field size removed"), "{found:?}");
    assert!(has(&found, "GET /api/v1/nodes/{id}"), "{found:?}");
}

#[test]
fn a_new_optional_field_passes() {
    // Arrange
    let new = mutated(|d| {
        schema(d, "NodeCreate")["properties"]["note"] = json!({"type": "string"});
        schema(d, "NodeView")["properties"]["note"] = json!({"type": "string"});
        schema(d, "NodeView")["properties"]["kind"]["enum"] =
            json!(["file", "folder", "symlink", "shortcut"]);
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_new_operation_passes() {
    // Arrange
    let new = mutated(|d| {
        d["paths"]["/api/v1/devices"] = json!({"get": {
            "operationId": "get_devices",
            "x-auth-class": "SessionOnly",
            "x-body-class": "None",
            "responses": {"200": {"description": "Success"}}
        }});
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_tightened_auth_class_fails_and_a_loosened_one_passes() {
    // Arrange
    let set_auth = |class: &'static str| {
        mutated(move |d| d["paths"]["/api/v1/nodes/{id}"]["get"]["x-auth-class"] = class.into())
    };
    let cases = [
        ("SessionOnly", true),
        ("Admin", true),
        ("Worker", true),
        ("Public", false),
        ("SessionOrDevice", false),
    ];

    for (class, breaks) in cases {
        // Act
        let found = reasons(&release(), &set_auth(class));

        // Assert
        assert_eq!(
            !found.is_empty(),
            breaks,
            "SessionOrDevice -> {class}: {found:?}"
        );
    }
    let session_only =
        mutated(|d| d["paths"]["/api/v1/nodes/{id}"]["get"]["x-auth-class"] = "SessionOnly".into());
    let loosened = reasons(&session_only, &release());
    assert_eq!(loosened, Vec::<String>::new());
}

#[test]
fn a_tightened_body_class_fails_and_a_looser_one_passes() {
    // Arrange
    let set_body = |class: &'static str| {
        mutated(move |d| d["paths"]["/api/v1/nodes"]["post"]["x-body-class"] = class.into())
    };

    // Act
    let tighter = reasons(&release(), &set_body("Auth"));
    let looser = reasons(&release(), &set_body("Commit"));

    // Assert
    assert!(has(&tighter, "body class tightened"), "{tighter:?}");
    assert_eq!(looser, Vec::<String>::new());
}

#[test]
fn a_new_required_parameter_or_a_retyped_field_fails() {
    // Arrange
    let required = mutated(|d| {
        d["paths"]["/api/v1/changes"]["get"]["parameters"]
            .as_array_mut()
            .expect("parameters")
            .push(json!({"name": "wait", "in": "query", "required": true, "schema": {"type": "integer"}}));
    });
    let optional = mutated(|d| {
        d["paths"]["/api/v1/changes"]["get"]["parameters"]
            .as_array_mut()
            .expect("parameters")
            .push(json!({"name": "wait", "in": "query", "required": false, "schema": {"type": "integer"}}));
    });
    let retyped =
        mutated(|d| schema(d, "NodeView")["properties"]["size"]["type"] = "string".into());

    // Act
    let (required, optional, retyped) = (
        reasons(&release(), &required),
        reasons(&release(), &optional),
        reasons(&release(), &retyped),
    );

    // Assert
    assert!(
        has(&required, "new required query parameter wait"),
        "{required:?}"
    );
    assert_eq!(optional, Vec::<String>::new());
    assert!(
        has(&retyped, "type changed from integer to string"),
        "{retyped:?}"
    );
}

#[test]
fn a_field_removed_inside_a_nested_array_item_or_a_removed_success_response_fails() {
    // Arrange
    let nested = mutated(|d| {
        schema(d, "NodeView")["properties"]["labels"]["items"]["properties"]
            .as_object_mut()
            .expect("properties")
            .remove("text");
    });
    let no_success = mutated(|d| {
        d["paths"]["/api/v1/nodes"]["post"]["responses"]
            .as_object_mut()
            .expect("responses")
            .remove("201");
    });

    // Act
    let (nested, no_success) = (
        reasons(&release(), &nested),
        reasons(&release(), &no_success),
    );

    // Assert
    assert!(
        has(&nested, "labels[].text") || has(&nested, "field text removed"),
        "{nested:?}"
    );
    assert!(has(&no_success, "response 201 removed"), "{no_success:?}");
}

#[test]
fn a_schema_that_refers_to_itself_is_compared_once() {
    // Arrange
    let new = mutated(|d| {
        schema(d, "NodeView")["properties"]["parent"]
            .as_object_mut()
            .expect("parent")
            .insert("description".into(), "the folder above".into());
    });

    // Act
    let found = reasons(&release(), &new);

    // Assert
    assert_eq!(found, Vec::<String>::new());
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_openapi_diff"))
        .args(args)
        .output()
        .expect("the diff tool starts")
}

fn write(name: &str, document: &Value) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("openapi_diff");
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_vec(document).expect("serialise")).expect("write fixture");
    path
}

#[test]
fn the_tool_exits_0_on_additive_changes_1_on_breaking_ones_and_2_on_bad_input() {
    // Arrange
    let old = write("old.json", &release());
    let additive = write(
        "additive.json",
        &mutated(|d| {
            d["paths"]["/api/v1/devices"] = json!({"get": {"x-auth-class": "SessionOnly", "x-body-class": "None", "responses": {}}});
        }),
    );
    let removed = write(
        "removed.json",
        &mutated(|d| {
            d["paths"]
                .as_object_mut()
                .expect("paths")
                .remove("/api/v1/changes");
        }),
    );
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let (old_path, additive_path, removed_path) = (
        old.to_str().expect("utf8"),
        additive.to_str().expect("utf8"),
        removed.to_str().expect("utf8"),
    );

    // Act
    let passes = run(&["diff", old_path, additive_path]);
    let fails = run(&["diff", old_path, removed_path]);
    let unreadable = run(&["diff", old_path, "/nonexistent/openapi.json"]);
    let usage = run(&["diff"]);

    // Assert
    assert_eq!(passes.status.code(), Some(0), "{}", text(&passes.stderr));
    assert_eq!(fails.status.code(), Some(1));
    assert!(text(&fails.stdout).contains("GET /api/v1/changes: operation removed"));
    assert_eq!(unreadable.status.code(), Some(2));
    assert!(text(&unreadable.stderr).contains("cannot read /nonexistent/openapi.json"));
    assert_eq!(usage.status.code(), Some(2));
}

#[test]
fn the_tool_prints_the_document_of_this_build_in_every_process() {
    // Arrange
    let expected = current_document();

    // Act
    let first = run(&["print"]);
    let second = run(&["print"]);

    // Assert
    assert_eq!(first.status.code(), Some(0));
    let mut printed = first.stdout.clone();
    assert_eq!(printed.pop(), Some(b'\n'));
    assert_eq!(printed, expected.as_bytes());
    assert_eq!(first.stdout, second.stdout);
}
