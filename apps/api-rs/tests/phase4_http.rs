#![cfg(feature = "duckdb-store")]

use std::{fs, sync::Arc};

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use duckdb::Connection;
use recitopia_api_rs::{
    AppState,
    assets::AssetManager,
    config::{AssetConfig, DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    router,
    store::{ReadStore, WriteStore},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");

fn fixture_app() -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let database_path = directory.path().join("phase4-http.duckdb");
    {
        let connection = Connection::open(&database_path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 4 fixture");
    }
    let converter = directory.path().join("convert-stub.sh");
    fs::write(
        &converter,
        "#!/bin/sh\nsrc=\"$1\"\nfor dst in \"$@\"; do :; done\ncp \"$src\" \"$dst\"\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&converter, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let store = Arc::new(
        DuckStore::open(&DatabaseConfig {
            path: database_path,
            mode: StoreMode::ReadWrite,
        })
        .expect("open fixture read-write"),
    );
    let read_store: Arc<dyn ReadStore> = store.clone();
    let write_store: Arc<dyn WriteStore> = store;
    let assets = Arc::new(AssetManager::new(AssetConfig {
        import_dir: directory.path().join("imports"),
        image_convert_bin: Some(converter),
    }));
    (
        directory,
        router(AppState::with_write_store(read_store, write_store).with_assets(assets)),
    )
}

fn tar_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut bytes);
        for (path, contents) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(u64::try_from(contents.len()).unwrap());
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, path, *contents)
                .expect("append tar image");
        }
        builder.finish().expect("finish tar");
    }
    bytes
}

async fn send(
    app: &Router,
    method: Method,
    uri: &str,
    body: Vec<u8>,
    content_type: Option<&str>,
    extra_headers: &[(&str, &str)],
) -> Response<Body> {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(content_type) = content_type {
        request = request.header(header::CONTENT_TYPE, content_type);
    }
    for (name, value) in extra_headers {
        request = request.header(*name, *value);
    }
    app.clone()
        .oneshot(request.body(Body::from(body)).expect("request"))
        .await
        .expect("response")
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("response JSON")
}

#[tokio::test]
async fn archive_review_and_avif_routes_round_trip() {
    let (_directory, app) = fixture_app();
    let archive = tar_bytes(&[("001.jpg", b"first-image"), ("002.png", b"second-image")]);
    let content_length = archive.len().to_string();
    let response = send(
        &app,
        Method::POST,
        "/api/cookbook-imports/archive?cookbookId=our-korean-kitchen&sourcePath=fixture-book",
        archive.clone(),
        Some("application/x-tar"),
        &[("content-length", &content_length)],
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let summary = json_body(response).await;
    assert_eq!(summary["pageCount"], 2);
    assert_eq!(summary["importRecord"]["status"], "uploaded");
    let import_id = summary["importRecord"]["id"].as_str().expect("import id");
    let page_id = format!("{import_id}-page-1");

    let original = send(
        &app,
        Method::GET,
        &format!("/api/cookbook-pages/{page_id}/image"),
        Vec::new(),
        None,
        &[],
    )
    .await;
    assert_eq!(original.status(), StatusCode::OK);
    assert_eq!(original.headers()[header::CONTENT_TYPE], "image/jpeg");

    let avif = send(
        &app,
        Method::GET,
        &format!("/api/cookbook-pages/{page_id}/image"),
        Vec::new(),
        None,
        &[("accept", "image/avif")],
    )
    .await;
    assert_eq!(avif.status(), StatusCode::OK);
    assert_eq!(avif.headers()[header::CONTENT_TYPE], "image/avif");
    let derivative_etag = avif.headers()[header::ETAG].to_str().unwrap().to_owned();
    assert!(derivative_etag.ends_with("-avif\""));
    let not_modified = send(
        &app,
        Method::GET,
        &format!("/api/cookbook-pages/{page_id}/image"),
        Vec::new(),
        None,
        &[
            ("accept", "image/avif"),
            ("if-none-match", &derivative_etag),
        ],
    )
    .await;
    assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);

    let patch = serde_json::to_vec(&json!({
        "ocrText": "Corrected introduction text",
        "pageKind": "essay",
        "reviewStatus": "needs_ocr_fix"
    }))
    .unwrap();
    let patched = send(
        &app,
        Method::PATCH,
        &format!("/api/cookbook-pages/{page_id}"),
        patch,
        Some("application/json"),
        &[],
    )
    .await;
    assert_eq!(patched.status(), StatusCode::OK);
    assert_eq!(
        json_body(patched).await["ocrText"],
        "Corrected introduction text"
    );

    let accepted = send(
        &app,
        Method::POST,
        &format!("/api/cookbook-pages/{page_id}/accept-content"),
        serde_json::to_vec(&json!({"kind": "callout", "title": "Introduction"})).unwrap(),
        Some("application/json"),
        &[],
    )
    .await;
    assert_eq!(accepted.status(), StatusCode::OK);
    let accepted = json_body(accepted).await;
    let block_id = accepted["id"].as_str().expect("accepted block id");
    assert_eq!(accepted["text"], "Corrected introduction text");

    let block_patch = send(
        &app,
        Method::PATCH,
        &format!("/api/cookbook-content-blocks/{block_id}"),
        serde_json::to_vec(&json!({"text": "Edited document text", "title": ""})).unwrap(),
        Some("application/json"),
        &[],
    )
    .await;
    assert_eq!(block_patch.status(), StatusCode::OK);
    let block_patch = json_body(block_patch).await;
    assert_eq!(block_patch["text"], "Edited document text");
    assert_eq!(block_patch["title"], Value::Null);

    let repeated = send(
        &app,
        Method::POST,
        &format!("/api/cookbook-pages/{page_id}/accept-content"),
        Vec::new(),
        None,
        &[],
    )
    .await;
    assert_eq!(repeated.status(), StatusCode::CONFLICT);

    let duplicate = send(
        &app,
        Method::POST,
        "/api/cookbook-imports/archive?cookbookId=our-korean-kitchen&sourcePath=duplicate",
        archive,
        Some("application/x-tar"),
        &[("content-length", &content_length)],
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn image_upload_and_archive_failures_are_bounded_json() {
    let (_directory, app) = fixture_app();
    let upload = send(
        &app,
        Method::POST,
        "/api/cookbook-page-images",
        serde_json::to_vec(&json!({
            "fileName": "page.png",
            "mimeType": "image/png",
            "imageBase64": "data:image/png;base64,cGFnZS1ieXRlcw=="
        }))
        .unwrap(),
        Some("application/json"),
        &[],
    )
    .await;
    assert_eq!(upload.status(), StatusCode::OK);
    let upload = json_body(upload).await;
    assert_eq!(upload["sizeBytes"], 10);
    assert!(
        std::path::Path::new(upload["imagePath"].as_str().unwrap())
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    );

    let malformed = b"not a tar".to_vec();
    let malformed_length = malformed.len().to_string();
    let malformed = send(
        &app,
        Method::POST,
        "/api/cookbook-imports/archive?cookbookId=our-korean-kitchen&sourcePath=malformed",
        malformed,
        Some("application/x-tar"),
        &[("content-length", &malformed_length)],
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(malformed).await,
        json!({"error": "could not inspect archive"})
    );

    let missing_length = send(
        &app,
        Method::POST,
        "/api/cookbook-imports/archive?cookbookId=our-korean-kitchen&sourcePath=no-length",
        Vec::new(),
        Some("application/x-tar"),
        &[],
    )
    .await;
    assert_eq!(missing_length.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(missing_length).await,
        json!({"error": "archive content length is required"})
    );
}
