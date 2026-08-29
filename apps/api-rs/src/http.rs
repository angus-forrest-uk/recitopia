use std::{
    collections::HashSet,
    path::{Path as FilePath, PathBuf},
    sync::Arc,
    time::Instant,
};

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    extract::{Path, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::any,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::{
    assets::{
        AssetError, AssetManager, IMAGE_DERIVATIVE_MAX_BYTES, MAX_ARCHIVE_BYTES,
        MAX_PAGE_IMAGE_UPLOAD_BODY_BYTES,
    },
    jobs::{CancellationSignal, JobRegistry, JobRegistryError},
    model::{
        AcceptPageContentInput, Cookbook, CookbookContentBlockPatch, CookbookImageSetImportSummary,
        CookbookImport, CookbookImportProgress, CookbookImportStatus, CookbookPage,
        CookbookPageImageUpload, CookbookPageImageUploadInput, CookbookPageKind, CookbookPagePatch,
        CookbookPageReviewStatus, CookbookRecipeDraftInput, CookbookSourceImport,
        CookbookSourceImportInput, CookbookSourceKind, ErrorResponse, HealthResponse,
        HealthUnavailableResponse, ImageRecipeImportInput, ImportJobState, ImportPipelineStage,
        MarkMadeInput, MealPlanEntry, MealPlanEntryInput, OkResponse, PantryItem, PantryItemInput,
        PantryItemPatch, Recipe, RecipeSourcePageSpan,
    },
    pipeline::{
        PipelineError, PipelineService, ProgressReporter, ProgressUpdate, RecipeDraftSource,
        validate_draft,
    },
    runtime::{generate_id, now_iso8601},
    store::{ReadStore, StoreError, WriteStore},
};

const MAX_PAGE_IMAGE_BYTES: u64 = 128 << 20;
const MAX_JSON_BODY_BYTES: usize = 16 << 20;

#[derive(Clone)]
pub struct AppState {
    store: Arc<dyn ReadStore>,
    write_store: Option<Arc<dyn WriteStore>>,
    assets: Option<Arc<AssetManager>>,
    jobs: Arc<JobRegistry>,
    pipeline: Option<Arc<PipelineService>>,
}

impl AppState {
    #[must_use]
    pub fn new(store: Arc<dyn ReadStore>) -> Self {
        Self {
            store,
            write_store: None,
            assets: None,
            jobs: Arc::new(JobRegistry::default()),
            pipeline: None,
        }
    }

    #[must_use]
    pub fn with_write_store(store: Arc<dyn ReadStore>, write_store: Arc<dyn WriteStore>) -> Self {
        Self {
            store,
            write_store: Some(write_store),
            assets: None,
            jobs: Arc::new(JobRegistry::default()),
            pipeline: None,
        }
    }

    #[must_use]
    pub fn with_assets(mut self, assets: Arc<AssetManager>) -> Self {
        self.assets = Some(assets);
        self
    }

    #[must_use]
    pub fn with_pipeline(mut self, pipeline: Arc<PipelineService>) -> Self {
        self.pipeline = Some(pipeline);
        self
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", any(health_route))
        .route("/api/catalogue", any(catalogue_route))
        .route("/api/pantry", any(pantry_route))
        .route("/api/pantry/{item_id}", any(pantry_item_route))
        .route("/api/meal-plan", any(meal_plan_route))
        .route("/api/meal-plan/{entry_id}", any(meal_plan_entry_route))
        .route("/api/cook-log", any(cook_log_route))
        .route("/api/cookbooks", any(cookbooks_route))
        .route("/api/cookbook-page-images", any(cookbook_page_images_route))
        .route(
            "/api/cookbook-imports/archive",
            any(cookbook_archive_import_route),
        )
        .route(
            "/api/cookbook-imports/images",
            any(cookbook_image_set_import_route),
        )
        .route(
            "/api/cookbook-imports/{import_id}/ocr",
            any(cookbook_import_ocr_route),
        )
        .route(
            "/api/cookbook-imports/{import_id}/progress",
            any(cookbook_import_progress_route),
        )
        .route(
            "/api/cookbook-imports/{import_id}/cancel",
            any(cookbook_import_cancel_route),
        )
        .route("/api/imports/images", any(recipe_image_import_route))
        .route("/api/imports/{import_id}", any(recipe_import_route))
        .route(
            "/api/imports/{import_id}/draft",
            any(recipe_import_draft_route),
        )
        .route(
            "/api/imports/{import_id}/commit",
            any(recipe_import_commit_route),
        )
        .route(
            "/api/cookbook-recipe-drafts",
            any(cookbook_recipe_draft_route),
        )
        .route(
            "/api/pipeline-diagnostics/cookbook",
            any(pipeline_diagnostic_start_route),
        )
        .route(
            "/api/pipeline-diagnostics/introduction-page",
            any(introduction_diagnostic_start_route),
        )
        .route(
            "/api/pipeline-diagnostics/{job_id}/progress",
            any(pipeline_diagnostic_progress_route),
        )
        .route(
            "/api/pipeline-diagnostics/{job_id}/cancel",
            any(pipeline_diagnostic_cancel_route),
        )
        .route(
            "/api/pipeline-diagnostics/{job_id}/introduction-page",
            any(introduction_diagnostic_result_route),
        )
        .route("/api/recipes", any(recipes_route))
        .route("/api/recipes/{recipe_id}", any(recipe_route))
        .route("/api/recipes/{recipe_id}/made", any(mark_recipe_made_route))
        .route(
            "/api/cookbook-pages/{page_id}/text",
            any(cookbook_page_text_route),
        )
        .route(
            "/api/cookbook-pages/{page_id}/image",
            any(cookbook_page_image_route),
        )
        .route(
            "/api/cookbook-pages/{page_id}/accept-content",
            any(accept_cookbook_page_content_route),
        )
        .route("/api/cookbook-pages/{page_id}", any(cookbook_page_route))
        .route(
            "/api/cookbook-content-blocks/{block_id}",
            any(cookbook_content_block_route),
        )
        .route(
            "/api/cookbooks/{cookbook_id}/blocks",
            any(cookbook_content_blocks_route),
        )
        .fallback(not_found)
        .layer(middleware::from_fn(request_log))
        .layer(middleware::from_fn(cors))
        .with_state(state)
}

async fn health_route(State(state): State<AppState>, method: Method) -> Response {
    if method != Method::GET {
        return not_found().await;
    }

    let store = Arc::clone(&state.store);
    match tokio::task::spawn_blocking(move || store.ping()).await {
        Ok(Ok(())) => (StatusCode::OK, Json(HealthResponse { ok: true })).into_response(),
        Ok(Err(error)) => {
            tracing::warn!(
                event = "health_store_unavailable",
                error = %error,
                "database health probe failed"
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthUnavailableResponse {
                    ok: false,
                    error: "database unavailable",
                }),
            )
                .into_response()
        }
        Err(error) => {
            tracing::error!(
                event = "health_probe_task_failed",
                error = %error,
                "database health task failed"
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(HealthUnavailableResponse {
                    ok: false,
                    error: "database unavailable",
                }),
            )
                .into_response()
        }
    }
}

async fn catalogue_route(State(state): State<AppState>, method: Method) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match store_read(state.store, "catalogue", |store| store.catalogue_summary()).await {
        Ok(catalogue) => (StatusCode::OK, Json(catalogue)).into_response(),
        Err(error) => store_failure("catalogue", &error),
    }
}

async fn pantry_route(State(state): State<AppState>, method: Method, request: Request) -> Response {
    match method {
        Method::GET => {
            match store_read(state.store, "pantry", |store| store.pantry_items()).await {
                Ok(items) => (StatusCode::OK, Json(items)).into_response(),
                Err(error) => store_failure("pantry", &error),
            }
        }
        Method::POST => {
            let input = match parse_json_body::<PantryItemInput>(request).await {
                Ok(input) => input,
                Err(response) => return response,
            };
            let item = PantryItem {
                id: generate_id("pantry"),
                item: input.item,
                display_name: input.display_name,
                quantity: input.quantity,
                unit: input.unit,
                category: input.category,
                source_recipe_id: input.source_recipe_id,
                notes: input.notes,
                expires_at: input.expires_at,
                added_at: now_iso8601(),
                owner_user_id: None,
                family_id: None,
            };
            let Some(store) = state.write_store else {
                return write_store_unavailable("add_pantry_item");
            };
            match store_write(store, "add_pantry_item", move |store| {
                store.add_pantry_item(item)
            })
            .await
            {
                Ok(item) => (StatusCode::OK, Json(item)).into_response(),
                Err(error) => store_failure("add_pantry_item", &error),
            }
        }
        _ => not_found().await,
    }
}

async fn pantry_item_route(
    State(state): State<AppState>,
    Path(item_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    let Some(store) = state.write_store else {
        return match method {
            Method::PATCH | Method::DELETE => write_store_unavailable("pantry_item"),
            _ => not_found().await,
        };
    };
    match method {
        Method::PATCH => {
            let patch = match parse_json_body::<PantryItemPatch>(request).await {
                Ok(patch) => patch,
                Err(response) => return response,
            };
            match store_write(store, "patch_pantry_item", move |store| {
                store.patch_pantry_item(&item_id, patch)
            })
            .await
            {
                Ok(item) => (StatusCode::OK, Json(item)).into_response(),
                Err(StoreError::PantryItemNotFound) => {
                    json_error(StatusCode::NOT_FOUND, "pantry item not found")
                }
                Err(error) => store_failure("patch_pantry_item", &error),
            }
        }
        Method::DELETE => {
            match store_write(store, "delete_pantry_item", move |store| {
                store.delete_pantry_item(&item_id)
            })
            .await
            {
                Ok(()) => ok_response(),
                Err(error) => store_failure("delete_pantry_item", &error),
            }
        }
        _ => not_found().await,
    }
}

async fn meal_plan_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    match method {
        Method::GET => {
            match store_read(state.store, "meal_plan", |store| store.meal_plan_entries()).await {
                Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
                Err(error) => store_failure("meal_plan", &error),
            }
        }
        Method::POST => {
            let input = match parse_json_body::<MealPlanEntryInput>(request).await {
                Ok(input) => input,
                Err(response) => return response,
            };
            let entry = MealPlanEntry {
                id: generate_id("meal-plan"),
                date: input.date,
                meal_type: input.meal_type,
                recipe_id: input.recipe_id,
                servings: input.servings,
                notes: input.notes,
                owner_user_id: None,
                family_id: None,
            };
            let Some(store) = state.write_store else {
                return write_store_unavailable("add_meal_plan_entry");
            };
            match store_write(store, "add_meal_plan_entry", move |store| {
                store.add_meal_plan_entry(entry)
            })
            .await
            {
                Ok(entry) => (StatusCode::OK, Json(entry)).into_response(),
                Err(error) => store_failure("add_meal_plan_entry", &error),
            }
        }
        _ => not_found().await,
    }
}

