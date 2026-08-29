#![cfg(feature = "duckdb-store")]

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use duckdb::Connection;
use recitopia_api_rs::{
    AppState,
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    model::{
        CookbookImportProgress, ImportJobState, ImportPipelineStage, RecipeImport,
        RecipeImportStatus,
    },
    router,
    store::{ReadStore, WriteStore},
};
use serde_json::Value;
use tower::ServiceExt;

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");
const NOW: &str = "2026-07-10T17:00:00.000Z";

fn fixture_app() -> (tempfile::TempDir, Arc<DuckStore>, Router) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase5-http.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 5 fixture");
    }
    let store = Arc::new(
        DuckStore::open(&DatabaseConfig {
            path,
            mode: StoreMode::ReadWrite,
        })
        .expect("open fixture read-write"),
    );
    let read_store: Arc<dyn ReadStore> = store.clone();
    let write_store: Arc<dyn WriteStore> = store.clone();
    let app = router(AppState::with_write_store(read_store, write_store));
    (directory, store, app)
}

async fn request(app: &Router, method: Method, uri: &str, body: Body) -> (StatusCode, Value) {
    let request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(body)
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap();
    (status, value)
}

#[tokio::test]
async fn persisted_progress_can_be_polled_and_canceled_without_a_browser_connection() {
    let (_directory, store, app) = fixture_app();
    let mut progress = CookbookImportProgress::queued("import-1");
    progress.stage = ImportPipelineStage::DeepseekSection;
    progress.current_section_index = Some(2);
    progress.section_total = Some(8);
    store
        .upsert_cookbook_import_progress(progress, NOW)
        .expect("persist fixture progress");

    let (status, running) = request(
        &app,
        Method::GET,
        "/api/cookbook-imports/import-1/progress",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(running["state"], "running");
    assert_eq!(running["currentSectionIndex"], 2);

    let (status, canceled) = request(
        &app,
        Method::POST,
        "/api/cookbook-imports/import-1/cancel",
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(canceled["state"], "canceled");
    assert_eq!(
        store
            .cookbook_import_progress("import-1")
            .expect("reload canceled progress")
            .state,
        ImportJobState::Canceled
    );
}

#[tokio::test]
async fn editable_recipe_import_drafts_commit_through_the_http_contract() {
    let (_directory, store, app) = fixture_app();
    let mut recipe = store.catalogue_summary().unwrap().recipes.remove(0);
    recipe.id = "phase-five-http-recipe".to_owned();
    recipe.last_made_at = None;
    recipe.times_made = 0;
    store
        .create_recipe_import(RecipeImport {
            id: "recipe-import-http".to_owned(),
            status: RecipeImportStatus::DraftReady,
            file_name: "fixture.txt".to_owned(),
            mime_type: "text/plain".to_owned(),
            image_path: "fixture:text".to_owned(),
            ocr_engine: "fixture".to_owned(),
            ocr_text: "fixture OCR".to_owned(),
            ocr_json: "{}".to_owned(),
            draft: Some(recipe.clone()),
            validation_issues: Vec::new(),
            created_at: NOW.to_owned(),
            updated_at: NOW.to_owned(),
        })
        .unwrap();

    recipe.title = "Edited Phase Five Recipe".to_owned();
    let body = Body::from(serde_json::to_vec(&recipe).unwrap());
    let (status, updated) = request(
        &app,
        Method::PUT,
        "/api/imports/recipe-import-http/draft",
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["draft"]["title"], "Edited Phase Five Recipe");

    let body = Body::from(serde_json::to_vec(&recipe).unwrap());
    let (status, committed) = request(
        &app,
        Method::POST,
        "/api/imports/recipe-import-http/commit",
        body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(committed["id"], "phase-five-http-recipe");
    assert_eq!(
        store.recipe_import("recipe-import-http").unwrap().status,
        RecipeImportStatus::Committed
    );
}
