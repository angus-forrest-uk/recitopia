#![cfg(feature = "duckdb-store")]

use duckdb::Connection;
use recitopia_api_rs::{
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    model::{
        CookbookContentBlock, CookbookContentBlockKind, CookbookImportProgress,
        CookbookImportStatus, CookbookSection, CookbookSectionKind, ImportJobState,
        ImportPipelineStage, RecipeImport, RecipeImportStatus,
    },
    store::{CookbookPipelineResult, ReadStore, StoreError, WriteStore},
};

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");
const NOW: &str = "2026-07-10T16:00:00.000Z";

fn fixture_store() -> (tempfile::TempDir, DuckStore) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase5.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 5 fixture");
    }
    let store = DuckStore::open(&DatabaseConfig {
        path,
        mode: StoreMode::ReadWrite,
    })
    .expect("open fixture read-write");
    (directory, store)
}

#[test]
fn progress_snapshots_survive_registry_loss_and_cancellation_is_persisted() {
    let (_directory, store) = fixture_store();
    let mut progress = CookbookImportProgress::queued("import-1");
    progress.stage = ImportPipelineStage::OcrPages;
    progress.current = Some(1);
    progress.total = Some(4);
    progress.processed_count = 1;
    store
        .upsert_cookbook_import_progress(progress, NOW)
        .expect("persist running progress");

    let stored = store
        .cookbook_import_progress("import-1")
        .expect("reload progress");
    assert_eq!(stored.state, ImportJobState::Running);
    assert_eq!(stored.stage, ImportPipelineStage::OcrPages);
    assert_eq!(stored.current, Some(1));

    let mut canceled = stored;
    canceled.state = ImportJobState::Canceled;
    canceled.stage = ImportPipelineStage::Canceled;
    canceled.message = "Cancellation requested.".to_owned();
    store
        .upsert_cookbook_import_progress(canceled, NOW)
        .expect("persist cancellation");
    assert_eq!(
        store
            .cookbook_import_progress("import-1")
            .expect("reload cancellation")
            .state,
        ImportJobState::Canceled
    );
}

#[test]
fn pipeline_graph_replacement_is_atomic_and_removes_stale_generated_recipes() {
    let (_directory, store) = fixture_store();
    let mut source = store
        .cookbook_pipeline_source("import-1")
        .expect("load full pipeline source");
    assert!(source.pages[0].ocr_text.len() > 420);
    source.import_record.status = CookbookImportStatus::Committed;
    source.import_record.updated_at = NOW.to_owned();
    source.pages[0].ocr_text = "Fresh OCR text".to_owned();
    source.pages[0].has_ocr_text = true;
    let section = CookbookSection {
        id: "import-1-section-new".to_owned(),
        cookbook_id: source.cookbook.id.clone(),
        parent_section_id: None,
        title: "New section".to_owned(),
        kind: CookbookSectionKind::Chapter,
        position: 1,
        page_start: Some(26),
        page_end: Some(26),
    };
    let block = CookbookContentBlock {
        id: "import-1-context-1".to_owned(),
        cookbook_id: source.cookbook.id.clone(),
        section_id: Some(section.id.clone()),
        page_start: Some(26),
        page_end: Some(26),
        position: 1,
        kind: CookbookContentBlockKind::Paragraph,
        title: Some("New context".to_owned()),
        text: "Persisted context".to_owned(),
        has_text: true,
        confidence: None,
        source_json: "{}".to_owned(),
    };
    let mut recipe = store.catalogue_summary().unwrap().recipes.remove(0);
    recipe.id = "phase-five-recipe".to_owned();
    recipe.title = "Phase Five Recipe".to_owned();
    recipe.source_block_id = Some("import-1-recipe-phase-five-recipe".to_owned());
    recipe.last_made_at = None;
    recipe.times_made = 0;

    let persisted = store
        .persist_cookbook_pipeline(
            CookbookPipelineResult {
                import_record: source.import_record.clone(),
                pages: source.pages.clone(),
                sections: vec![section.clone()],
                content_blocks: vec![block],
                recipes: vec![recipe],
            },
            NOW,
        )
        .expect("persist extracted graph");
    assert_eq!(persisted.recipe_count, 1);
    let catalogue = store.catalogue_summary().expect("reload catalogue");
    assert!(
        catalogue
            .recipes
            .iter()
            .any(|recipe| recipe.id == "phase-five-recipe")
    );
    assert!(
        catalogue
            .cookbook_sections
            .iter()
            .any(|item| item.id == section.id)
    );

    store
        .persist_cookbook_pipeline(
            CookbookPipelineResult {
                import_record: source.import_record,
                pages: source.pages,
                sections: vec![section],
                content_blocks: Vec::new(),
                recipes: Vec::new(),
            },
            NOW,
        )
        .expect("replace extracted graph");
    assert!(
        !store
            .catalogue_summary()
            .unwrap()
            .recipes
            .iter()
            .any(|recipe| recipe.id == "phase-five-recipe")
    );
}

#[test]
fn recipe_import_commit_is_atomic_when_recipe_validation_fails() {
    let (_directory, store) = fixture_store();
    let recipe = store.catalogue_summary().unwrap().recipes.remove(0);
    let recipe_import = RecipeImport {
        id: "recipe-import-phase-five".to_owned(),
        status: RecipeImportStatus::DraftReady,
        file_name: "page.jpg".to_owned(),
        mime_type: "image/jpeg".to_owned(),
        image_path: "/tmp/page.jpg".to_owned(),
        ocr_engine: "fixture".to_owned(),
        ocr_text: "recipe OCR".to_owned(),
        ocr_json: "{}".to_owned(),
        draft: Some(recipe.clone()),
        validation_issues: Vec::new(),
        created_at: NOW.to_owned(),
        updated_at: NOW.to_owned(),
    };
    store
        .create_recipe_import(recipe_import)
        .expect("create recipe import");

    let mut invalid = recipe;
    invalid.id = "invalid-phase-five-recipe".to_owned();
    invalid.steps.clear();
    assert!(matches!(
        store.commit_recipe_import("recipe-import-phase-five", invalid, Vec::new(), NOW),
        Err(StoreError::InvalidRecipe(_))
    ));
    assert_eq!(
        store
            .recipe_import("recipe-import-phase-five")
            .expect("recipe import remains")
            .status,
        RecipeImportStatus::DraftReady
    );
}