async fn meal_plan_entry_route(
    State(state): State<AppState>,
    Path(entry_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::DELETE {
        return not_found().await;
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("delete_meal_plan_entry");
    };
    match store_write(store, "delete_meal_plan_entry", move |store| {
        store.delete_meal_plan_entry(&entry_id)
    })
    .await
    {
        Ok(()) => ok_response(),
        Err(error) => store_failure("delete_meal_plan_entry", &error),
    }
}

async fn cook_log_route(State(state): State<AppState>, method: Method) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match store_read(state.store, "cook_log", |store| store.cook_log_entries()).await {
        Ok(entries) => (StatusCode::OK, Json(entries)).into_response(),
        Err(error) => store_failure("cook_log", &error),
    }
}

async fn cookbooks_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let cookbook = match parse_json_body::<Cookbook>(request).await {
        Ok(cookbook) => cookbook,
        Err(response) => return response,
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("create_cookbook");
    };
    match store_write(store, "create_cookbook", move |store| {
        store.create_cookbook(cookbook)
    })
    .await
    {
        Ok(cookbook) => (StatusCode::OK, Json(cookbook)).into_response(),
        Err(StoreError::CookbookAlreadyExists) => {
            json_error(StatusCode::CONFLICT, "cookbook already exists")
        }
        Err(StoreError::InvalidCookbook) => json_error(StatusCode::BAD_REQUEST, "invalid cookbook"),
        Err(error) => store_failure("create_cookbook", &error),
    }
}

async fn cookbook_page_images_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("upload_cookbook_page_image");
    };
    let Some(assets) = state.assets else {
        return asset_service_unavailable("upload_cookbook_page_image");
    };
    let input = match parse_json_body_limit::<CookbookPageImageUploadInput>(
        request,
        MAX_PAGE_IMAGE_UPLOAD_BODY_BYTES,
    )
    .await
    {
        Ok(input) => input,
        Err(response) => return response,
    };
    let CookbookPageImageUploadInput {
        file_name: _,
        mime_type,
        image_base64,
    } = input;
    let stored = match tokio::task::spawn_blocking(move || {
        assets.store_base64_image(&image_base64, &mime_type)
    })
    .await
    {
        Ok(Ok(stored)) => stored,
        Ok(Err(AssetError::InvalidBase64)) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid cookbook page image");
        }
        Ok(Err(AssetError::ImageTooLarge)) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "cookbook page image is too large",
            );
        }
        Ok(Err(error)) => return asset_failure("upload_cookbook_page_image", &error),
        Err(error) => return blocking_task_failure("upload_cookbook_page_image", &error),
    };
    let hash = stored.image_hash.clone();
    match store_write(store, "check_cookbook_page_image_hash", move |store| {
        store.cookbook_page_image_hash_exists(&hash)
    })
    .await
    {
        Ok(true) => json_error(StatusCode::CONFLICT, "duplicate cookbook page image"),
        Ok(false) => (
            StatusCode::OK,
            Json(CookbookPageImageUpload {
                image_path: stored.image_path,
                image_hash: stored.image_hash,
                size_bytes: stored.size_bytes,
            }),
        )
            .into_response(),
        Err(error) => store_failure("check_cookbook_page_image_hash", &error),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CookbookArchiveImportQuery {
    cookbook_id: String,
    source_path: String,
}

async fn cookbook_archive_import_route(
    State(state): State<AppState>,
    method: Method,
    Query(query): Query<CookbookArchiveImportQuery>,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    if query.cookbook_id.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "cookbook id is required");
    }
    if query.source_path.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "source path is required");
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("create_cookbook_archive_import");
    };
    let Some(assets) = state.assets else {
        return asset_service_unavailable("create_cookbook_archive_import");
    };
    let cookbook_id = query.cookbook_id.clone();
    match store_write(store.clone(), "cookbook_exists", move |store| {
        store.cookbook_exists(&cookbook_id)
    })
    .await
    {
        Ok(true) => {}
        Ok(false) => return json_error(StatusCode::NOT_FOUND, "cookbook not found"),
        Err(error) => return store_failure("cookbook_exists", &error),
    }

    let content_length = match request
        .headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
    {
        Some(length) if length <= MAX_ARCHIVE_BYTES => length,
        Some(_) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "cookbook archive is too large",
            );
        }
        None => {
            return json_error(
                StatusCode::BAD_REQUEST,
                "archive content length is required",
            );
        }
    };
    let import_id = generate_id("cookbook-import");
    let asset_manager = Arc::clone(&assets);
    let path_import_id = import_id.clone();
    let archive_path = match tokio::task::spawn_blocking(move || {
        asset_manager.archive_upload_path(&path_import_id)
    })
    .await
    {
        Ok(Ok(path)) => path,
        Ok(Err(error)) => return asset_failure("prepare_cookbook_archive", &error),
        Err(error) => return blocking_task_failure("prepare_cookbook_archive", &error),
    };
    let archive_bytes = match stream_archive_body(request, &archive_path, content_length).await {
        Ok(bytes) => bytes,
        Err(ArchiveUploadError::TooLarge) => {
            return json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "cookbook archive is too large",
            );
        }
        Err(error) => {
            tracing::warn!(
                event = "cookbook_archive_body_failed",
                error = %error,
                "cookbook archive body could not be saved"
            );
            return json_error(StatusCode::BAD_REQUEST, "could not read cookbook archive");
        }
    };
    let asset_manager = Arc::clone(&assets);
    let ingestion_import_id = import_id.clone();
    let archive_images = match tokio::task::spawn_blocking(move || {
        asset_manager.ingest_archive(&archive_path, &ingestion_import_id)
    })
    .await
    {
        Ok(Ok(images)) => images,
        Ok(Err(error)) => return archive_asset_failure(&error),
        Err(error) => return blocking_task_failure("ingest_cookbook_archive", &error),
    };
    let now = now_iso8601();
    let review_notes = format!(
        "Browser archive import: {} images, {archive_bytes} bytes.",
        archive_images.len()
    );
    let import_record = CookbookImport {
        id: import_id.clone(),
        cookbook_id: query.cookbook_id.clone(),
        source_kind: CookbookSourceKind::ImageSet,
        source_path: query.source_path,
        status: CookbookImportStatus::Uploaded,
        ocr_engine: None,
        created_at: now.clone(),
        updated_at: now,
        review_notes: Some(review_notes),
    };
    let pages = archive_images
        .into_iter()
        .map(|image| CookbookPage {
            id: format!("{import_id}-page-{}", image.image_index),
            cookbook_id: query.cookbook_id.clone(),
            import_id: import_id.clone(),
            image_index: image.image_index,
            printed_page_label: None,
            printed_page_number: None,
            image_path: image.image_path,
            image_hash: Some(image.image_hash),
            ocr_text: String::new(),
            ocr_json: "{}".to_owned(),
            has_ocr_text: false,
            average_confidence: None,
            minimum_confidence: None,
            page_kind: CookbookPageKind::Unknown,
            review_status: CookbookPageReviewStatus::Pending,
        })
        .collect();
    let source = CookbookSourceImport {
        import_record,
        pages,
        sections: Vec::new(),
        content_blocks: Vec::new(),
        menus: Vec::new(),
        glossary_entries: Vec::new(),
        suppliers: Vec::new(),
        index_entries: Vec::new(),
        cross_references: Vec::new(),
    };
    persist_source_import(store, source).await
}

async fn cookbook_image_set_import_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("create_cookbook_source_import");
    };
    let input = match parse_json_body::<CookbookSourceImportInput>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if input.pages.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "at least one page is required");
    }
    if input
        .pages
        .iter()
        .any(|page| page.image_index == 0 || page.image_path.is_empty())
    {
        return json_error(StatusCode::BAD_REQUEST, "invalid cookbook page");
    }
    let import_id = generate_id("cookbook-import");
    let now = now_iso8601();
    let import_record = CookbookImport {
        id: import_id.clone(),
        cookbook_id: input.cookbook_id.clone(),
        source_kind: CookbookSourceKind::ImageSet,
        source_path: input.source_path,
        status: input.status,
        ocr_engine: input.ocr_engine,
        created_at: now.clone(),
        updated_at: now,
        review_notes: input.review_notes,
    };
    let pages = input
        .pages
        .into_iter()
        .map(|page| {
            let has_ocr_text = !page.ocr_text.trim().is_empty();
            CookbookPage {
                id: format!("{import_id}-page-{}", page.image_index),
                cookbook_id: input.cookbook_id.clone(),
                import_id: import_id.clone(),
                image_index: page.image_index,
                printed_page_label: page.printed_page_label,
                printed_page_number: page.printed_page_number,
                image_path: page.image_path,
                image_hash: page.image_hash,
                ocr_text: page.ocr_text,
                ocr_json: page.ocr_json,
                has_ocr_text,
                average_confidence: page.average_confidence,
                minimum_confidence: page.minimum_confidence,
                page_kind: page.page_kind,
                review_status: page.review_status,
            }
        })
        .collect();
    let source = CookbookSourceImport {
        import_record,
        pages,
        sections: input.sections,
        content_blocks: input.content_blocks,
        menus: input.menus,
        glossary_entries: input.glossary_entries,
        suppliers: input.suppliers,
        index_entries: input.index_entries,
        cross_references: input.cross_references,
    };
    persist_source_import(store, source).await
}

