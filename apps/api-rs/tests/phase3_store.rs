#![cfg(feature = "duckdb-store")]

use duckdb::Connection;
use recitopia_api_rs::{
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    model::{
        Cookbook, Ingredient, IngredientQuantityKind, IngredientQuantityReviewStatus,
        InstructionStep, MarkMadeInput, MealPlanEntry, MealType, PantryCategory, PantryItem,
        PantryItemPatch, Recipe, RecipeExtractionStatus, ShareScope, SubstitutionInput,
    },
    store::{ReadStore, StoreError, WriteStore},
};

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");
const CACHE_TIME: &str = "2026-07-10T08:00:00.000Z";

fn fixture_store() -> (tempfile::TempDir, DuckStore) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase3.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 3 fixture");
    }
    let store = DuckStore::open(&DatabaseConfig {
        path,
        mode: StoreMode::ReadWrite,
    })
    .expect("open fixture read-write");
    (directory, store)
}

fn recipe(id: &str, title: &str) -> Recipe {
    Recipe {
        id: id.to_owned(),
        title: title.to_owned(),
        subtitle: None,
        alternate_names: Vec::new(),
        cookbook_id: "our-korean-kitchen".to_owned(),
        author_ids: vec!["author-1".to_owned()],
        page_start: Some(40),
        page_end: Some(41),
        source_label: "Our Korean Kitchen, p. 40".to_owned(),
        headnote: Some("A compact transaction fixture.".to_owned()),
        serving_context: None,
        yield_quantity: Some(4.0),
        yield_unit: Some("servings".to_owned()),
        prep_minutes: Some(10),
        cook_minutes: Some(35),
        total_minutes: None,
        cuisine: Some("Korean".to_owned()),
        category: Some("Rice".to_owned()),
        tags: vec!["test".to_owned()],
        searchable_text: String::new(),
        source_block_id: None,
        source_page_spans: Vec::new(),
        component_recipe_ids: Vec::new(),
        pictured_page_number: None,
        extraction_status: RecipeExtractionStatus::Verified,
        images: Vec::new(),
        ingredients: vec![
            Ingredient {
                id: "ingredient-a".to_owned(),
                position: Some(1),
                display_name: "250g rice".to_owned(),
                item: "rice".to_owned(),
                quantity: Some(0.25),
                quantity_text: Some("250g".to_owned()),
                quantity_min: None,
                quantity_max: None,
                quantity_kind: IngredientQuantityKind::Exact,
                quantity_review_status: IngredientQuantityReviewStatus::Parsed,
                quantity_review_reason: None,
                unit: Some("kg".to_owned()),
                preparation: None,
                section: None,
                optional: false,
                alternative_text: None,
                source_line: None,
                source_page_id: None,
                unit_cost_cents: Some(300),
                estimated_cost_cents: None,
            },
            Ingredient {
                id: "ingredient-b".to_owned(),
                position: Some(2),
                display_name: "seasoning".to_owned(),
                item: "seasoning".to_owned(),
                quantity: None,
                quantity_text: None,
                quantity_min: None,
                quantity_max: None,
                quantity_kind: IngredientQuantityKind::AsNeeded,
                quantity_review_status: IngredientQuantityReviewStatus::Parsed,
                quantity_review_reason: None,
                unit: None,
                preparation: None,
                section: None,
                optional: false,
                alternative_text: None,
                source_line: None,
                source_page_id: None,
                unit_cost_cents: None,
                estimated_cost_cents: Some(75),
            },
        ],
        steps: vec![InstructionStep {
            id: "step-a".to_owned(),
            position: 1,
            section: None,
            text: "Cook until tender.".to_owned(),
            source_page_id: None,
            source_line_start: None,
            source_line_end: None,
        }],
        notes: Vec::new(),
        last_made_at: None,
        times_made: 0,
        cost_cents: None,
        cost_per_serving_cents: None,
        cache_key: "uncached".to_owned(),
        cache_updated_at: None,
    }
}

