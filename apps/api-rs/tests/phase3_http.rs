#![cfg(feature = "duckdb-store")]

use std::sync::Arc;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, Response, StatusCode, header},
};
use duckdb::Connection;
use recitopia_api_rs::{
    AppState,
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    router,
    store::{ReadStore, WriteStore},
};
use serde_json::{Value, json};
use tower::ServiceExt;

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");

fn fixture_app() -> (tempfile::TempDir, Router) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase3-http.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 3 fixture");
    }
    let store = Arc::new(
        DuckStore::open(&DatabaseConfig {
            path,
            mode: StoreMode::ReadWrite,
        })
        .expect("open fixture read-write"),
    );
    let read_store: Arc<dyn ReadStore> = store.clone();
    let write_store: Arc<dyn WriteStore> = store;
    (
        directory,
        router(AppState::with_write_store(read_store, write_store)),
    )
}

async fn send(app: &Router, method: Method, uri: &str, payload: Option<Value>) -> Response<Body> {
    let request = Request::builder().method(method).uri(uri);
    let (request, body) = if let Some(payload) = payload {
        (
            request.header(header::CONTENT_TYPE, "application/json"),
            Body::from(serde_json::to_vec(&payload).expect("request JSON")),
        )
    } else {
        (request, Body::empty())
    };
    app.clone()
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("response")
}

async fn json_body(response: Response<Body>) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).expect("response JSON")
}

fn recipe_payload(id: &str, title: &str) -> Value {
    json!({
        "id": id,
        "title": title,
        "cookbookId": "our-korean-kitchen",
        "authorIds": ["author-1"],
        "sourceLabel": "Our Korean Kitchen, p. 40",
        "yieldQuantity": 2,
        "prepMinutes": 5,
        "cookMinutes": 10,
        "tags": ["http-test"],
        "images": [],
        "ingredients": [{
            "id": "http-ingredient",
            "displayName": "2 cups rice",
            "item": "rice",
            "quantity": 2,
            "estimatedCostCents": 100
        }],
        "steps": [{"id": "http-step", "position": 1, "text": "Cook the rice."}],
        "notes": []
    })
}

#[tokio::test]
#[allow(clippy::too_many_lines)] // One end-to-end flow verifies state across every Phase 3 route.
async fn phase_3_http_routes_cover_crud_mark_made_and_error_contracts() {
    let (_directory, app) = fixture_app();

    let pantry = send(
        &app,
        Method::POST,
        "/api/pantry",
        Some(json!({
            "item": "sesame oil",
            "displayName": "Sesame oil",
            "quantity": 1,
            "unit": "bottle",
            "category": "raw"
        })),
    )
    .await;
    assert_eq!(pantry.status(), StatusCode::OK);
    let pantry = json_body(pantry).await;
    let pantry_id = pantry["id"].as_str().expect("pantry id");
    assert_eq!(pantry["ownerUserId"], "avery-river");
    let patched = send(
        &app,
        Method::PATCH,
        &format!("/api/pantry/{pantry_id}"),
        Some(json!({"quantity": 0.5, "notes": "Half full"})),
    )
    .await;
    assert_eq!(patched.status(), StatusCode::OK);
    assert_eq!(json_body(patched).await["notes"], "Half full");

    let meal = send(
        &app,
        Method::POST,
        "/api/meal-plan",
        Some(json!({
            "date": "2026-07-14",
            "mealType": "dinner",
            "recipeId": "recipe-1",
            "servings": 2
        })),
    )
    .await;
    assert_eq!(meal.status(), StatusCode::OK);
    let meal = json_body(meal).await;
    let meal_id = meal["id"].as_str().expect("meal id");

    let cookbook_payload = json!({
        "id": "http-cookbook",
        "title": "HTTP Cookbook",
        "authorIds": ["author-1"]
    });
    let cookbook = send(
        &app,
        Method::POST,
        "/api/cookbooks",
        Some(cookbook_payload.clone()),
    )
    .await;
    assert_eq!(cookbook.status(), StatusCode::OK);
    assert_eq!(json_body(cookbook).await["shareScope"], "personal");
    let conflict = send(&app, Method::POST, "/api/cookbooks", Some(cookbook_payload)).await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(conflict).await,
        json!({"error": "cookbook already exists"})
    );

    let recipe = send(
        &app,
        Method::POST,
        "/api/recipes",
        Some(recipe_payload("http-recipe", "HTTP Rice")),
    )
    .await;
    assert_eq!(recipe.status(), StatusCode::OK);
    let recipe = json_body(recipe).await;
    assert_eq!(recipe["totalMinutes"], 15);
    assert_eq!(recipe["costCents"], 100);
    assert_eq!(recipe["costPerServingCents"], 50);

    let update = send(
        &app,
        Method::PUT,
        "/api/recipes/http-recipe",
        Some(recipe_payload("ignored-body-id", "Updated HTTP Rice")),
    )
    .await;
    assert_eq!(update.status(), StatusCode::OK);
    let update = json_body(update).await;
    assert_eq!(update["id"], "http-recipe");
    assert_eq!(update["title"], "Updated HTTP Rice");

    let made = send(
        &app,
        Method::POST,
        "/api/recipes/http-recipe/made",
        Some(json!({
            "madeAt": "2026-07-10T10:00:00.000Z",
            "servingsMade": 2,
            "servingsEaten": 1,
            "leftoverServings": 1,
            "substitutions": [{
                "ingredientId": "http-ingredient",
                "originalItem": "rice",
                "substituteText": "brown rice"
            }]
        })),
    )
    .await;
    assert_eq!(made.status(), StatusCode::OK);
    assert_eq!(json_body(made).await["timesMade"], 1);

    let cook_log = send(&app, Method::GET, "/api/cook-log", None).await;
    assert_eq!(cook_log.status(), StatusCode::OK);
    assert!(
        json_body(cook_log)
            .await
            .as_array()
            .expect("cook log array")
            .iter()
            .any(|entry| entry["recipeId"] == "http-recipe")
    );

    assert_eq!(
        send(
            &app,
            Method::DELETE,
            &format!("/api/meal-plan/{meal_id}"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            Method::DELETE,
            &format!("/api/pantry/{pantry_id}"),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, Method::DELETE, "/api/recipes/http-recipe", None)
            .await
            .status(),
        StatusCode::OK
    );
    let missing = send(&app, Method::DELETE, "/api/recipes/http-recipe", None).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        json_body(missing).await,
        json!({"error": "recipe not found"})
    );

    let malformed = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/pantry")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(malformed).await,
        json!({"error": "invalid request"})
    );
}