async fn persist_source_import(
    store: Arc<dyn WriteStore>,
    source: CookbookSourceImport,
) -> Response {
    let summary = CookbookImageSetImportSummary {
        import_record: source.import_record.clone(),
        page_count: source.pages.len(),
        section_count: source.sections.len(),
        content_block_count: source.content_blocks.len(),
        recipe_count: 0,
        menu_count: source.menus.len(),
        glossary_entry_count: source.glossary_entries.len(),
        supplier_count: source.suppliers.len(),
        index_entry_count: source.index_entries.len(),
        cross_reference_count: source.cross_references.len(),
    };
    match store_write(store, "create_cookbook_source_import", move |store| {
        store.create_cookbook_source_import(source)
    })
    .await
    {
        Ok(()) => (StatusCode::OK, Json(summary)).into_response(),
        Err(StoreError::CookbookNotFound) => {
            json_error(StatusCode::NOT_FOUND, "cookbook not found")
        }
        Err(StoreError::InvalidCookbookImport) => {
            json_error(StatusCode::BAD_REQUEST, "invalid cookbook import")
        }
        Err(StoreError::DuplicateCookbookPageImage) => {
            json_error(StatusCode::CONFLICT, "duplicate cookbook page image")
        }
        Err(error) => store_failure("create_cookbook_source_import", &error),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CookbookImportOcrQuery {
    #[serde(default)]
    refresh_ocr: bool,
}

async fn cookbook_import_ocr_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    Query(query): Query<CookbookImportOcrQuery>,
    method: Method,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("start_cookbook_import_ocr");
    };
    let Some(pipeline) = state.pipeline.clone() else {
        return pipeline_service_unavailable("start_cookbook_import_ocr");
    };
    let begin = match state.jobs.begin(&import_id) {
        Ok(begin) => begin,
        Err(error) => return job_registry_failure("start_cookbook_import_ocr", error),
    };
    if begin.started {
        if let Err(error) = persist_progress_snapshot(write_store, begin.progress.clone()).await {
            tracing::error!(
                event = "import_progress_initial_persist_failed",
                import_id,
                error = %error
            );
            let _ = state.jobs.update(&import_id, |progress| {
                progress.state = ImportJobState::Failed;
                progress.stage = ImportPipelineStage::Failed;
                progress.message = "Could not persist queued cookbook processing.".to_owned();
                progress.error_message = Some(error.to_string());
            });
            return store_failure("persist_import_progress", &error);
        }
        let job_state = state.clone();
        tokio::spawn(async move {
            run_cookbook_import_job(
                job_state,
                pipeline,
                import_id,
                query.refresh_ocr,
                begin.cancellation,
            )
            .await;
        });
    }
    (StatusCode::ACCEPTED, Json(begin.progress)).into_response()
}

async fn cookbook_import_progress_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match state.jobs.get(&import_id) {
        Ok(Some(progress)) => return (StatusCode::OK, Json(progress)).into_response(),
        Ok(None) => {}
        Err(error) => return job_registry_failure("get_cookbook_import_progress", error),
    }
    match store_read(state.store, "get_cookbook_import_progress", move |store| {
        store.cookbook_import_progress(&import_id)
    })
    .await
    {
        Ok(progress) => (StatusCode::OK, Json(progress)).into_response(),
        Err(StoreError::CookbookImportProgressNotFound) => {
            json_error(StatusCode::NOT_FOUND, "cookbook import progress not found")
        }
        Err(error) => store_failure("get_cookbook_import_progress", &error),
    }
}

async fn cookbook_import_cancel_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("cancel_cookbook_import");
    };
    let mut progress = match state.jobs.cancel(&import_id) {
        Ok(Some(progress)) => progress,
        Ok(None) => match store_read(
            state.store.clone(),
            "get_cookbook_import_progress_for_cancel",
            {
                let import_id = import_id.clone();
                move |store| store.cookbook_import_progress(&import_id)
            },
        )
        .await
        {
            Ok(progress) => progress,
            Err(StoreError::CookbookImportProgressNotFound) => {
                return json_error(StatusCode::NOT_FOUND, "cookbook import progress not found");
            }
            Err(error) => return store_failure("cancel_cookbook_import", &error),
        },
        Err(error) => return job_registry_failure("cancel_cookbook_import", error),
    };
    if progress.state == ImportJobState::Running {
        progress.state = ImportJobState::Canceled;
        progress.stage = ImportPipelineStage::Canceled;
        progress.message = "Cancellation requested.".to_owned();
        progress.current_section_title = None;
        progress.error_message = None;
    }
    if progress.state == ImportJobState::Canceled {
        if let Some(pipeline) = state.pipeline {
            if let Err(error) = pipeline.request_cancel(&import_id).await {
                tracing::warn!(
                    event = "cookbook_import_cancel_marker_failed",
                    import_id,
                    error = %error
                );
            }
        }
        tracing::info!(event = "cookbook_import_cancel_requested", import_id);
    }
    match persist_progress_snapshot(write_store, progress.clone()).await {
        Ok(()) => (StatusCode::OK, Json(progress)).into_response(),
        Err(error) => store_failure("cancel_cookbook_import", &error),
    }
}

async fn recipe_image_import_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("create_recipe_image_import");
    };
    let Some(assets) = state.assets else {
        return asset_service_unavailable("create_recipe_image_import");
    };
    let Some(pipeline) = state.pipeline else {
        return pipeline_service_unavailable("create_recipe_image_import");
    };
    let input = match parse_json_body_limit::<ImageRecipeImportInput>(
        request,
        MAX_PAGE_IMAGE_UPLOAD_BODY_BYTES,
    )
    .await
    {
        Ok(input) => input,
        Err(response) => return response,
    };
    let catalogue = match store_read(state.store, "load_recipe_import_cookbook", |store| {
        store.catalogue_summary()
    })
    .await
    {
        Ok(catalogue) => catalogue,
        Err(error) => return store_failure("load_recipe_import_cookbook", &error),
    };
    let Some(mut cookbook) = catalogue
        .cookbooks
        .into_iter()
        .find(|cookbook| cookbook.id == input.cookbook_id)
    else {
        return json_error(StatusCode::NOT_FOUND, "cookbook not found");
    };
    if !input.author_ids.is_empty() {
        cookbook.author_ids.clone_from(&input.author_ids);
    }
    let mime_type = input.mime_type.clone();
    let image_base64 = input.image_base64;
    let stored = match tokio::task::spawn_blocking(move || {
        assets.store_base64_image(&image_base64, &mime_type)
    })
    .await
    {
        Ok(Ok(stored)) => stored,
        Ok(Err(AssetError::InvalidBase64)) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid recipe image");
        }
        Ok(Err(AssetError::ImageTooLarge)) => {
            return json_error(StatusCode::PAYLOAD_TOO_LARGE, "recipe image is too large");
        }
        Ok(Err(error)) => return asset_failure("create_recipe_image_import", &error),
        Err(error) => return blocking_task_failure("create_recipe_image_import", &error),
    };
    let import_id = generate_id("import");
    let cancellation = CancellationSignal::new();
    let (ocr_engine, ocr_text, ocr_json) = match input.ocr_text_override {
        Some(text) if !text.trim().is_empty() => {
            ("provided-text".to_owned(), text, "{}".to_owned())
        }
        _ => match pipeline
            .ocr_recipe_image(&import_id, &stored.image_path, &cancellation)
            .await
        {
            Ok(ocr) => ocr,
            Err(error) => return pipeline_failure("create_recipe_image_import", &error),
        },
    };
    let source_label = input.source_label.unwrap_or_else(|| {
        source_label_for_range(&cookbook.title, input.page_start, input.page_end)
    });
    let draft = pipeline
        .create_recipe_draft(
            RecipeDraftSource {
                import_id,
                file_name: input.file_name,
                mime_type: input.mime_type,
                image_path: stored.image_path,
                ocr_text,
                ocr_json,
                ocr_engine,
                cookbook,
                page_start: input.page_start,
                page_end: input.page_end,
                source_label,
                source_block_id: None,
                source_page_spans: Vec::new(),
                timestamp: now_iso8601(),
            },
            &cancellation,
        )
        .await;
    let recipe_import = match draft {
        Ok(recipe_import) => recipe_import,
        Err(error) => return pipeline_failure("create_recipe_image_import", &error),
    };
    match store_write(write_store, "create_recipe_import", move |store| {
        store.create_recipe_import(recipe_import)
    })
    .await
    {
        Ok(recipe_import) => (StatusCode::OK, Json(recipe_import)).into_response(),
        Err(error) => store_failure("create_recipe_import", &error),
    }
}

async fn recipe_import_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match store_read(state.store, "get_recipe_import", move |store| {
        store.recipe_import(&import_id)
    })
    .await
    {
        Ok(recipe_import) => (StatusCode::OK, Json(recipe_import)).into_response(),
        Err(StoreError::RecipeImportNotFound) => {
            json_error(StatusCode::NOT_FOUND, "import not found")
        }
        Err(error) => store_failure("get_recipe_import", &error),
    }
}

async fn recipe_import_draft_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::PUT {
        return not_found().await;
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("update_recipe_import_draft");
    };
    let recipe = match parse_json_body::<Recipe>(request).await {
        Ok(recipe) => recipe,
        Err(response) => return response,
    };
    let issues = validate_draft(&recipe);
    let updated_at = now_iso8601();
    match store_write(store, "update_recipe_import_draft", move |store| {
        store.update_recipe_import_draft(&import_id, recipe, issues, &updated_at)
    })
    .await
    {
        Ok(recipe_import) => (StatusCode::OK, Json(recipe_import)).into_response(),
        Err(StoreError::RecipeImportNotFound) => {
            json_error(StatusCode::NOT_FOUND, "import not found")
        }
        Err(error) => store_failure("update_recipe_import_draft", &error),
    }
}