#[test]
fn pantry_and_meal_plan_writes_round_trip() {
    let (_directory, store) = fixture_store();

    let item = store
        .add_pantry_item(PantryItem {
            id: "pantry-new".to_owned(),
            item: "gochugaru".to_owned(),
            display_name: "Gochugaru".to_owned(),
            quantity: Some(250.0),
            unit: Some("g".to_owned()),
            category: PantryCategory::Raw,
            source_recipe_id: None,
            notes: None,
            expires_at: None,
            added_at: CACHE_TIME.to_owned(),
            owner_user_id: None,
            family_id: None,
        })
        .expect("add pantry item");
    assert_eq!(item.owner_user_id.as_deref(), Some("avery-river"));
    assert_eq!(item.family_id.as_deref(), Some("river-house"));

    let patched = store
        .patch_pantry_item(
            "pantry-new",
            PantryItemPatch {
                quantity: Some(200.0),
                category: Some(PantryCategory::Prepared),
                notes: Some("Toasted".to_owned()),
                ..PantryItemPatch::default()
            },
        )
        .expect("patch pantry item");
    assert_eq!(patched.quantity, Some(200.0));
    assert_eq!(patched.category, PantryCategory::Prepared);
    assert_eq!(patched.notes.as_deref(), Some("Toasted"));
    assert!(matches!(
        store.patch_pantry_item("missing", PantryItemPatch::default()),
        Err(StoreError::PantryItemNotFound)
    ));

    let meal = store
        .add_meal_plan_entry(MealPlanEntry {
            id: "meal-plan-new".to_owned(),
            date: "2026-07-12".to_owned(),
            meal_type: MealType::Lunch,
            recipe_id: "recipe-1".to_owned(),
            servings: Some(2.0),
            notes: Some("Pack leftovers".to_owned()),
            owner_user_id: None,
            family_id: None,
        })
        .expect("add meal plan entry");
    assert_eq!(meal.meal_type, MealType::Lunch);
    assert_eq!(store.meal_plan_entries().unwrap().len(), 2);

    store
        .delete_meal_plan_entry("meal-plan-new")
        .expect("delete meal entry");
    store
        .delete_pantry_item("pantry-new")
        .expect("delete pantry item");
    store
        .delete_pantry_item("missing")
        .expect("unknown pantry delete is idempotent");
    assert_eq!(store.meal_plan_entries().unwrap().len(), 1);
    assert_eq!(store.pantry_items().unwrap().len(), 1);
}

#[test]
fn cookbook_creation_validates_conflicts_and_rolls_back_child_failures() {
    let (_directory, store) = fixture_store();
    let cookbook = Cookbook {
        id: "phase-three-book".to_owned(),
        title: "Phase Three Book".to_owned(),
        author_ids: vec!["author-1".to_owned()],
        isbn: None,
        publisher: None,
        published_year: Some(2026),
        cover_image_url: None,
        owner_user_id: None,
        family_id: None,
        share_scope: ShareScope::Family,
        shared_with_user_ids: vec!["shared-user".to_owned()],
    };

    let created = store
        .create_cookbook(cookbook.clone())
        .expect("create cookbook");
    assert_eq!(created.owner_user_id.as_deref(), Some("avery-river"));
    assert_eq!(created.family_id.as_deref(), Some("river-house"));
    assert!(matches!(
        store.create_cookbook(cookbook),
        Err(StoreError::CookbookAlreadyExists)
    ));

    let mut invalid = created.clone();
    invalid.id = "Invalid_ID".to_owned();
    assert!(matches!(
        store.create_cookbook(invalid),
        Err(StoreError::InvalidCookbook)
    ));

    let mut rollback = created;
    rollback.id = "rollback-book".to_owned();
    rollback.title = "Must Roll Back".to_owned();
    rollback.author_ids = vec!["author-1".to_owned(), "author-1".to_owned()];
    assert!(matches!(
        store.create_cookbook(rollback),
        Err(StoreError::Backend { .. })
    ));
    assert!(
        store
            .catalogue_summary()
            .unwrap()
            .cookbooks
            .iter()
            .all(|cookbook| cookbook.id != "rollback-book")
    );
}

