#![cfg(feature = "duckdb-store")]

use duckdb::Connection;
use recitopia_api_rs::{
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    model::{
        CookbookContentBlockKind, CookbookPageKind, CookbookPageReviewStatus, CookbookSectionKind,
        IngredientQuantityKind, IngredientQuantityReviewStatus, PantryCategory,
        RecipeExtractionStatus, ShareScope,
    },
    store::{ReadStore, StoreError},
};

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");

fn fixture_store() -> (tempfile::TempDir, DuckStore) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase2.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 2 fixture");
    }
    let store = DuckStore::open(&DatabaseConfig {
        path,
        mode: StoreMode::ReadOnly,
    })
    .expect("open fixture read-only");
    (directory, store)
}

#[test]
fn reads_the_complete_phase_2_catalogue_contract() {
    let (_directory, store) = fixture_store();
    let catalogue = store.catalogue_summary().expect("read catalogue");

    assert_eq!(catalogue.current_user_id.as_deref(), Some("avery-river"));
    assert_eq!(catalogue.families.len(), 1);
    assert_eq!(catalogue.users.len(), 2);
    assert_eq!(catalogue.authors.len(), 1);

    let cookbook = &catalogue.cookbooks[0];
    assert_eq!(cookbook.id, "our-korean-kitchen");
    assert_eq!(cookbook.author_ids, ["author-1"]);
    assert_eq!(cookbook.share_scope, ShareScope::Family);
    assert_eq!(cookbook.shared_with_user_ids, ["shared-user"]);

    let recipe = &catalogue.recipes[0];
    assert_eq!(recipe.title, "Short-grain Rice");
    assert_eq!(recipe.alternate_names[0].value, "bap");
    assert_eq!(recipe.author_ids, ["author-1"]);
    assert_eq!(recipe.tags, ["rice", "staple"]);
    assert_eq!(recipe.source_page_spans[0].printed_page_number, Some(26));
    assert_eq!(recipe.component_recipe_ids, ["component-stock"]);
    assert_eq!(
        recipe.extraction_status,
        RecipeExtractionStatus::NeedsReview
    );
    assert_eq!(recipe.images[0].id, "image-1");
    assert_eq!(recipe.steps[0].source_line_start, Some(12));
    assert_eq!(recipe.notes[0].id, "note-1");

    let ingredient = &recipe.ingredients[0];
    assert_eq!(ingredient.quantity_text.as_deref(), Some("?00g"));
    assert_eq!(ingredient.quantity_kind, IngredientQuantityKind::Unknown);
    assert_eq!(
        ingredient.quantity_review_status,
        IngredientQuantityReviewStatus::NeedsReview
    );
    assert_eq!(
        ingredient.quantity_review_reason.as_deref(),
        Some("OCR may have lost a leading digit")
    );

    let page = &catalogue.cookbook_pages[0];
    assert_eq!(page.page_kind, CookbookPageKind::Recipe);
    assert_eq!(page.review_status, CookbookPageReviewStatus::Accepted);
    assert_eq!(page.ocr_text.len(), 420);
    assert_eq!(page.ocr_json, "{}");
    assert!(page.has_ocr_text);

    let section = &catalogue.cookbook_sections[0];
    assert_eq!(section.kind, CookbookSectionKind::Chapter);
    assert_eq!((section.page_start, section.page_end), (Some(25), Some(51)));

    let block = &catalogue.cookbook_content_blocks[0];
    assert_eq!(block.kind, CookbookContentBlockKind::RecipeHeadnote);
    assert_eq!(block.text.len(), 420);
    assert_eq!(block.source_json, "{}");
    assert!(block.has_text);

    assert_eq!(catalogue.cookbook_menus[0].recipes[0].recipe_id, "recipe-1");
    assert_eq!(
        catalogue.cookbook_glossary_entries[0].aliases,
        ["chilli paste"]
    );
    assert_eq!(catalogue.cookbook_suppliers[0].source_page, Some(265));
    assert_eq!(
        catalogue.cookbook_index_entries[0]
            .target_recipe_id
            .as_deref(),
        Some("recipe-1")
    );
    assert_eq!(
        catalogue.cookbook_cross_references[0].to_id.as_deref(),
        Some("page-26")
    );
}

#[test]
fn reads_full_page_block_image_and_pantry_projections() {
    let (_directory, store) = fixture_store();

    let page = store.cookbook_page_text("page-26").expect("read page text");
    assert!(page.ocr_text.len() > 420);
    assert_eq!(page.ocr_json, r#"{"blocks":[{"text":"Short-grain Rice"}]}"#);

    let image = store
        .cookbook_page_image("page-26")
        .expect("read page image metadata");
    assert_eq!(image.image_path, "/tmp/recitopia-phase2-page.jpg");
    assert_eq!(image.image_hash.as_deref(), Some("phase2-image-hash"));

    let blocks = store
        .cookbook_content_blocks("our-korean-kitchen")
        .expect("read full blocks");
    assert_eq!(blocks[0].text.len(), 430);
    assert_eq!(blocks[0].source_json, r#"{"sourcePageIds":["page-26"]}"#);
    assert!(
        store
            .cookbook_content_blocks("missing-cookbook")
            .expect("unknown cookbook is empty")
            .is_empty()
    );

    let pantry = store.pantry_items().expect("read pantry");
    assert_eq!(pantry[0].category, PantryCategory::Raw);
    assert_eq!(pantry[0].owner_user_id.as_deref(), Some("avery-river"));
    assert_eq!(pantry[0].family_id.as_deref(), Some("river-house"));

    let meal_plan = store.meal_plan_entries().expect("read meal plan");
    assert_eq!(meal_plan[0].id, "meal-plan-initial");
    assert_eq!(meal_plan[0].recipe_id, "recipe-1");

    let cook_log = store.cook_log_entries().expect("read cook log");
    assert_eq!(cook_log[0].id, "cook-log-initial");
    assert_eq!(cook_log[0].substitutions[0].id, "sub-initial");

    assert!(matches!(
        store.cookbook_page_text("missing-page"),
        Err(StoreError::CookbookPageNotFound)
    ));
}

#[test]
fn rejects_invalid_embedded_json_instead_of_returning_partial_data() {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("invalid-json.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 2 fixture");
        connection
            .execute(
                "update cookbook_glossary_entries set aliases_json = $1",
                ["not-json"],
            )
            .expect("damage fixture JSON");
    }
    let store = DuckStore::open(&DatabaseConfig {
        path,
        mode: StoreMode::ReadOnly,
    })
    .expect("open fixture read-only");

    assert!(matches!(
        store.catalogue_summary(),
        Err(StoreError::InvalidJson {
            context: "cookbook_glossary_entries.aliases_json",
            ..
        })
    ));
}