async fn recipe_import_commit_route(
    State(state): State<AppState>,
    Path(import_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(store) = state.write_store else {
        return write_store_unavailable("commit_recipe_import");
    };
    let recipe = match parse_json_body::<Recipe>(request).await {
        Ok(recipe) => recipe,
        Err(response) => return response,
    };
    let issues = validate_draft(&recipe);
    let updated_at = now_iso8601();
    match store_write(store, "commit_recipe_import", move |store| {
        store.commit_recipe_import(&import_id, recipe, issues, &updated_at)
    })
    .await
    {
        Ok(recipe) => (StatusCode::OK, Json(recipe)).into_response(),
        Err(StoreError::RecipeImportNotFound) => {
            json_error(StatusCode::NOT_FOUND, "import not found")
        }
        Err(StoreError::InvalidRecipe(_)) => json_error(StatusCode::BAD_REQUEST, "invalid recipe"),
        Err(error) => store_failure("commit_recipe_import", &error),
    }
}

async fn cookbook_recipe_draft_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(store) = state.write_store.clone() else {
        return write_store_unavailable("create_cookbook_recipe_draft");
    };
    let Some(pipeline) = state.pipeline else {
        return pipeline_service_unavailable("create_cookbook_recipe_draft");
    };
    let input = match parse_json_body::<CookbookRecipeDraftInput>(request).await {
        Ok(input) => input,
        Err(response) => return response,
    };
    if input.source_block_id.is_none() && input.page_id.is_none() && input.page_ids.is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "source block or page is required");
    }
    let catalogue = match store_read(
        state.store.clone(),
        "load_cookbook_recipe_source",
        |store| store.catalogue_summary(),
    )
    .await
    {
        Ok(catalogue) => catalogue,
        Err(error) => return store_failure("load_cookbook_recipe_source", &error),
    };
    let Some(cookbook) = catalogue
        .cookbooks
        .iter()
        .find(|cookbook| cookbook.id == input.cookbook_id)
        .cloned()
    else {
        return json_error(StatusCode::NOT_FOUND, "cookbook not found");
    };

    let full_blocks = if input.source_block_id.is_some() {
        match store_read(state.store.clone(), "load_cookbook_recipe_block", {
            let cookbook_id = input.cookbook_id.clone();
            move |store| store.cookbook_content_blocks(&cookbook_id)
        })
        .await
        {
            Ok(blocks) => blocks,
            Err(error) => return store_failure("load_cookbook_recipe_block", &error),
        }
    } else {
        Vec::new()
    };
    let source_block = input.source_block_id.as_ref().and_then(|block_id| {
        full_blocks
            .iter()
            .find(|block| block.id == *block_id)
            .cloned()
    });
    if input.source_block_id.is_some() && source_block.is_none() {
        return json_error(StatusCode::NOT_FOUND, "source block not found");
    }

    let mut selected_ids = Vec::new();
    if let Some(page_id) = input.page_id {
        selected_ids.push(page_id);
    }
    selected_ids.extend(input.page_ids);
    if let Some(block) = &source_block {
        selected_ids.extend(
            catalogue
                .cookbook_pages
                .iter()
                .filter(|page| {
                    page.cookbook_id == input.cookbook_id
                        && page_in_range(page, block.page_start, block.page_end)
                })
                .map(|page| page.id.clone()),
        );
    }
    let mut seen = HashSet::new();
    selected_ids.retain(|id| seen.insert(id.clone()));

    let mut text_parts = Vec::new();
    if let Some(block) = &source_block {
        if let Some(title) = &block.title {
            text_parts.push(title.clone());
        }
        if !block.text.trim().is_empty() {
            text_parts.push(block.text.clone());
        }
    }
    let mut spans = Vec::new();
    let mut image_path = None;
    let mut page_start = source_block.as_ref().and_then(|block| block.page_start);
    let mut page_end = source_block.as_ref().and_then(|block| block.page_end);
    for page_id in selected_ids {
        let Some(page) = catalogue
            .cookbook_pages
            .iter()
            .find(|page| page.id == page_id && page.cookbook_id == input.cookbook_id)
        else {
            return json_error(StatusCode::NOT_FOUND, "page not found");
        };
        let page_text = match store_read(state.store.clone(), "load_cookbook_recipe_page", {
            let page_id = page.id.clone();
            move |store| store.cookbook_page_text(&page_id)
        })
        .await
        {
            Ok(page_text) => page_text,
            Err(StoreError::CookbookPageNotFound) => {
                return json_error(StatusCode::NOT_FOUND, "page not found");
            }
            Err(error) => return store_failure("load_cookbook_recipe_page", &error),
        };
        let number = page.printed_page_number.unwrap_or(page.image_index);
        text_parts.push(format!("[Page {number}]\n{}", page_text.ocr_text));
        spans.push(RecipeSourcePageSpan {
            page_id: Some(page.id.clone()),
            printed_page_number: Some(number),
            line_start: None,
            line_end: None,
            confidence: page.average_confidence,
        });
        image_path.get_or_insert_with(|| page.image_path.clone());
        page_start = Some(page_start.map_or(number, |current| current.min(number)));
        page_end = Some(page_end.map_or(number, |current| current.max(number)));
    }
    let ocr_text = text_parts.join("\n\n");
    if ocr_text.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "source text is empty");
    }
    let source_label = input
        .source_label
        .unwrap_or_else(|| source_label_for_range(&cookbook.title, page_start, page_end));
    let import_id = generate_id("import");
    let timestamp = now_iso8601();
    let recipe_import = pipeline
        .create_recipe_draft(
            RecipeDraftSource {
                import_id,
                file_name: source_block
                    .as_ref()
                    .map_or_else(|| "cookbook-source".to_owned(), |block| block.id.clone()),
                mime_type: "text/plain".to_owned(),
                image_path: image_path.unwrap_or_else(|| "cookbook-source:text".to_owned()),
                ocr_text,
                ocr_json: "{}".to_owned(),
                ocr_engine: "stored-ocr".to_owned(),
                cookbook,
                page_start,
                page_end,
                source_label,
                source_block_id: source_block.map(|block| block.id),
                source_page_spans: spans,
                timestamp,
            },
            &CancellationSignal::new(),
        )
        .await;
    let recipe_import = match recipe_import {
        Ok(recipe_import) => recipe_import,
        Err(error) => return pipeline_failure("create_cookbook_recipe_draft", &error),
    };
    match store_write(store, "create_cookbook_recipe_draft", move |store| {
        store.create_recipe_import(recipe_import)
    })
    .await
    {
        Ok(recipe_import) => (StatusCode::OK, Json(recipe_import)).into_response(),
        Err(error) => store_failure("create_cookbook_recipe_draft", &error),
    }
}

fn page_in_range(page: &CookbookPage, start: Option<u32>, end: Option<u32>) -> bool {
    let number = page.printed_page_number.unwrap_or(page.image_index);
    start.is_none_or(|start| number >= start) && end.is_none_or(|end| number <= end)
}

fn source_label_for_range(title: &str, start: Option<u32>, end: Option<u32>) -> String {
    match (start, end) {
        (Some(start), Some(end)) if end > start => format!("{title}, pp. {start}-{end}"),
        (Some(page), _) | (_, Some(page)) => format!("{title}, p. {page}"),
        (None, None) => title.to_owned(),
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PipelineDiagnosticQuery {
    cookbook_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IntroductionDiagnosticQuery {
    cookbook_id: String,
    #[serde(default = "default_introduction_image_index")]
    image_index: u32,
    #[serde(default, rename = "printedPage")]
    _printed_page: Option<u32>,
}

const fn default_introduction_image_index() -> u32 {
    4
}

async fn pipeline_diagnostic_start_route(
    State(state): State<AppState>,
    Query(query): Query<PipelineDiagnosticQuery>,
    method: Method,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("start_pipeline_diagnostic");
    };
    let Some(pipeline) = state.pipeline.clone() else {
        return pipeline_service_unavailable("start_pipeline_diagnostic");
    };
    if query.cookbook_id.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "cookbook id is required");
    }
    let job_id = generate_id("diagnostic");
    let begin = match state.jobs.begin(&job_id) {
        Ok(begin) => begin,
        Err(error) => return job_registry_failure("start_pipeline_diagnostic", error),
    };
    if let Err(error) = persist_progress_snapshot(write_store, begin.progress.clone()).await {
        return store_failure("start_pipeline_diagnostic", &error);
    }
    let job_state = state.clone();
    tokio::spawn(async move {
        run_pipeline_diagnostic_job(
            job_state,
            pipeline,
            job_id,
            query.cookbook_id,
            begin.cancellation,
        )
        .await;
    });
    (StatusCode::ACCEPTED, Json(begin.progress)).into_response()
}

async fn introduction_diagnostic_start_route(
    State(state): State<AppState>,
    Query(query): Query<IntroductionDiagnosticQuery>,
    method: Method,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("start_introduction_diagnostic");
    };
    let Some(pipeline) = state.pipeline.clone() else {
        return pipeline_service_unavailable("start_introduction_diagnostic");
    };
    if query.cookbook_id.trim().is_empty() {
        return json_error(StatusCode::BAD_REQUEST, "cookbook id is required");
    }
    let job_id = generate_id("diagnostic-intro");
    let begin = match state.jobs.begin(&job_id) {
        Ok(begin) => begin,
        Err(error) => return job_registry_failure("start_introduction_diagnostic", error),
    };
    if let Err(error) = persist_progress_snapshot(write_store, begin.progress.clone()).await {
        return store_failure("start_introduction_diagnostic", &error);
    }
    let job_state = state.clone();
    tokio::spawn(async move {
        run_introduction_diagnostic_job(
            job_state,
            pipeline,
            job_id,
            query.cookbook_id,
            query.image_index,
            begin.cancellation,
        )
        .await;
    });
    (StatusCode::ACCEPTED, Json(begin.progress)).into_response()
}

async fn pipeline_diagnostic_progress_route(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match state.jobs.get(&job_id) {
        Ok(Some(progress)) => return (StatusCode::OK, Json(progress)).into_response(),
        Ok(None) => {}
        Err(error) => return job_registry_failure("get_pipeline_diagnostic_progress", error),
    }
    match store_read(
        state.store,
        "get_pipeline_diagnostic_progress",
        move |store| store.cookbook_import_progress(&job_id),
    )
    .await
    {
        Ok(progress) => (StatusCode::OK, Json(progress)).into_response(),
        Err(StoreError::CookbookImportProgressNotFound) => {
            json_error(StatusCode::NOT_FOUND, "pipeline diagnostic not found")
        }
        Err(error) => store_failure("get_pipeline_diagnostic_progress", &error),
    }
}