#[test]
fn recipe_lifecycle_recomputes_history_and_preserves_historical_references() {
    let (_directory, store) = fixture_store();
    let created = store
        .create_recipe(recipe("recipe-new", "Transaction Rice"), CACHE_TIME)
        .expect("create recipe");
    assert_eq!(created.total_minutes, Some(45));
    assert_eq!(created.cost_cents, Some(150));
    assert_eq!(created.cost_per_serving_cents, Some(38));
    assert_eq!(created.searchable_text, "Transaction Rice");
    assert_ne!(created.cache_key, "uncached");
    assert!(matches!(
        store.create_recipe(recipe("recipe-new", "Duplicate"), CACHE_TIME),
        Err(StoreError::RecipeAlreadyExists)
    ));

    store
        .add_meal_plan_entry(MealPlanEntry {
            id: "meal-plan-recipe-new".to_owned(),
            date: "2026-07-13".to_owned(),
            meal_type: MealType::Dinner,
            recipe_id: "recipe-new".to_owned(),
            servings: Some(4.0),
            notes: None,
            owner_user_id: None,
            family_id: None,
        })
        .expect("plan new recipe");

    let made_at = "2026-07-10T09:00:00.000Z";
    let made = store
        .mark_recipe_made(
            "recipe-new",
            made_at,
            MarkMadeInput {
                servings_made: Some(4.0),
                servings_eaten: Some(2.0),
                leftover_servings: Some(2.0),
                notes: Some("Good texture".to_owned()),
                substitutions: vec![SubstitutionInput {
                    ingredient_id: "ingredient-a".to_owned(),
                    original_item: "rice".to_owned(),
                    substitute_text: "brown rice".to_owned(),
                }],
                ..MarkMadeInput::default()
            },
            CACHE_TIME,
        )
        .expect("mark recipe made");
    assert_eq!(made.last_made_at.as_deref(), Some(made_at));
    assert_eq!(made.times_made, 1);
    assert!(
        store
            .cook_log_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.recipe_id == "recipe-new" && entry.substitutions.len() == 1)
    );
    assert!(
        store
            .pantry_items()
            .unwrap()
            .iter()
            .any(|item| item.source_recipe_id.as_deref() == Some("recipe-new"))
    );

    let mut update = recipe("ignored-body-id", "Updated Transaction Rice");
    update.id = "recipe-new".to_owned();
    update.times_made = 0;
    update.last_made_at = None;
    let updated = store
        .update_recipe(update, CACHE_TIME)
        .expect("update recipe");
    assert_eq!(updated.times_made, 1);
    assert_eq!(updated.last_made_at.as_deref(), Some(made_at));

    store.delete_recipe("recipe-new").expect("delete recipe");
    assert!(
        store
            .meal_plan_entries()
            .unwrap()
            .iter()
            .all(|entry| entry.recipe_id != "recipe-new")
    );
    assert!(
        store
            .cook_log_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.recipe_id == "recipe-new")
    );
    assert!(
        store
            .pantry_items()
            .unwrap()
            .iter()
            .any(|item| item.source_recipe_id.as_deref() == Some("recipe-new"))
    );
    assert!(matches!(
        store.delete_recipe("recipe-new"),
        Err(StoreError::RecipeNotFound)
    ));
}

#[test]
fn invalid_recipes_are_rejected_and_child_constraint_failures_roll_back() {
    let (_directory, store) = fixture_store();
    let mut invalid = recipe("invalid-recipe", "Invalid");
    invalid.ingredients.clear();
    assert!(matches!(
        store.create_recipe(invalid, CACHE_TIME),
        Err(StoreError::InvalidRecipe(_))
    ));

    store
        .create_recipe(recipe("rollback-recipe", "Original Title"), CACHE_TIME)
        .expect("create rollback recipe");
    let mut update = recipe("rollback-recipe", "Changed Title");
    let duplicate_id = update.ingredients[0].id.clone();
    update.ingredients[1].id = duplicate_id;
    assert!(matches!(
        store.update_recipe(update, CACHE_TIME),
        Err(StoreError::Backend { .. })
    ));

    let persisted = store
        .catalogue_summary()
        .unwrap()
        .recipes
        .into_iter()
        .find(|recipe| recipe.id == "rollback-recipe")
        .expect("rollback recipe remains");
    assert_eq!(persisted.title, "Original Title");
    assert_eq!(persisted.ingredients.len(), 2);
}