async fn pipeline_diagnostic_cancel_route(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let Some(write_store) = state.write_store.clone() else {
        return write_store_unavailable("cancel_pipeline_diagnostic");
    };
    let mut progress = match state.jobs.cancel(&job_id) {
        Ok(Some(progress)) => progress,
        Ok(None) => match store_read(
            state.store,
            "get_pipeline_diagnostic_progress_for_cancel",
            {
                let job_id = job_id.clone();
                move |store| store.cookbook_import_progress(&job_id)
            },
        )
        .await
        {
            Ok(progress) => progress,
            Err(StoreError::CookbookImportProgressNotFound) => {
                return json_error(StatusCode::NOT_FOUND, "pipeline diagnostic not found");
            }
            Err(error) => return store_failure("cancel_pipeline_diagnostic", &error),
        },
        Err(error) => return job_registry_failure("cancel_pipeline_diagnostic", error),
    };
    if progress.state == ImportJobState::Running {
        progress.state = ImportJobState::Canceled;
        progress.stage = ImportPipelineStage::Canceled;
        progress.message = "Cancellation requested.".to_owned();
        progress.current_section_title = None;
        progress.error_message = None;
    }
    if progress.state == ImportJobState::Canceled {
        if let Some(pipeline) = state.pipeline {
            if let Err(error) = pipeline.request_cancel(&job_id).await {
                tracing::warn!(
                    event = "pipeline_diagnostic_cancel_marker_failed",
                    job_id,
                    error = %error
                );
            }
        }
    }
    match persist_progress_snapshot(write_store, progress.clone()).await {
        Ok(()) => (StatusCode::OK, Json(progress)).into_response(),
        Err(error) => store_failure("cancel_pipeline_diagnostic", &error),
    }
}

async fn introduction_diagnostic_result_route(
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match state.jobs.diagnostic(&job_id) {
        Ok(Some(result)) => return (StatusCode::OK, Json(result)).into_response(),
        Ok(None) => {}
        Err(error) => return job_registry_failure("get_introduction_diagnostic", error),
    }
    let Some(pipeline) = state.pipeline else {
        return pipeline_service_unavailable("get_introduction_diagnostic");
    };
    match pipeline.load_introduction_diagnostic(&job_id).await {
        Ok(result) => (StatusCode::OK, Json(result)).into_response(),
        Err(PipelineError::DiagnosticPageNotFound) => {
            json_error(StatusCode::NOT_FOUND, "introduction diagnostic not found")
        }
        Err(PipelineError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            json_error(StatusCode::NOT_FOUND, "introduction diagnostic not found")
        }
        Err(error) => pipeline_failure("get_introduction_diagnostic", &error),
    }
}

async fn run_pipeline_diagnostic_job(
    state: AppState,
    pipeline: Arc<PipelineService>,
    job_id: String,
    cookbook_id: String,
    cancellation: CancellationSignal,
) {
    let Some(write_store) = state.write_store.clone() else {
        return;
    };
    let (report, persistence_task) = diagnostic_progress_reporter(&state, write_store, &job_id);
    let source = store_read(
        state.store,
        "load_pipeline_diagnostic_source",
        move |store| store.latest_cookbook_pipeline_source(&cookbook_id),
    )
    .await;
    let result = match source {
        Ok(source) => {
            pipeline
                .run_diagnostic(source, &job_id, &cancellation, Arc::clone(&report))
                .await
        }
        Err(error) => {
            fail_job(
                &report,
                format!("Pipeline diagnostic failed: {error}."),
                error.to_string(),
            );
            drop(report);
            let _ = persistence_task.await;
            return;
        }
    };
    match result {
        Ok(run) => {
            report(ProgressUpdate {
                state: Some(ImportJobState::Complete),
                stage: Some(ImportPipelineStage::Complete),
                message: Some("Pipeline diagnostic complete.".to_owned()),
                current: Some(run.processed_count + run.skipped_count + run.failed_count),
                total: Some(run.processed_count + run.skipped_count + run.failed_count),
                processed_count: Some(run.processed_count),
                skipped_count: Some(run.skipped_count),
                failed_count: Some(run.failed_count),
                section_count: Some(run.persistence.sections.len()),
                content_block_count: Some(run.persistence.content_blocks.len()),
                recipe_count: Some(run.persistence.recipes.len()),
                extraction_engine: Some(run.extraction_engine),
                clear_current_section_title: true,
                ..ProgressUpdate::default()
            });
        }
        Err(PipelineError::Canceled) => {}
        Err(error) => fail_job(
            &report,
            format!("Pipeline diagnostic failed: {error}."),
            error.to_string(),
        ),
    }
    drop(report);
    let _ = persistence_task.await;
}

async fn run_introduction_diagnostic_job(
    state: AppState,
    pipeline: Arc<PipelineService>,
    job_id: String,
    cookbook_id: String,
    image_index: u32,
    cancellation: CancellationSignal,
) {
    let Some(write_store) = state.write_store.clone() else {
        return;
    };
    let (report, persistence_task) = diagnostic_progress_reporter(&state, write_store, &job_id);
    let source = store_read(
        state.store,
        "load_introduction_diagnostic_source",
        move |store| store.latest_cookbook_pipeline_source(&cookbook_id),
    )
    .await;
    let result = match source {
        Ok(source) => {
            pipeline
                .run_introduction_diagnostic(
                    source,
                    &job_id,
                    image_index,
                    &cancellation,
                    Arc::clone(&report),
                )
                .await
        }
        Err(error) => {
            fail_job(
                &report,
                format!("Introduction page diagnostic failed: {error}."),
                error.to_string(),
            );
            drop(report);
            let _ = persistence_task.await;
            return;
        }
    };
    match result {
        Ok(result) => {
            let _ = state.jobs.put_diagnostic(result.clone());
            report(ProgressUpdate {
                state: Some(ImportJobState::Complete),
                stage: Some(ImportPipelineStage::Complete),
                message: Some("Introduction page diagnostic complete.".to_owned()),
                current: Some(1),
                total: Some(1),
                processed_count: Some(1),
                section_count: Some(result.source_map_section_count),
                content_block_count: Some(result.extracted_content_block_count),
                recipe_count: Some(result.extracted_recipe_count),
                extraction_engine: Some(result.extraction_engine),
                clear_current_section_title: true,
                ..ProgressUpdate::default()
            });
        }
        Err(PipelineError::Canceled) => {}
        Err(error) => fail_job(
            &report,
            format!("Introduction page diagnostic failed: {error}."),
            error.to_string(),
        ),
    }
    drop(report);
    let _ = persistence_task.await;
}

fn diagnostic_progress_reporter(
    state: &AppState,
    store: Arc<dyn WriteStore>,
    job_id: &str,
) -> (ProgressReporter, tokio::task::JoinHandle<()>) {
    let (snapshots, mut pending_snapshots) = mpsc::unbounded_channel();
    let persistence_job_id = job_id.to_owned();
    let persistence_task = tokio::spawn(async move {
        while let Some(progress) = pending_snapshots.recv().await {
            if let Err(error) = persist_progress_snapshot(store.clone(), progress).await {
                tracing::error!(
                    event = "diagnostic_progress_persist_failed",
                    job_id = persistence_job_id,
                    error = %error
                );
            }
        }
    });
    let jobs = state.jobs.clone();
    let reporter_job_id = job_id.to_owned();
    let report: ProgressReporter = Arc::new(move |update| {
        match jobs.update(&reporter_job_id, |progress| {
            apply_progress_update(progress, update);
        }) {
            Ok(Some(progress)) => {
                let _ = snapshots.send(progress);
            }
            Ok(None) => {}
            Err(error) => tracing::error!(
                event = "diagnostic_progress_update_failed",
                job_id = reporter_job_id,
                error = %error
            ),
        }
    });
    (report, persistence_task)
}

async fn run_cookbook_import_job(
    state: AppState,
    pipeline: Arc<PipelineService>,
    import_id: String,
    refresh_ocr: bool,
    cancellation: crate::jobs::CancellationSignal,
) {
    let Some(write_store) = state.write_store.clone() else {
        return;
    };
    let (snapshots, mut pending_snapshots) = mpsc::unbounded_channel();
    let persistence_store = write_store.clone();
    let persistence_import_id = import_id.clone();
    let persistence_task = tokio::spawn(async move {
        while let Some(progress) = pending_snapshots.recv().await {
            if let Err(error) = persist_progress_snapshot(persistence_store.clone(), progress).await
            {
                tracing::error!(
                    event = "import_progress_persist_failed",
                    import_id = persistence_import_id,
                    error = %error
                );
            }
        }
    });
    let reporter_jobs = state.jobs.clone();
    let reporter_import_id = import_id.clone();
    let report: ProgressReporter = Arc::new(move |update| {
        match reporter_jobs.update(&reporter_import_id, |progress| {
            apply_progress_update(progress, update);
        }) {
            Ok(Some(progress)) => {
                let _ = snapshots.send(progress);
            }
            Ok(None) => {}
            Err(error) => tracing::error!(
                event = "import_progress_update_failed",
                import_id = reporter_import_id,
                error = %error
            ),
        }
    });

    let source = store_read(state.store.clone(), "load_cookbook_pipeline_source", {
        let import_id = import_id.clone();
        move |store| store.cookbook_pipeline_source(&import_id)
    })
    .await;
    let result = match source {
        Ok(source) => {
            pipeline
                .run_cookbook(source, refresh_ocr, &cancellation, Arc::clone(&report))
                .await
        }
        Err(error) => {
            fail_job(
                &report,
                format!("Cookbook import processing failed: {error}."),
                error.to_string(),
            );
            drop(report);
            let _ = persistence_task.await;
            tracing::error!(
                event = "cookbook_import_job_failed",
                import_id,
                error = %error
            );
            return;
        }
    };

    match result {
        Ok(run) => {
            report(ProgressUpdate {
                stage: Some(ImportPipelineStage::Persisting),
                message: Some("Saving extracted cookbook data.".to_owned()),
                section_count: Some(run.persistence.sections.len()),
                content_block_count: Some(run.persistence.content_blocks.len()),
                recipe_count: Some(run.persistence.recipes.len()),
                extraction_engine: Some(run.extraction_engine.clone()),
                ..ProgressUpdate::default()
            });
            let cache_updated_at = now_iso8601();
            let persist = store_write(write_store, "persist_cookbook_pipeline", move |store| {
                store.persist_cookbook_pipeline(run.persistence, &cache_updated_at)
            })
            .await;
            match persist {
                Ok(persisted) => {
                    report(ProgressUpdate {
                        state: Some(ImportJobState::Complete),
                        stage: Some(ImportPipelineStage::Complete),
                        message: Some("Cookbook import processing complete.".to_owned()),
                        current: Some(run.processed_count + run.skipped_count + run.failed_count),
                        total: Some(run.processed_count + run.skipped_count + run.failed_count),
                        processed_count: Some(run.processed_count),
                        skipped_count: Some(run.skipped_count),
                        failed_count: Some(run.failed_count),
                        recipe_count: Some(persisted.recipe_count),
                        extraction_engine: Some(run.extraction_engine.clone()),
                        clear_current_section_title: true,
                        ..ProgressUpdate::default()
                    });
                    tracing::info!(
                        event = "cookbook_import_job_complete",
                        import_id,
                        processed = run.processed_count,
                        skipped = run.skipped_count,
                        failed = run.failed_count,
                        recipes = persisted.recipe_count,
                        recipes_skipped = persisted.skipped_recipe_count,
                        engine = run.extraction_engine
                    );
                }
                Err(error) => {
                    fail_job(
                        &report,
                        format!("Cookbook import persistence failed: {error}."),
                        error.to_string(),
                    );
                    tracing::error!(
                        event = "cookbook_import_persistence_failed",
                        import_id,
                        error = %error
                    );
                }
            }
        }
        Err(PipelineError::Canceled) => {
            tracing::info!(event = "cookbook_import_job_canceled", import_id);
        }
        Err(error) => {
            fail_job(
                &report,
                format!("Cookbook import processing failed: {error}."),
                error.to_string(),
            );
            tracing::error!(
                event = "cookbook_import_job_failed",
                import_id,
                error = %error
            );
        }
    }
    drop(report);
    let _ = persistence_task.await;
}

fn apply_progress_update(progress: &mut CookbookImportProgress, update: ProgressUpdate) {
    if let Some(state) = update.state {
        progress.state = state;
    }
    if let Some(stage) = update.stage {
        progress.stage = stage;
    }
    if let Some(message) = update.message {
        progress.message = message;
    }
    if let Some(value) = update.current {
        progress.current = Some(value);
    }
    if let Some(value) = update.total {
        progress.total = Some(value);
    }
    if let Some(value) = update.processed_count {
        progress.processed_count = value;
    }
    if let Some(value) = update.skipped_count {
        progress.skipped_count = value;
    }
    if let Some(value) = update.failed_count {
        progress.failed_count = value;
    }
    if let Some(value) = update.section_count {
        progress.section_count = value;
    }
    if let Some(value) = update.content_block_count {
        progress.content_block_count = value;
    }
    if let Some(value) = update.recipe_count {
        progress.recipe_count = value;
    }
    if let Some(value) = update.current_section_index {
        progress.current_section_index = Some(value);
    }
    if let Some(value) = update.section_total {
        progress.section_total = Some(value);
    }
    if update.clear_current_section_title {
        progress.current_section_title = None;
    } else if let Some(value) = update.current_section_title {
        progress.current_section_title = Some(value);
    }
    if let Some(value) = update.extraction_engine {
        progress.extraction_engine = Some(value);
    }
    if let Some(value) = update.error_message {
        progress.error_message = Some(value);
    }
}

fn fail_job(report: &ProgressReporter, message: String, error: String) {
    report(ProgressUpdate {
        state: Some(ImportJobState::Failed),
        stage: Some(ImportPipelineStage::Failed),
        message: Some(message),
        error_message: Some(error),
        clear_current_section_title: true,
        ..ProgressUpdate::default()
    });
}

async fn persist_progress_snapshot(
    store: Arc<dyn WriteStore>,
    progress: CookbookImportProgress,
) -> Result<(), StoreError> {
    let updated_at = now_iso8601();
    store_write(store, "persist_cookbook_import_progress", move |store| {
        store.upsert_cookbook_import_progress(progress, &updated_at)
    })
    .await
}

async fn recipes_route(
    State(state): State<AppState>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let recipe = match parse_json_body::<Recipe>(request).await {
        Ok(recipe) => recipe,
        Err(response) => return response,
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("create_recipe");
    };
    let cache_updated_at = now_iso8601();
    match store_write(store, "create_recipe", move |store| {
        store.create_recipe(recipe, &cache_updated_at)
    })
    .await
    {
        Ok(recipe) => (StatusCode::OK, Json(recipe)).into_response(),
        Err(StoreError::RecipeAlreadyExists) => {
            json_error(StatusCode::CONFLICT, "recipe already exists")
        }
        Err(StoreError::InvalidRecipe(_)) => json_error(StatusCode::BAD_REQUEST, "invalid recipe"),
        Err(error) => store_failure("create_recipe", &error),
    }
}

async fn recipe_route(
    State(state): State<AppState>,
    Path(recipe_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    let Some(store) = state.write_store else {
        return match method {
            Method::PUT | Method::DELETE => write_store_unavailable("recipe_mutation"),
            _ => not_found().await,
        };
    };
    match method {
        Method::PUT => {
            let mut recipe = match parse_json_body::<Recipe>(request).await {
                Ok(recipe) => recipe,
                Err(response) => return response,
            };
            recipe.id = recipe_id;
            let cache_updated_at = now_iso8601();
            match store_write(store, "update_recipe", move |store| {
                store.update_recipe(recipe, &cache_updated_at)
            })
            .await
            {
                Ok(recipe) => (StatusCode::OK, Json(recipe)).into_response(),
                Err(StoreError::RecipeNotFound) => {
                    json_error(StatusCode::NOT_FOUND, "recipe not found")
                }
                Err(StoreError::InvalidRecipe(_)) => {
                    json_error(StatusCode::BAD_REQUEST, "invalid recipe")
                }
                Err(error) => store_failure("update_recipe", &error),
            }
        }
        Method::DELETE => {
            match store_write(store, "delete_recipe", move |store| {
                store.delete_recipe(&recipe_id)
            })
            .await
            {
                Ok(()) => ok_response(),
                Err(StoreError::RecipeNotFound) => {
                    json_error(StatusCode::NOT_FOUND, "recipe not found")
                }
                Err(error) => store_failure("delete_recipe", &error),
            }
        }
        _ => not_found().await,
    }
}

async fn mark_recipe_made_route(
    State(state): State<AppState>,
    Path(recipe_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let details = match parse_default_json_body::<MarkMadeInput>(request).await {
        Ok(details) => details,
        Err(response) => return response,
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("mark_recipe_made");
    };
    let made_at = details.made_at.clone().unwrap_or_else(now_iso8601);
    let cache_updated_at = now_iso8601();
    match store_write(store, "mark_recipe_made", move |store| {
        store.mark_recipe_made(&recipe_id, &made_at, details, &cache_updated_at)
    })
    .await
    {
        Ok(recipe) => (StatusCode::OK, Json(recipe)).into_response(),
        Err(StoreError::RecipeNotFound) => json_error(StatusCode::NOT_FOUND, "recipe not found"),
        Err(error) => store_failure("mark_recipe_made", &error),
    }
}

async fn cookbook_page_route(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::PATCH {
        return not_found().await;
    }
    let patch = match parse_json_body::<CookbookPagePatch>(request).await {
        Ok(patch) => patch,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid page patch"),
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("patch_cookbook_page");
    };
    match store_write(store, "patch_cookbook_page", move |store| {
        store.patch_cookbook_page(&page_id, patch)
    })
    .await
    {
        Ok(page) => (StatusCode::OK, Json(page)).into_response(),
        Err(StoreError::CookbookPageNotFound) => {
            json_error(StatusCode::NOT_FOUND, "page not found")
        }
        Err(error) => store_failure("patch_cookbook_page", &error),
    }
}

async fn cookbook_content_block_route(
    State(state): State<AppState>,
    Path(block_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::PATCH {
        return not_found().await;
    }
    let patch = match parse_json_body::<CookbookContentBlockPatch>(request).await {
        Ok(patch) => patch,
        Err(_) => return json_error(StatusCode::BAD_REQUEST, "invalid block patch"),
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("patch_cookbook_content_block");
    };
    match store_write(store, "patch_cookbook_content_block", move |store| {
        store.patch_cookbook_content_block(&block_id, patch)
    })
    .await
    {
        Ok(block) => (StatusCode::OK, Json(block)).into_response(),
        Err(StoreError::CookbookContentBlockNotFound) => {
            json_error(StatusCode::NOT_FOUND, "content block not found")
        }
        Err(error) => store_failure("patch_cookbook_content_block", &error),
    }
}

async fn accept_cookbook_page_content_route(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
    method: Method,
    request: Request,
) -> Response {
    if method != Method::POST {
        return not_found().await;
    }
    let input = match parse_default_json_body::<AcceptPageContentInput>(request).await {
        Ok(input) => input,
        Err(_) => {
            return json_error(StatusCode::BAD_REQUEST, "invalid accept-content body");
        }
    };
    let Some(store) = state.write_store else {
        return write_store_unavailable("accept_cookbook_page_content");
    };
    match store_write(store, "accept_cookbook_page_content", move |store| {
        store.accept_cookbook_page_content(&page_id, input)
    })
    .await
    {
        Ok(block) => (StatusCode::OK, Json(block)).into_response(),
        Err(StoreError::CookbookPageNotFound) => {
            json_error(StatusCode::NOT_FOUND, "page not found")
        }
        Err(StoreError::CookbookPageHasNoText) => {
            json_error(StatusCode::BAD_REQUEST, "page has no OCR text to accept")
        }
        Err(StoreError::CookbookPageAlreadyAccepted) => {
            json_error(StatusCode::CONFLICT, "page content was already accepted")
        }
        Err(error) => store_failure("accept_cookbook_page_content", &error),
    }
}

async fn cookbook_page_text_route(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match store_read(state.store, "cookbook_page_text", move |store| {
        store.cookbook_page_text(&page_id)
    })
    .await
    {
        Ok(page) => (StatusCode::OK, Json(page)).into_response(),
        Err(StoreError::CookbookPageNotFound) => {
            json_error(StatusCode::NOT_FOUND, "page not found")
        }
        Err(error) => store_failure("cookbook_page_text", &error),
    }
}

async fn cookbook_content_blocks_route(
    State(state): State<AppState>,
    Path(cookbook_id): Path<String>,
    method: Method,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    match store_read(state.store, "cookbook_content_blocks", move |store| {
        store.cookbook_content_blocks(&cookbook_id)
    })
    .await
    {
        Ok(blocks) => (StatusCode::OK, Json(blocks)).into_response(),
        Err(error) => store_failure("cookbook_content_blocks", &error),
    }
}

async fn cookbook_page_image_route(
    State(state): State<AppState>,
    Path(page_id): Path<String>,
    method: Method,
    request_headers: HeaderMap,
) -> Response {
    if method != Method::GET {
        return not_found().await;
    }
    let image = match store_read(state.store, "cookbook_page_image", move |store| {
        store.cookbook_page_image(&page_id)
    })
    .await
    {
        Ok(image) => image,
        Err(StoreError::CookbookPageNotFound) => {
            return json_error(StatusCode::NOT_FOUND, "page not found");
        }
        Err(error) => return store_failure("cookbook_page_image", &error),
    };

    let accepts_avif = request_headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("image/avif"));
    if accepts_avif {
        if let (Some(assets), Some(image_hash)) = (state.assets, image.image_hash.clone()) {
            let source_path = PathBuf::from(&image.image_path);
            let derivative_hash = image_hash.clone();
            match tokio::task::spawn_blocking(move || {
                assets.avif_derivative(&source_path, &derivative_hash)
            })
            .await
            {
                Ok(Ok(derivative_path)) => {
                    let derivative_etag = format!("\"{image_hash}-avif\"");
                    let matches = request_headers
                        .get(header::IF_NONE_MATCH)
                        .and_then(|value| value.to_str().ok())
                        .is_some_and(|candidate| candidate.contains(&derivative_etag));
                    if matches {
                        return image_response(
                            StatusCode::NOT_MODIFIED,
                            Body::empty(),
                            None,
                            Some(&derivative_etag),
                        );
                    }
                    match read_bounded_file(&derivative_path, IMAGE_DERIVATIVE_MAX_BYTES).await {
                        Ok(bytes) => {
                            return image_response(
                                StatusCode::OK,
                                Body::from(bytes),
                                Some("image/avif"),
                                Some(&derivative_etag),
                            );
                        }
                        Err(error) => tracing::warn!(
                            event = "image_derivative_read_failed",
                            path = %derivative_path.display(),
                            error = %error,
                            "AVIF derivative could not be read; serving the original"
                        ),
                    }
                }
                Ok(Err(error)) => tracing::warn!(
                    event = "image_derivative_unavailable",
                    path = image.image_path,
                    error = %error,
                    "AVIF derivative is unavailable; serving the original"
                ),
                Err(error) => tracing::error!(
                    event = "image_derivative_task_failed",
                    error = %error,
                    "AVIF derivative task failed; serving the original"
                ),
            }
        }
    }

    let etag = image
        .image_hash
        .as_deref()
        .map(|hash| format!("\"{hash}\""));
    if let Some(etag) = etag.as_deref() {
        let matches = request_headers
            .get(header::IF_NONE_MATCH)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|candidate| candidate.contains(etag));
        if matches {
            return image_response(StatusCode::NOT_MODIFIED, Body::empty(), None, Some(etag));
        }
    }

    let metadata = match tokio::fs::metadata(&image.image_path).await {
        Ok(metadata) if metadata.is_file() && metadata.len() <= MAX_PAGE_IMAGE_BYTES => metadata,
        Ok(_) | Err(_) => {
            return json_error(StatusCode::NOT_FOUND, "page image not available");
        }
    };
    debug_assert!(metadata.is_file());
    let bytes = match tokio::fs::read(&image.image_path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            tracing::warn!(
                event = "page_image_read_failed",
                path = image.image_path,
                error = %error,
                "page image is unavailable"
            );
            return json_error(StatusCode::NOT_FOUND, "page image not available");
        }
    };
    image_response(
        StatusCode::OK,
        Body::from(bytes),
        Some(image_content_type(&image.image_path)),
        etag.as_deref(),
    )
}

async fn store_read<T, F>(
    store: Arc<dyn ReadStore>,
    operation: &'static str,
    read: F,
) -> Result<T, StoreError>
where
    T: Send + 'static,
    F: FnOnce(&dyn ReadStore) -> Result<T, StoreError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || read(store.as_ref()))
        .await
        .map_err(|error| {
            tracing::error!(
                event = "store_read_task_failed",
                operation,
                error = %error,
                "store read task failed"
            );
            StoreError::Unavailable(error.to_string())
        })?
}

async fn store_write<T, F>(
    store: Arc<dyn WriteStore>,
    operation: &'static str,
    write: F,
) -> Result<T, StoreError>
where
    T: Send + 'static,
    F: FnOnce(&dyn WriteStore) -> Result<T, StoreError> + Send + 'static,
{
    tokio::task::spawn_blocking(move || write(store.as_ref()))
        .await
        .map_err(|error| {
            tracing::error!(
                event = "store_write_task_failed",
                operation,
                error = %error,
                "store write task failed"
            );
            StoreError::Unavailable(error.to_string())
        })?
}

async fn parse_json_body<T: DeserializeOwned>(request: Request) -> Result<T, Response> {
    parse_json_body_limit(request, MAX_JSON_BODY_BYTES).await
}

async fn parse_json_body_limit<T: DeserializeOwned>(
    request: Request,
    limit: usize,
) -> Result<T, Response> {
    let bytes = to_bytes(request.into_body(), limit)
        .await
        .map_err(|error| {
            tracing::warn!(
                event = "request_body_read_failed",
                error = %error,
                "request body could not be read"
            );
            json_error(StatusCode::BAD_REQUEST, "invalid request")
        })?;
    if bytes.is_empty() {
        return Err(json_error(StatusCode::BAD_REQUEST, "invalid request"));
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        tracing::warn!(
            event = "request_json_invalid",
            error = %error,
            "request JSON is invalid"
        );
        json_error(StatusCode::BAD_REQUEST, "invalid request")
    })
}

async fn read_bounded_file(path: &FilePath, limit: u64) -> Result<Vec<u8>, std::io::Error> {
    let metadata = tokio::fs::metadata(path).await?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file is missing or exceeds its serving limit",
        ));
    }
    tokio::fs::read(path).await
}

#[derive(Debug, thiserror::Error)]
enum ArchiveUploadError {
    #[error("archive body exceeds its limit")]
    TooLarge,
    #[error("archive body length does not match content-length")]
    LengthMismatch,
    #[error("archive body stream failed: {0}")]
    Body(String),
    #[error("archive file I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

async fn stream_archive_body(
    request: Request,
    path: &FilePath,
    expected_length: u64,
) -> Result<u64, ArchiveUploadError> {
    let mut file = tokio::fs::File::create(path).await?;
    let mut stream = request.into_body().into_data_stream();
    let mut total = 0_u64;
    let result = async {
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|error| ArchiveUploadError::Body(error.to_string()))?;
            let chunk_length =
                u64::try_from(chunk.len()).map_err(|_| ArchiveUploadError::TooLarge)?;
            total = total
                .checked_add(chunk_length)
                .ok_or(ArchiveUploadError::TooLarge)?;
            if total > MAX_ARCHIVE_BYTES || total > expected_length {
                return Err(ArchiveUploadError::TooLarge);
            }
            file.write_all(&chunk).await?;
        }
        if total != expected_length {
            return Err(ArchiveUploadError::LengthMismatch);
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok(total)
    }
    .await;
    if result.is_err() {
        drop(file);
        let _ = tokio::fs::remove_file(path).await;
    }
    result
}

fn asset_service_unavailable(operation: &'static str) -> Response {
    tracing::warn!(
        event = "asset_service_unavailable",
        operation,
        "asset operation attempted without configured asset storage"
    );
    json_error(StatusCode::SERVICE_UNAVAILABLE, "asset storage unavailable")
}

fn pipeline_service_unavailable(operation: &'static str) -> Response {
    tracing::warn!(
        event = "pipeline_service_unavailable",
        operation,
        "pipeline operation attempted without configured workers"
    );
    json_error(StatusCode::SERVICE_UNAVAILABLE, "pipeline unavailable")
}

fn pipeline_failure(operation: &'static str, error: &PipelineError) -> Response {
    tracing::error!(
        event = "pipeline_operation_failed",
        operation,
        error = %error,
        "pipeline operation failed"
    );
    match error {
        PipelineError::LlmNotConfigured => json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "LLM is not configured",
        ),
        PipelineError::OcrProducedNoText => {
            json_error(StatusCode::BAD_REQUEST, "OCR produced no text")
        }
        PipelineError::Canceled => json_error(StatusCode::CONFLICT, "processing was canceled"),
        _ => json_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "pipeline processing failed",
        ),
    }
}

fn job_registry_failure(operation: &'static str, error: JobRegistryError) -> Response {
    tracing::error!(
        event = "job_registry_failed",
        operation,
        error = %error,
        "background job registry failed"
    );
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "job registry unavailable",
    )
}

fn asset_failure(operation: &'static str, error: &AssetError) -> Response {
    tracing::error!(
        event = "asset_operation_failed",
        operation,
        error = %error,
        "asset operation failed"
    );
    json_error(StatusCode::INTERNAL_SERVER_ERROR, "asset operation failed")
}

fn blocking_task_failure(operation: &'static str, error: &tokio::task::JoinError) -> Response {
    tracing::error!(
        event = "blocking_task_failed",
        operation,
        error = %error,
        "blocking task failed"
    );
    json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        "request processing failed",
    )
}

fn archive_asset_failure(error: &AssetError) -> Response {
    tracing::warn!(
        event = "cookbook_archive_invalid",
        error = %error,
        "cookbook archive was rejected"
    );
    match error {
        AssetError::EmptyArchive => {
            json_error(StatusCode::BAD_REQUEST, "archive contains no images")
        }
        AssetError::UnsafeArchivePath => {
            json_error(StatusCode::BAD_REQUEST, "archive contains an unsafe path")
        }
        AssetError::DuplicateImage => {
            json_error(StatusCode::CONFLICT, "duplicate cookbook page image")
        }
        AssetError::ArchiveTooLarge | AssetError::ImageTooLarge | AssetError::TooManyEntries => {
            json_error(
                StatusCode::PAYLOAD_TOO_LARGE,
                "cookbook archive is too large",
            )
        }
        AssetError::InvalidArchive | AssetError::UnsupportedArchiveEntry => {
            json_error(StatusCode::BAD_REQUEST, "could not inspect archive")
        }
        _ => asset_failure("ingest_cookbook_archive", error),
    }
}

async fn parse_default_json_body<T>(request: Request) -> Result<T, Response>
where
    T: Default + DeserializeOwned,
{
    let bytes = to_bytes(request.into_body(), MAX_JSON_BODY_BYTES)
        .await
        .map_err(|error| {
            tracing::warn!(
                event = "request_body_read_failed",
                error = %error,
                "request body could not be read"
            );
            json_error(StatusCode::BAD_REQUEST, "invalid request")
        })?;
    if bytes.is_empty() {
        return Ok(T::default());
    }
    serde_json::from_slice(&bytes).map_err(|error| {
        tracing::warn!(
            event = "request_json_invalid",
            error = %error,
            "request JSON is invalid"
        );
        json_error(StatusCode::BAD_REQUEST, "invalid request")
    })
}

fn store_failure(operation: &'static str, error: &StoreError) -> Response {
    tracing::error!(
        event = "store_read_failed",
        operation,
        error = %error,
        "read-only store operation failed"
    );
    json_error(StatusCode::SERVICE_UNAVAILABLE, "database unavailable")
}

fn write_store_unavailable(operation: &'static str) -> Response {
    tracing::warn!(
        event = "store_write_unavailable",
        operation,
        "write attempted while the Rust store is read-only"
    );
    json_error(StatusCode::SERVICE_UNAVAILABLE, "database unavailable")
}

fn ok_response() -> Response {
    (StatusCode::OK, Json(OkResponse { ok: true })).into_response()
}

fn json_error(status: StatusCode, error: &'static str) -> Response {
    (status, Json(ErrorResponse { error })).into_response()
}

fn image_response(
    status: StatusCode,
    body: Body,
    content_type: Option<&'static str>,
    etag: Option<&str>,
) -> Response {
    let mut response = (status, body).into_response();
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    }
    if let Some(etag) = etag.and_then(|value| HeaderValue::from_str(value).ok()) {
        response.headers_mut().insert(header::ETAG, etag);
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("private, no-cache"),
        );
    }
    response
}

fn image_content_type(path: &str) -> &'static str {
    match FilePath::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        _ => "image/jpeg",
    }
}

async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(ErrorResponse { error: "not found" }),
    )
        .into_response()
}

async fn cors(request: Request, next: Next) -> Response {
    let mut response = if request.method() == Method::OPTIONS {
        StatusCode::NO_CONTENT.into_response()
    } else {
        next.run(request).await
    };
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, PUT, PATCH, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("content-type"),
    );
    response
}

async fn request_log(request: Request, next: Next) -> Response {
    let started = Instant::now();
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    tracing::info!(
        event = "http_request_complete",
        method = %method,
        path,
        status = response.status().as_u16(),
        elapsed_ms,
        "request completed"
    );
    response
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::{
        body::{Body, to_bytes},
        http::Request as HttpRequest,
    };
    use tower::ServiceExt;

    use crate::{
        model::{
            Catalogue, CookLogEntry, CookbookContentBlock, CookbookPageText, MealPlanEntry,
            PantryItem,
        },
        store::{CookbookPageImage, ReadStore, StoreError, StoreProbe},
    };

    use super::*;

    struct Probe(Result<(), &'static str>);

    impl StoreProbe for Probe {
        fn ping(&self) -> Result<(), StoreError> {
            self.0
                .map_err(|message| StoreError::Unavailable(message.to_owned()))
        }
    }

    impl ReadStore for Probe {
        fn catalogue_summary(&self) -> Result<Catalogue, StoreError> {
            Ok(Catalogue::default())
        }

        fn pantry_items(&self) -> Result<Vec<PantryItem>, StoreError> {
            Ok(Vec::new())
        }

        fn meal_plan_entries(&self) -> Result<Vec<MealPlanEntry>, StoreError> {
            Ok(Vec::new())
        }

        fn cook_log_entries(&self) -> Result<Vec<CookLogEntry>, StoreError> {
            Ok(Vec::new())
        }

        fn cookbook_page_text(&self, _page_id: &str) -> Result<CookbookPageText, StoreError> {
            Err(StoreError::CookbookPageNotFound)
        }

        fn cookbook_page_image(&self, _page_id: &str) -> Result<CookbookPageImage, StoreError> {
            Err(StoreError::CookbookPageNotFound)
        }

        fn cookbook_content_blocks(
            &self,
            _cookbook_id: &str,
        ) -> Result<Vec<CookbookContentBlock>, StoreError> {
            Ok(Vec::new())
        }
    }

    fn app(probe: Probe) -> Router {
        router(AppState::new(Arc::new(probe)))
    }

    fn app_with_store(store: Arc<dyn ReadStore>) -> Router {
        router(AppState::new(store))
    }

    async fn body(response: Response) -> String {
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body");
        String::from_utf8(bytes.to_vec()).expect("UTF-8 response")
    }

    struct FailingReadStore;

    impl StoreProbe for FailingReadStore {
        fn ping(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    impl ReadStore for FailingReadStore {
        fn catalogue_summary(&self) -> Result<Catalogue, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn pantry_items(&self) -> Result<Vec<PantryItem>, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn meal_plan_entries(&self) -> Result<Vec<MealPlanEntry>, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn cook_log_entries(&self) -> Result<Vec<CookLogEntry>, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn cookbook_page_text(&self, _page_id: &str) -> Result<CookbookPageText, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn cookbook_page_image(&self, _page_id: &str) -> Result<CookbookPageImage, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }

        fn cookbook_content_blocks(
            &self,
            _cookbook_id: &str,
        ) -> Result<Vec<CookbookContentBlock>, StoreError> {
            Err(StoreError::Unavailable("fixture unavailable".to_owned()))
        }
    }

    struct ImageStore {
        image_path: String,
    }

    impl StoreProbe for ImageStore {
        fn ping(&self) -> Result<(), StoreError> {
            Ok(())
        }
    }

    impl ReadStore for ImageStore {
        fn catalogue_summary(&self) -> Result<Catalogue, StoreError> {
            Ok(Catalogue::default())
        }

        fn pantry_items(&self) -> Result<Vec<PantryItem>, StoreError> {
            Ok(Vec::new())
        }

        fn meal_plan_entries(&self) -> Result<Vec<MealPlanEntry>, StoreError> {
            Ok(Vec::new())
        }

        fn cook_log_entries(&self) -> Result<Vec<CookLogEntry>, StoreError> {
            Ok(Vec::new())
        }

        fn cookbook_page_text(&self, _page_id: &str) -> Result<CookbookPageText, StoreError> {
            Err(StoreError::CookbookPageNotFound)
        }

        fn cookbook_page_image(&self, page_id: &str) -> Result<CookbookPageImage, StoreError> {
            if page_id != "page-1" {
                return Err(StoreError::CookbookPageNotFound);
            }
            Ok(CookbookPageImage {
                image_path: self.image_path.clone(),
                image_hash: Some("fixture-hash".to_owned()),
            })
        }

        fn cookbook_content_blocks(
            &self,
            _cookbook_id: &str,
        ) -> Result<Vec<CookbookContentBlock>, StoreError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn health_matches_the_zig_contract() {
        let response = app(Probe(Ok(())))
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/health?ignored=true")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(body(response).await, r#"{"ok":true}"#);
    }

    #[tokio::test]
    async fn unhealthy_store_is_a_service_unavailable_response() {
        let response = app(Probe(Err("offline")))
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            body(response).await,
            r#"{"ok":false,"error":"database unavailable"}"#
        );
    }

    #[tokio::test]
    async fn unknown_routes_keep_the_existing_error_shape() {
        let response = app(Probe(Ok(())))
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/not-real")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(body(response).await, r#"{"error":"not found"}"#);
    }

    #[tokio::test]
    async fn options_are_handled_for_every_path() {
        let response = app(Probe(Ok(())))
            .oneshot(
                HttpRequest::builder()
                    .method(Method::OPTIONS)
                    .uri("/api/future-route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS],
            "GET, POST, PUT, PATCH, DELETE, OPTIONS"
        );
    }

    #[tokio::test]
    async fn phase_2_read_routes_keep_the_expected_empty_and_missing_shapes() {
        let application = app(Probe(Ok(())));

        let catalogue = application
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/catalogue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(catalogue.status(), StatusCode::OK);
        let catalogue_json = serde_json::from_str::<serde_json::Value>(&body(catalogue).await)
            .expect("catalogue JSON");
        assert_eq!(catalogue_json["currentUserId"], serde_json::Value::Null);
        assert_eq!(catalogue_json["recipes"], serde_json::json!([]));
        assert_eq!(
            catalogue_json.as_object().expect("catalogue object").len(),
            15
        );

        let pantry = application
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/pantry")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(pantry.status(), StatusCode::OK);
        assert_eq!(body(pantry).await, "[]");

        for path in ["/api/meal-plan", "/api/cook-log"] {
            let response = application
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(body(response).await, "[]");
        }

        let blocks = application
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/cookbooks/missing/blocks")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(blocks.status(), StatusCode::OK);
        assert_eq!(body(blocks).await, "[]");

        for path in [
            "/api/cookbook-pages/missing/text",
            "/api/cookbook-pages/missing/image",
        ] {
            let response = application
                .clone()
                .oneshot(
                    HttpRequest::builder()
                        .uri(path)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert_eq!(body(response).await, r#"{"error":"page not found"}"#);
        }
    }

    #[tokio::test]
    async fn store_failures_are_bounded_service_unavailable_responses() {
        let response = app_with_store(Arc::new(FailingReadStore))
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/catalogue")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body(response).await, r#"{"error":"database unavailable"}"#);
    }

    #[tokio::test]
    async fn mutations_are_unavailable_when_the_store_is_read_only() {
        let response = app(Probe(Ok(())))
            .oneshot(
                HttpRequest::builder()
                    .method(Method::POST)
                    .uri("/api/pantry")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(
                        r#"{"item":"rice","displayName":"Rice","category":"raw"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body(response).await, r#"{"error":"database unavailable"}"#);
    }

    #[tokio::test]
    async fn page_images_use_content_type_etag_and_conditional_get() {
        let directory = tempfile::tempdir().expect("temporary image directory");
        let image_path = directory.path().join("page.png");
        fs::write(&image_path, b"fixture-png").expect("write fixture image");
        let application = app_with_store(Arc::new(ImageStore {
            image_path: image_path.to_string_lossy().into_owned(),
        }));

        let response = application
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/cookbook-pages/page-1/image")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
        assert_eq!(response.headers()[header::ETAG], "\"fixture-hash\"");
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "private, no-cache"
        );
        let image_bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("image response body");
        assert_eq!(&image_bytes[..], b"fixture-png");

        let not_modified = application
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/cookbook-pages/page-1/image")
                    .header(header::IF_NONE_MATCH, "\"fixture-hash\"")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(not_modified.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(not_modified.headers()[header::ETAG], "\"fixture-hash\"");
        assert!(
            to_bytes(not_modified.into_body(), usize::MAX)
                .await
                .expect("304 response body")
                .is_empty()
        );
    }
}
