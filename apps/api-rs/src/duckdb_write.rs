use std::collections::HashSet;

use duckdb::{Connection, OptionalExt, Params, params};

use crate::{
    duckdb_read,
    model::{
        AcceptPageContentInput, Cookbook, CookbookContentBlock, CookbookContentBlockKind,
        CookbookContentBlockPatch, CookbookCrossReference, CookbookGlossaryEntry, CookbookImport,
        CookbookImportProgress, CookbookImportStatus, CookbookIndexEntry, CookbookMenu,
        CookbookPage, CookbookPageKind, CookbookPagePatch, CookbookPageReviewStatus,
        CookbookSection, CookbookSectionKind, CookbookSourceImport, CookbookSourceKind,
        CookbookSupplier, ImportIssue, ImportJobState, ImportPipelineStage, MarkMadeInput,
        MealPlanEntry, PantryItem, PantryItemPatch, Recipe, RecipeImport, RecipeImportStatus,
        ShareScope,
    },
    runtime::generate_id,
    store::{CookbookExtractionPersistResult, CookbookPipelineResult, StoreError},
};

const DEFAULT_CURRENT_USER_ID: &str = "avery-river";
const DEFAULT_FAMILY_ID: &str = "river-house";

pub(crate) fn create_cookbook(
    conn: &mut Connection,
    cookbook: &Cookbook,
) -> Result<Cookbook, StoreError> {
    if row_exists(conn, "cookbooks", &cookbook.id)? {
        return Err(StoreError::CookbookAlreadyExists);
    }
    if !is_valid_id(&cookbook.id) || cookbook.title.is_empty() || cookbook.author_ids.is_empty() {
        return Err(StoreError::InvalidCookbook);
    }

    let id = cookbook.id.clone();
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook create", &error))?;
    upsert_cookbook(&transaction, cookbook)?;
    transaction
        .commit()
        .map_err(|error| backend("commit cookbook create", &error))?;
    duckdb_read::cookbook_by_id(conn, &id)
}

pub(crate) fn create_recipe(
    conn: &mut Connection,
    recipe: Recipe,
    cache_updated_at: &str,
) -> Result<Recipe, StoreError> {
    if row_exists(conn, "recipes", &recipe.id)? {
        return Err(StoreError::RecipeAlreadyExists);
    }
    let recipe = recipe.recompute(cache_updated_at)?;
    let id = recipe.id.clone();
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe create", &error))?;
    upsert_recipe(&transaction, &recipe)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe create", &error))?;
    duckdb_read::recipe_by_id(conn, &id)
}

pub(crate) fn update_recipe(
    conn: &mut Connection,
    mut recipe: Recipe,
    cache_updated_at: &str,
) -> Result<Recipe, StoreError> {
    let id = recipe.id.clone();
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe update", &error))?;
    let existing = duckdb_read::recipe_by_id(&transaction, &id)?;
    recipe.last_made_at = existing.last_made_at;
    recipe.times_made = existing.times_made;
    let recipe = recipe.recompute(cache_updated_at)?;
    upsert_recipe(&transaction, &recipe)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe update", &error))?;
    duckdb_read::recipe_by_id(conn, &id)
}

pub(crate) fn delete_recipe(conn: &mut Connection, id: &str) -> Result<(), StoreError> {
    if !row_exists(conn, "recipes", id)? {
        return Err(StoreError::RecipeNotFound);
    }
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe delete", &error))?;
    delete_recipe_rows(&transaction, id)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe delete", &error))
}

pub(crate) fn mark_recipe_made(
    conn: &mut Connection,
    id: &str,
    made_at: &str,
    details: MarkMadeInput,
    cache_updated_at: &str,
) -> Result<Recipe, StoreError> {
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin mark made", &error))?;
    let mut recipe = duckdb_read::recipe_by_id(&transaction, id)?;
    let recipe_title = recipe.title.clone();
    recipe.last_made_at = Some(made_at.to_owned());
    recipe.times_made = recipe.times_made.saturating_add(1);
    let recipe = recipe.recompute(cache_updated_at)?;
    upsert_recipe(&transaction, &recipe)?;

    let cook_log_id = generate_id("cook-log");
    execute(
        &transaction,
        "insert cook log",
        r"insert into cook_log_entries (
             id, recipe_id, made_at, servings_made, servings_eaten, leftover_servings, notes
           ) values ($1, $2, $3, $4, $5, $6, $7)",
        params![
            cook_log_id,
            id,
            made_at,
            details.servings_made,
            details.servings_eaten,
            details.leftover_servings,
            details.notes
        ],
    )?;

    for substitution in details.substitutions {
        execute(
            &transaction,
            "insert cook log substitution",
            r"insert into cook_log_substitutions (
                 id, cook_log_id, ingredient_id, original_item, substitute_text
               ) values ($1, $2, $3, $4, $5)",
            params![
                generate_id("sub"),
                cook_log_id,
                substitution.ingredient_id,
                substitution.original_item,
                substitution.substitute_text
            ],
        )?;
    }

    if details
        .leftover_servings
        .is_some_and(|leftover| leftover > 0.0)
    {
        upsert_pantry_item(
            &transaction,
            &PantryItem {
                id: generate_id("pantry"),
                item: id.to_owned(),
                display_name: format!("Leftover: {recipe_title}"),
                quantity: details.leftover_servings,
                unit: Some("serving".to_owned()),
                category: crate::model::PantryCategory::Leftover,
                source_recipe_id: Some(id.to_owned()),
                notes: None,
                expires_at: None,
                added_at: made_at.to_owned(),
                owner_user_id: Some(DEFAULT_CURRENT_USER_ID.to_owned()),
                family_id: Some(DEFAULT_FAMILY_ID.to_owned()),
            },
        )?;
    }

    transaction
        .commit()
        .map_err(|error| backend("commit mark made", &error))?;
    duckdb_read::recipe_by_id(conn, id)
}

pub(crate) fn add_pantry_item(
    conn: &Connection,
    item: &PantryItem,
) -> Result<PantryItem, StoreError> {
    let id = item.id.clone();
    upsert_pantry_item(conn, item)?;
    duckdb_read::pantry_item_by_id(conn, &id)
}

pub(crate) fn patch_pantry_item(
    conn: &mut Connection,
    id: &str,
    patch: PantryItemPatch,
) -> Result<PantryItem, StoreError> {
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin pantry patch", &error))?;
    let mut item = duckdb_read::pantry_item_by_id(&transaction, id)?;
    if let Some(quantity) = patch.quantity {
        item.quantity = Some(quantity);
    }
    if let Some(unit) = patch.unit {
        item.unit = Some(unit);
    }
    if let Some(category) = patch.category {
        item.category = category;
    }
    if let Some(notes) = patch.notes {
        item.notes = Some(notes);
    }
    if let Some(expires_at) = patch.expires_at {
        item.expires_at = Some(expires_at);
    }
    upsert_pantry_item(&transaction, &item)?;
    transaction
        .commit()
        .map_err(|error| backend("commit pantry patch", &error))?;
    duckdb_read::pantry_item_by_id(conn, id)
}

pub(crate) fn delete_pantry_item(conn: &Connection, id: &str) -> Result<(), StoreError> {
    execute(
        conn,
        "delete pantry item",
        "delete from pantry_items where id = $1",
        params![id],
    )
    .map(|_| ())
}

pub(crate) fn add_meal_plan_entry(
    conn: &Connection,
    entry: &MealPlanEntry,
) -> Result<MealPlanEntry, StoreError> {
    execute(
        conn,
        "insert meal plan entry",
        r"insert into meal_plan_entries (
             id, date, meal_type, recipe_id, servings, notes, owner_user_id, family_id
           ) values ($1, $2, $3, $4, $5, $6, $7, $8)",
        params![
            entry.id,
            entry.date,
            meal_type_name(entry.meal_type),
            entry.recipe_id,
            entry.servings,
            entry.notes,
            entry.owner_user_id,
            entry.family_id
        ],
    )?;
    duckdb_read::meal_plan_entry_by_id(conn, &entry.id)
}

pub(crate) fn delete_meal_plan_entry(conn: &Connection, id: &str) -> Result<(), StoreError> {
    execute(
        conn,
        "delete meal plan entry",
        "delete from meal_plan_entries where id = $1",
        params![id],
    )
    .map(|_| ())
}

pub(crate) fn cookbook_exists(conn: &Connection, id: &str) -> Result<bool, StoreError> {
    row_exists(conn, "cookbooks", id)
}

pub(crate) fn cookbook_page_image_hash_exists(
    conn: &Connection,
    image_hash: &str,
) -> Result<bool, StoreError> {
    conn.query_row(
        "select count(*) > 0 from cookbook_pages where image_hash = $1",
        params![image_hash],
        |row| row.get(0),
    )
    .map_err(|error| backend("check cookbook image hash", &error))
}

pub(crate) fn create_cookbook_source_import(
    conn: &mut Connection,
    source: &CookbookSourceImport,
) -> Result<(), StoreError> {
    validate_cookbook_source_import(source)?;
    if !row_exists(conn, "cookbooks", &source.import_record.cookbook_id)? {
        return Err(StoreError::CookbookNotFound);
    }

    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook source import", &error))?;
    let mut incoming_hashes = HashSet::new();
    for page in &source.pages {
        let Some(image_hash) = page.image_hash.as_deref() else {
            continue;
        };
        if !incoming_hashes.insert(image_hash) {
            return Err(StoreError::DuplicateCookbookPageImage);
        }
        let exists = transaction
            .query_row(
                "select count(*) > 0 from cookbook_pages where image_hash = $1",
                params![image_hash],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| backend("check duplicate cookbook image", &error))?;
        if exists {
            return Err(StoreError::DuplicateCookbookPageImage);
        }
    }

    upsert_cookbook_import(&transaction, &source.import_record)?;
    for page in &source.pages {
        upsert_cookbook_page(&transaction, page)?;
    }
    for section in &source.sections {
        upsert_cookbook_section(&transaction, section)?;
    }
    for block in &source.content_blocks {
        upsert_cookbook_content_block(&transaction, block)?;
    }
    for menu in &source.menus {
        upsert_cookbook_menu(&transaction, menu)?;
    }
    for entry in &source.glossary_entries {
        upsert_cookbook_glossary_entry(&transaction, entry)?;
    }
    for supplier in &source.suppliers {
        upsert_cookbook_supplier(&transaction, supplier)?;
    }
    for entry in &source.index_entries {
        upsert_cookbook_index_entry(&transaction, entry)?;
    }
    for reference in &source.cross_references {
        upsert_cookbook_cross_reference(&transaction, reference)?;
    }
    transaction
        .commit()
        .map_err(|error| backend("commit cookbook source import", &error))
}

pub(crate) fn patch_cookbook_page(
    conn: &mut Connection,
    id: &str,
    patch: CookbookPagePatch,
) -> Result<CookbookPage, StoreError> {
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook page patch", &error))?;
    let mut page = duckdb_read::cookbook_page_by_id(&transaction, id)?;
    if let Some(page_kind) = patch.page_kind {
        page.page_kind = page_kind;
    }
    if let Some(review_status) = patch.review_status {
        page.review_status = review_status;
    }
    if let Some(ocr_text) = patch.ocr_text {
        page.has_ocr_text = !ocr_text.trim().is_empty();
        page.ocr_text = ocr_text;
    }
    upsert_cookbook_page(&transaction, &page)?;
    transaction
        .commit()
        .map_err(|error| backend("commit cookbook page patch", &error))?;
    duckdb_read::cookbook_page_by_id(conn, id)
}

pub(crate) fn patch_cookbook_content_block(
    conn: &mut Connection,
    id: &str,
    patch: CookbookContentBlockPatch,
) -> Result<CookbookContentBlock, StoreError> {
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook content block patch", &error))?;
    let mut block = duckdb_read::cookbook_content_block_by_id(&transaction, id)?;
    if let Some(text) = patch.text {
        block.has_text = !text.trim().is_empty();
        block.text = text;
    }
    if let Some(title) = patch.title {
        block.title = (!title.is_empty()).then_some(title);
    }
    upsert_cookbook_content_block(&transaction, &block)?;
    transaction
        .commit()
        .map_err(|error| backend("commit cookbook content block patch", &error))?;
    duckdb_read::cookbook_content_block_by_id(conn, id)
}

pub(crate) fn accept_cookbook_page_content(
    conn: &mut Connection,
    id: &str,
    input: AcceptPageContentInput,
) -> Result<CookbookContentBlock, StoreError> {
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook page acceptance", &error))?;
    let mut page = duckdb_read::cookbook_page_by_id(&transaction, id)?;
    if !page.has_ocr_text {
        return Err(StoreError::CookbookPageHasNoText);
    }
    let block_id = format!("{id}-content");
    if row_exists(&transaction, "cookbook_content_blocks", &block_id)? {
        return Err(StoreError::CookbookPageAlreadyAccepted);
    }

    let page_number = page.printed_page_number.unwrap_or(page.image_index);
    let section_id = transaction
        .query_row(
            r"select id from cookbook_sections
               where cookbook_id = $1
                 and page_start is not null and page_start <= $2
                 and (page_end is null or page_end >= $2)
               order by page_start desc
               limit 1",
            params![page.cookbook_id, i64::from(page_number)],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| backend("resolve accepted page section", &error))?;
    let position_i64 = transaction
        .query_row(
            "select coalesce(max(position), 0) + 1 from cookbook_content_blocks where cookbook_id = $1",
            params![page.cookbook_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| backend("resolve accepted block position", &error))?;
    let position = u32::try_from(position_i64)
        .map_err(|_| StoreError::NumericOutOfRange("content block position"))?;
    let kind = input.kind.unwrap_or(match page.page_kind {
        CookbookPageKind::Supplier => CookbookContentBlockKind::Supplier,
        CookbookPageKind::Index => CookbookContentBlockKind::IndexEntry,
        _ => CookbookContentBlockKind::Paragraph,
    });
    let source_json = serde_json::to_string(&serde_json::json!({
        "source": "page-accept",
        "pageId": page.id,
        "imageIndex": page.image_index,
        "pageKind": page_kind_name(page.page_kind),
    }))
    .map_err(|error| StoreError::InvalidJson {
        context: "accepted page source",
        detail: error.to_string(),
    })?;
    let block = CookbookContentBlock {
        id: block_id,
        cookbook_id: page.cookbook_id.clone(),
        section_id,
        page_start: Some(page_number),
        page_end: Some(page_number),
        position,
        kind,
        title: input.title,
        text: page.ocr_text.clone(),
        has_text: true,
        confidence: page.average_confidence,
        source_json,
    };
    upsert_cookbook_content_block(&transaction, &block)?;
    page.review_status = CookbookPageReviewStatus::Accepted;
    upsert_cookbook_page(&transaction, &page)?;
    transaction
        .commit()
        .map_err(|error| backend("commit cookbook page acceptance", &error))?;
    Ok(block)
}

pub(crate) fn upsert_cookbook_import_progress(
    conn: &mut Connection,
    progress: &CookbookImportProgress,
    updated_at: &str,
) -> Result<(), StoreError> {
    let created_at = conn
        .query_row(
            "select created_at from cookbook_import_jobs where import_id = $1",
            params![progress.import_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| backend("read cookbook import progress timestamp", &error))?
        .unwrap_or_else(|| updated_at.to_owned());
    execute(
        conn,
        "upsert cookbook import progress",
        r"insert or replace into cookbook_import_jobs (
             import_id, state, stage, message, current_count, total_count,
             processed_count, skipped_count, failed_count, section_count,
             content_block_count, recipe_count, current_section_index, section_total,
             current_section_title, extraction_engine, error_message, created_at, updated_at
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
             $14, $15, $16, $17, $18, $19)",
        params![
            progress.import_id,
            import_job_state_name(progress.state),
            import_pipeline_stage_name(progress.stage),
            progress.message,
            optional_usize_i64(progress.current, "progress current")?,
            optional_usize_i64(progress.total, "progress total")?,
            required_usize_i64(progress.processed_count, "progress processed count")?,
            required_usize_i64(progress.skipped_count, "progress skipped count")?,
            required_usize_i64(progress.failed_count, "progress failed count")?,
            required_usize_i64(progress.section_count, "progress section count")?,
            required_usize_i64(progress.content_block_count, "progress content block count")?,
            required_usize_i64(progress.recipe_count, "progress recipe count")?,
            optional_usize_i64(progress.current_section_index, "progress current section")?,
            optional_usize_i64(progress.section_total, "progress section total")?,
            progress.current_section_title,
            progress.extraction_engine,
            progress.error_message,
            created_at,
            updated_at,
        ],
    )
    .map(|_| ())
}

pub(crate) fn persist_cookbook_pipeline(
    conn: &mut Connection,
    mut result: CookbookPipelineResult,
    cache_updated_at: &str,
) -> Result<CookbookExtractionPersistResult, StoreError> {
    validate_pipeline_result(&result)?;

    let requested_recipe_count = result.recipes.len();
    result.recipes.retain(|recipe| {
        let valid = recipe.cookbook_id == result.import_record.cookbook_id
            && !recipe.id.is_empty()
            && recipe.validate().is_ok();
        if !valid {
            tracing::warn!(
                event = "cookbook_import_recipe_skipped",
                import_id = result.import_record.id,
                recipe_id = recipe.id,
                title = recipe.title,
                reason = "invalid_recipe"
            );
        }
        valid
    });
    result.import_record.status = if result.recipes.is_empty() {
        if result.sections.is_empty() && result.content_blocks.is_empty() {
            CookbookImportStatus::OcrReady
        } else {
            CookbookImportStatus::Mapped
        }
    } else {
        CookbookImportStatus::Committed
    };

    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin cookbook pipeline persistence", &error))?;
    upsert_cookbook_import(&transaction, &result.import_record)?;
    for page in &result.pages {
        upsert_cookbook_page(&transaction, page)?;
    }

    let section_pattern = format!("{}-section-%", result.import_record.id);
    let block_pattern = format!("{}-block-%", result.import_record.id);
    let context_pattern = format!("{}-context-%", result.import_record.id);
    let recipe_pattern = format!("{}-recipe-%", result.import_record.id);
    execute(
        &transaction,
        "delete stale cookbook sections",
        "delete from cookbook_sections where cookbook_id = $1 and id like $2",
        params![result.import_record.cookbook_id, section_pattern],
    )?;
    execute(
        &transaction,
        "delete stale cookbook content blocks",
        r"delete from cookbook_content_blocks
           where cookbook_id = $1 and (id like $2 or id like $3)",
        params![
            result.import_record.cookbook_id,
            block_pattern,
            context_pattern
        ],
    )?;
    for section in &result.sections {
        upsert_cookbook_section(&transaction, section)?;
    }
    for block in &result.content_blocks {
        upsert_cookbook_content_block(&transaction, block)?;
    }

    let existing_generated_ids = {
        let mut statement = transaction
            .prepare("select id from recipes where cookbook_id = $1 and source_block_id like $2")
            .map_err(|error| backend("prepare generated recipe lookup", &error))?;
        let rows = statement
            .query_map(
                params![result.import_record.cookbook_id, recipe_pattern],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| backend("read generated recipes", &error))?;
        rows.collect::<duckdb::Result<Vec<_>>>()
            .map_err(|error| backend("collect generated recipes", &error))?
    };

    let mut persisted_ids = HashSet::new();
    let mut skipped_recipe_count = requested_recipe_count - result.recipes.len();
    for mut recipe in result.recipes {
        if let Ok(existing) = duckdb_read::recipe_by_id(&transaction, &recipe.id) {
            recipe.last_made_at = existing.last_made_at;
            recipe.times_made = existing.times_made;
        }
        let recipe_id = recipe.id.clone();
        let recipe = match recipe.recompute(cache_updated_at) {
            Ok(recipe) => recipe,
            Err(error) => {
                skipped_recipe_count += 1;
                tracing::warn!(
                    event = "cookbook_import_recipe_skipped",
                    import_id = result.import_record.id,
                    recipe_id,
                    reason = %error
                );
                continue;
            }
        };
        if let Err(error) = upsert_recipe(&transaction, &recipe) {
            skipped_recipe_count += 1;
            tracing::warn!(
                event = "cookbook_import_recipe_persist_failed",
                import_id = result.import_record.id,
                recipe_id = recipe.id,
                error = %error
            );
            delete_recipe_rows(&transaction, &recipe.id)?;
            continue;
        }
        persisted_ids.insert(recipe.id);
    }
    for stale_id in existing_generated_ids {
        if !persisted_ids.contains(&stale_id) {
            delete_recipe_rows(&transaction, &stale_id)?;
        }
    }

    transaction
        .commit()
        .map_err(|error| backend("commit cookbook pipeline persistence", &error))?;
    Ok(CookbookExtractionPersistResult {
        recipe_count: persisted_ids.len(),
        skipped_recipe_count,
    })
}

pub(crate) fn create_recipe_import(
    conn: &mut Connection,
    recipe_import: &RecipeImport,
) -> Result<RecipeImport, StoreError> {
    if row_exists(conn, "recipe_imports", &recipe_import.id)? {
        return Err(StoreError::RecipeImportAlreadyExists);
    }
    let id = recipe_import.id.clone();
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe import create", &error))?;
    upsert_recipe_import(&transaction, recipe_import)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe import create", &error))?;
    duckdb_read::recipe_import(conn, &id)
}

pub(crate) fn update_recipe_import_draft(
    conn: &mut Connection,
    import_id: &str,
    recipe: Recipe,
    issues: Vec<ImportIssue>,
    updated_at: &str,
) -> Result<RecipeImport, StoreError> {
    let mut recipe_import = duckdb_read::recipe_import(conn, import_id)?;
    recipe_import.status = RecipeImportStatus::DraftReady;
    recipe_import.draft = Some(recipe);
    recipe_import.validation_issues = issues;
    recipe_import.updated_at = updated_at.to_owned();
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe import draft update", &error))?;
    upsert_recipe_import(&transaction, &recipe_import)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe import draft update", &error))?;
    duckdb_read::recipe_import(conn, import_id)
}

pub(crate) fn commit_recipe_import(
    conn: &mut Connection,
    import_id: &str,
    mut recipe: Recipe,
    issues: Vec<ImportIssue>,
    updated_at: &str,
) -> Result<Recipe, StoreError> {
    let mut recipe_import = duckdb_read::recipe_import(conn, import_id)?;
    let transaction = conn
        .transaction()
        .map_err(|error| backend("begin recipe import commit", &error))?;
    if let Ok(existing) = duckdb_read::recipe_by_id(&transaction, &recipe.id) {
        recipe.last_made_at = existing.last_made_at;
        recipe.times_made = existing.times_made;
    }
    let recipe = recipe.recompute(updated_at)?;
    upsert_recipe(&transaction, &recipe)?;
    recipe_import.status = RecipeImportStatus::Committed;
    recipe_import.draft = Some(recipe.clone());
    recipe_import.validation_issues = issues;
    recipe_import.updated_at = updated_at.to_owned();
    upsert_recipe_import(&transaction, &recipe_import)?;
    transaction
        .commit()
        .map_err(|error| backend("commit recipe import", &error))?;
    duckdb_read::recipe_by_id(conn, &recipe.id)
}

fn validate_cookbook_source_import(source: &CookbookSourceImport) -> Result<(), StoreError> {
    let import = &source.import_record;
    if !is_valid_id(&import.id) || source.pages.is_empty() || import.source_path.is_empty() {
        return Err(StoreError::InvalidCookbookImport);
    }
    for page in &source.pages {
        if !is_valid_id(&page.id)
            || page.image_index == 0
            || page.image_path.is_empty()
            || page.cookbook_id != import.cookbook_id
            || page.import_id != import.id
            || page
                .image_hash
                .as_deref()
                .is_some_and(|hash| !is_sha256_hex(hash))
        {
            return Err(StoreError::InvalidCookbookImport);
        }
    }
    let records_are_valid = source
        .sections
        .iter()
        .map(|record| (&record.id, &record.cookbook_id))
        .chain(
            source
                .content_blocks
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .chain(
            source
                .menus
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .chain(
            source
                .glossary_entries
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .chain(
            source
                .suppliers
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .chain(
            source
                .index_entries
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .chain(
            source
                .cross_references
                .iter()
                .map(|record| (&record.id, &record.cookbook_id)),
        )
        .all(|(id, cookbook_id)| is_valid_id(id) && cookbook_id == &import.cookbook_id);
    if !records_are_valid {
        return Err(StoreError::InvalidCookbookImport);
    }
    Ok(())
}

fn upsert_cookbook(conn: &Connection, cookbook: &Cookbook) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook",
        r"insert or replace into cookbooks (
             id, title, isbn, publisher, published_year, cover_image_url,
             owner_user_id, family_id, share_scope
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        params![
            cookbook.id,
            cookbook.title,
            cookbook.isbn,
            cookbook.publisher,
            cookbook.published_year.map(i64::from),
            cookbook.cover_image_url,
            cookbook
                .owner_user_id
                .as_deref()
                .unwrap_or(DEFAULT_CURRENT_USER_ID),
            cookbook.family_id.as_deref().unwrap_or(DEFAULT_FAMILY_ID),
            share_scope_name(cookbook.share_scope)
        ],
    )?;
    execute(
        conn,
        "clear cookbook authors",
        "delete from cookbook_authors where cookbook_id = $1",
        params![cookbook.id],
    )?;
    for (position, author_id) in cookbook.author_ids.iter().enumerate() {
        execute(
            conn,
            "insert cookbook author",
            "insert into cookbook_authors (cookbook_id, author_id, position) values ($1, $2, $3)",
            params![
                cookbook.id,
                author_id,
                usize_i64(position, "author position")?
            ],
        )?;
    }
    execute(
        conn,
        "clear cookbook shares",
        "delete from cookbook_shares where cookbook_id = $1",
        params![cookbook.id],
    )?;
    for user_id in &cookbook.shared_with_user_ids {
        execute(
            conn,
            "insert cookbook share",
            "insert into cookbook_shares (cookbook_id, user_id) values ($1, $2)",
            params![cookbook.id, user_id],
        )?;
    }
    Ok(())
}

fn upsert_cookbook_import(conn: &Connection, import: &CookbookImport) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook import",
        r"insert or replace into cookbook_imports (
             id, cookbook_id, source_kind, source_path, status, ocr_engine,
             created_at, updated_at, review_notes
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        params![
            import.id,
            import.cookbook_id,
            cookbook_source_kind_name(import.source_kind),
            import.source_path,
            cookbook_import_status_name(import.status),
            import.ocr_engine,
            import.created_at,
            import.updated_at,
            import.review_notes
        ],
    )
    .map(|_| ())
}

fn upsert_recipe_import(conn: &Connection, recipe_import: &RecipeImport) -> Result<(), StoreError> {
    let draft_json = recipe_import
        .draft
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .map_err(|error| StoreError::InvalidJson {
            context: "recipe import draft",
            detail: error.to_string(),
        })?;
    let issues_json = serde_json::to_string(&recipe_import.validation_issues).map_err(|error| {
        StoreError::InvalidJson {
            context: "recipe import validation issues",
            detail: error.to_string(),
        }
    })?;
    execute(
        conn,
        "upsert recipe import",
        r"insert or replace into recipe_imports (
             id, status, file_name, mime_type, image_path, ocr_engine, ocr_text,
             ocr_json, draft_json, validation_issues_json, created_at, updated_at
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        params![
            recipe_import.id,
            recipe_import_status_name(recipe_import.status),
            recipe_import.file_name,
            recipe_import.mime_type,
            recipe_import.image_path,
            recipe_import.ocr_engine,
            recipe_import.ocr_text,
            recipe_import.ocr_json,
            draft_json,
            issues_json,
            recipe_import.created_at,
            recipe_import.updated_at,
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_page(conn: &Connection, page: &CookbookPage) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook page",
        r"insert or replace into cookbook_pages (
             id, cookbook_id, import_id, image_index, printed_page_label, printed_page_number,
             image_path, image_hash, ocr_text, ocr_json, average_confidence, minimum_confidence,
             page_kind, review_status
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)",
        params![
            page.id,
            page.cookbook_id,
            page.import_id,
            i64::from(page.image_index),
            page.printed_page_label,
            page.printed_page_number.map(i64::from),
            page.image_path,
            page.image_hash,
            page.ocr_text,
            page.ocr_json,
            page.average_confidence,
            page.minimum_confidence,
            page_kind_name(page.page_kind),
            cookbook_page_review_status_name(page.review_status)
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_section(conn: &Connection, section: &CookbookSection) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook section",
        r"insert or replace into cookbook_sections (
             id, cookbook_id, parent_section_id, title, kind, position, page_start, page_end
           ) values ($1, $2, $3, $4, $5, $6, $7, $8)",
        params![
            section.id,
            section.cookbook_id,
            section.parent_section_id,
            section.title,
            cookbook_section_kind_name(section.kind),
            i64::from(section.position),
            section.page_start.map(i64::from),
            section.page_end.map(i64::from)
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_content_block(
    conn: &Connection,
    block: &CookbookContentBlock,
) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook content block",
        r"insert or replace into cookbook_content_blocks (
             id, cookbook_id, section_id, page_start, page_end, position, kind,
             title, text, confidence, source_json
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        params![
            block.id,
            block.cookbook_id,
            block.section_id,
            block.page_start.map(i64::from),
            block.page_end.map(i64::from),
            i64::from(block.position),
            cookbook_content_block_kind_name(block.kind),
            block.title,
            block.text,
            block.confidence,
            block.source_json
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_menu(conn: &Connection, menu: &CookbookMenu) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook menu",
        r"insert or replace into cookbook_menus (
             id, cookbook_id, source_block_id, title, theme, notes
           ) values ($1, $2, $3, $4, $5, $6)",
        params![
            menu.id,
            menu.cookbook_id,
            menu.source_block_id,
            menu.title,
            menu.theme,
            menu.notes
        ],
    )?;
    execute(
        conn,
        "clear cookbook menu recipes",
        "delete from cookbook_menu_recipes where menu_id = $1",
        params![menu.id],
    )?;
    for recipe in &menu.recipes {
        execute(
            conn,
            "insert cookbook menu recipe",
            r"insert into cookbook_menu_recipes (
                 menu_id, recipe_id, position, role, serving_notes
               ) values ($1, $2, $3, $4, $5)",
            params![
                menu.id,
                recipe.recipe_id,
                i64::from(recipe.position),
                recipe.role,
                recipe.serving_notes
            ],
        )?;
    }
    Ok(())
}

fn upsert_cookbook_glossary_entry(
    conn: &Connection,
    entry: &CookbookGlossaryEntry,
) -> Result<(), StoreError> {
    let aliases_json =
        serde_json::to_string(&entry.aliases).map_err(|error| StoreError::InvalidJson {
            context: "cookbook glossary aliases",
            detail: error.to_string(),
        })?;
    let native_names_json =
        serde_json::to_string(&entry.native_names).map_err(|error| StoreError::InvalidJson {
            context: "cookbook glossary native names",
            detail: error.to_string(),
        })?;
    execute(
        conn,
        "upsert cookbook glossary entry",
        r"insert or replace into cookbook_glossary_entries (
             id, cookbook_id, source_block_id, title, aliases_json, native_names_json,
             description, storage_notes, substitution_notes, page_start, page_end
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        params![
            entry.id,
            entry.cookbook_id,
            entry.source_block_id,
            entry.title,
            aliases_json,
            native_names_json,
            entry.description,
            entry.storage_notes,
            entry.substitution_notes,
            entry.page_start.map(i64::from),
            entry.page_end.map(i64::from)
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_supplier(
    conn: &Connection,
    supplier: &CookbookSupplier,
) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook supplier",
        r"insert or replace into cookbook_suppliers (
             id, cookbook_id, source_block_id, name, url, region, notes,
             source_page, review_status
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        params![
            supplier.id,
            supplier.cookbook_id,
            supplier.source_block_id,
            supplier.name,
            supplier.url,
            supplier.region,
            supplier.notes,
            supplier.source_page.map(i64::from),
            cookbook_page_review_status_name(supplier.review_status)
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_index_entry(
    conn: &Connection,
    entry: &CookbookIndexEntry,
) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook index entry",
        r"insert or replace into cookbook_index_entries (
             id, cookbook_id, term, subterm, target_page_label, target_page_number,
             target_recipe_id, illustration
           ) values ($1, $2, $3, $4, $5, $6, $7, $8)",
        params![
            entry.id,
            entry.cookbook_id,
            entry.term,
            entry.subterm,
            entry.target_page_label,
            entry.target_page_number.map(i64::from),
            entry.target_recipe_id,
            entry.illustration
        ],
    )
    .map(|_| ())
}

fn upsert_cookbook_cross_reference(
    conn: &Connection,
    reference: &CookbookCrossReference,
) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert cookbook cross reference",
        r"insert or replace into cookbook_cross_references (
             id, cookbook_id, from_kind, from_id, to_kind, to_id, label, relation_kind
           ) values ($1, $2, $3, $4, $5, $6, $7, $8)",
        params![
            reference.id,
            reference.cookbook_id,
            reference.from_kind,
            reference.from_id,
            reference.to_kind,
            reference.to_id,
            reference.label,
            reference.relation_kind
        ],
    )
    .map(|_| ())
}

#[allow(clippy::too_many_lines)] // Mirrors the ordered replacement of all nine recipe child tables.
fn upsert_recipe(conn: &Connection, recipe: &Recipe) -> Result<(), StoreError> {
    let cost_cents = optional_i64(recipe.cost_cents, "recipe cost")?;
    let cost_per_serving = optional_i64(recipe.cost_per_serving_cents, "serving cost")?;
    execute(
        conn,
        "upsert recipe",
        r"insert or replace into recipes (
             id, title, cookbook_id, source_label, page_start, page_end,
             yield_quantity, yield_unit, prep_minutes, cook_minutes, total_minutes,
             cuisine, category, subtitle, headnote, serving_context,
             source_block_id, pictured_page_number, extraction_status,
             searchable_text, last_made_at, times_made,
             cost_cents, cost_per_serving_cents, cache_key, cache_updated_at
           ) values (
             $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
             $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25, $26
           )",
        params![
            recipe.id,
            recipe.title,
            recipe.cookbook_id,
            recipe.source_label,
            recipe.page_start.map(i64::from),
            recipe.page_end.map(i64::from),
            recipe.yield_quantity,
            recipe.yield_unit,
            recipe.prep_minutes.map(i64::from),
            recipe.cook_minutes.map(i64::from),
            recipe.total_minutes.map(i64::from),
            recipe.cuisine,
            recipe.category,
            recipe.subtitle,
            recipe.headnote,
            recipe.serving_context,
            recipe.source_block_id,
            recipe.pictured_page_number.map(i64::from),
            extraction_status_name(recipe.extraction_status),
            recipe.searchable_text,
            recipe.last_made_at,
            i64::from(recipe.times_made),
            cost_cents,
            cost_per_serving,
            recipe.cache_key,
            recipe.cache_updated_at
        ],
    )?;

    replace_recipe_string_rows(
        conn,
        &recipe.id,
        "recipe_authors",
        "author_id",
        &recipe.author_ids,
    )?;
    replace_recipe_string_rows(conn, &recipe.id, "recipe_tags", "tag", &recipe.tags)?;

    clear_recipe_rows(conn, "recipe_alternate_names", &recipe.id)?;
    for (position, name) in recipe.alternate_names.iter().enumerate() {
        execute(
            conn,
            "insert recipe alternate name",
            r"insert into recipe_alternate_names (recipe_id, kind, value, position)
               values ($1, $2, $3, $4)",
            params![
                recipe.id,
                name.kind,
                name.value,
                usize_i64(position, "alternate-name position")?
            ],
        )?;
    }

    clear_recipe_rows(conn, "recipe_source_page_spans", &recipe.id)?;
    for (position, span) in recipe.source_page_spans.iter().enumerate() {
        execute(
            conn,
            "insert recipe source span",
            r"insert into recipe_source_page_spans (
                 recipe_id, page_id, printed_page_number, line_start, line_end, confidence, position
               ) values ($1, $2, $3, $4, $5, $6, $7)",
            params![
                recipe.id,
                span.page_id,
                span.printed_page_number.map(i64::from),
                span.line_start.map(i64::from),
                span.line_end.map(i64::from),
                span.confidence,
                usize_i64(position, "source-span position")?
            ],
        )?;
    }

    replace_recipe_string_rows(
        conn,
        &recipe.id,
        "recipe_components",
        "component_recipe_id",
        &recipe.component_recipe_ids,
    )?;

    clear_recipe_rows(conn, "recipe_ingredients", &recipe.id)?;
    for (index, ingredient) in recipe.ingredients.iter().enumerate() {
        let position = ingredient.position.map_or_else(
            || usize_i64(index + 1, "ingredient position"),
            |position| Ok(i64::from(position)),
        )?;
        execute(
            conn,
            "insert recipe ingredient",
            r"insert into recipe_ingredients (
                 recipe_id, ingredient_id, position, display_name, item, quantity, unit,
                 preparation, section, optional, alternative_text, source_line, source_page_id,
                 unit_cost_cents, estimated_cost_cents, quantity_text, quantity_min, quantity_max,
                 quantity_kind, quantity_review_status, quantity_review_reason
               ) values (
                 $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13,
                 $14, $15, $16, $17, $18, $19, $20, $21
               )",
            params![
                recipe.id,
                ingredient.id,
                position,
                ingredient.display_name,
                ingredient.item,
                ingredient.quantity,
                ingredient.unit,
                ingredient.preparation,
                ingredient.section,
                ingredient.optional,
                ingredient.alternative_text,
                ingredient.source_line.map(i64::from),
                ingredient.source_page_id,
                optional_i64(ingredient.unit_cost_cents, "ingredient unit cost")?,
                optional_i64(ingredient.estimated_cost_cents, "ingredient estimated cost")?,
                ingredient.quantity_text,
                ingredient.quantity_min,
                ingredient.quantity_max,
                quantity_kind_name(ingredient.quantity_kind),
                quantity_review_status_name(ingredient.quantity_review_status),
                ingredient.quantity_review_reason
            ],
        )?;
    }

    clear_recipe_rows(conn, "recipe_steps", &recipe.id)?;
    for step in &recipe.steps {
        execute(
            conn,
            "insert recipe step",
            r"insert into recipe_steps (
                 recipe_id, step_id, position, section, text, source_page_id,
                 source_line_start, source_line_end
               ) values ($1, $2, $3, $4, $5, $6, $7, $8)",
            params![
                recipe.id,
                step.id,
                i64::from(step.position),
                step.section,
                step.text,
                step.source_page_id,
                step.source_line_start.map(i64::from),
                step.source_line_end.map(i64::from)
            ],
        )?;
    }

    clear_recipe_rows(conn, "recipe_images", &recipe.id)?;
    for image in &recipe.images {
        execute(
            conn,
            "insert recipe image",
            r"insert into recipe_images (recipe_id, image_id, url, alt, credit, is_primary)
               values ($1, $2, $3, $4, $5, $6)",
            params![
                recipe.id,
                image.id,
                image.url,
                image.alt,
                image.credit,
                image.is_primary
            ],
        )?;
    }

    clear_recipe_rows(conn, "recipe_notes", &recipe.id)?;
    for note in &recipe.notes {
        execute(
            conn,
            "insert recipe note",
            r"insert into recipe_notes (recipe_id, note_id, text, created_at)
               values ($1, $2, $3, $4)",
            params![recipe.id, note.id, note.text, note.created_at],
        )?;
    }
    Ok(())
}

fn upsert_pantry_item(conn: &Connection, item: &PantryItem) -> Result<(), StoreError> {
    execute(
        conn,
        "upsert pantry item",
        r"insert or replace into pantry_items (
             id, item, display_name, quantity, unit, category, source_recipe_id, notes,
             expires_at, added_at, owner_user_id, family_id
           ) values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)",
        params![
            item.id,
            item.item,
            item.display_name,
            item.quantity,
            item.unit,
            pantry_category_name(item.category),
            item.source_recipe_id,
            item.notes,
            item.expires_at,
            item.added_at,
            item.owner_user_id
                .as_deref()
                .unwrap_or(DEFAULT_CURRENT_USER_ID),
            item.family_id.as_deref().unwrap_or(DEFAULT_FAMILY_ID)
        ],
    )
    .map(|_| ())
}

fn replace_recipe_string_rows(
    conn: &Connection,
    recipe_id: &str,
    table: &'static str,
    value_column: &'static str,
    values: &[String],
) -> Result<(), StoreError> {
    clear_recipe_rows(conn, table, recipe_id)?;
    let sql =
        format!("insert into {table} (recipe_id, {value_column}, position) values ($1, $2, $3)");
    for (position, value) in values.iter().enumerate() {
        execute(
            conn,
            "insert recipe string row",
            &sql,
            params![
                recipe_id,
                value,
                usize_i64(position, "recipe child position")?
            ],
        )?;
    }
    Ok(())
}

fn clear_recipe_rows(
    conn: &Connection,
    table: &'static str,
    recipe_id: &str,
) -> Result<(), StoreError> {
    execute(
        conn,
        "clear recipe child rows",
        &format!("delete from {table} where recipe_id = $1"),
        params![recipe_id],
    )
    .map(|_| ())
}

fn delete_recipe_rows(conn: &Connection, id: &str) -> Result<(), StoreError> {
    for table in [
        "recipe_authors",
        "recipe_tags",
        "recipe_alternate_names",
        "recipe_source_page_spans",
        "recipe_components",
        "recipe_ingredients",
        "recipe_steps",
        "recipe_images",
        "recipe_notes",
    ] {
        clear_recipe_rows(conn, table, id)?;
    }
    execute(
        conn,
        "delete recipe",
        "delete from recipes where id = $1",
        params![id],
    )?;
    execute(
        conn,
        "delete recipe meal-plan entries",
        "delete from meal_plan_entries where recipe_id = $1",
        params![id],
    )?;
    Ok(())
}

fn row_exists(conn: &Connection, table: &'static str, id: &str) -> Result<bool, StoreError> {
    conn.query_row(
        &format!("select count(*) > 0 from {table} where id = $1"),
        params![id],
        |row| row.get(0),
    )
    .map_err(|error| backend("check row existence", &error))
}

fn validate_pipeline_result(result: &CookbookPipelineResult) -> Result<(), StoreError> {
    if result.import_record.id.is_empty()
        || result.import_record.cookbook_id.is_empty()
        || result.import_record.source_path.is_empty()
        || result.pages.is_empty()
    {
        return Err(StoreError::InvalidCookbookImport);
    }
    if result.pages.iter().any(|page| {
        page.id.is_empty()
            || page.image_index == 0
            || page.image_path.is_empty()
            || page.import_id != result.import_record.id
            || page.cookbook_id != result.import_record.cookbook_id
    }) {
        return Err(StoreError::InvalidCookbookImport);
    }
    if result.sections.iter().any(|section| {
        section.id.is_empty() || section.cookbook_id != result.import_record.cookbook_id
    }) || result
        .content_blocks
        .iter()
        .any(|block| block.id.is_empty() || block.cookbook_id != result.import_record.cookbook_id)
    {
        return Err(StoreError::InvalidCookbookImport);
    }
    Ok(())
}

const fn import_job_state_name(state: ImportJobState) -> &'static str {
    match state {
        ImportJobState::Running => "running",
        ImportJobState::Complete => "complete",
        ImportJobState::Failed => "failed",
        ImportJobState::Canceled => "canceled",
    }
}

const fn import_pipeline_stage_name(stage: ImportPipelineStage) -> &'static str {
    match stage {
        ImportPipelineStage::Queued => "queued",
        ImportPipelineStage::LoadingPages => "loading_pages",
        ImportPipelineStage::OcrPages => "ocr_pages",
        ImportPipelineStage::SourceMap => "source_map",
        ImportPipelineStage::DeepseekPlan => "deepseek_plan",
        ImportPipelineStage::DeepseekSection => "deepseek_section",
        ImportPipelineStage::Normalizing => "normalizing",
        ImportPipelineStage::Persisting => "persisting",
        ImportPipelineStage::Complete => "complete",
        ImportPipelineStage::Failed => "failed",
        ImportPipelineStage::Canceled => "canceled",
    }
}

const fn recipe_import_status_name(status: RecipeImportStatus) -> &'static str {
    match status {
        RecipeImportStatus::Processing => "processing",
        RecipeImportStatus::DraftReady => "draft_ready",
        RecipeImportStatus::Failed => "failed",
        RecipeImportStatus::Committed => "committed",
    }
}

fn optional_usize_i64(
    value: Option<usize>,
    field: &'static str,
) -> Result<Option<i64>, StoreError> {
    value
        .map(|value| required_usize_i64(value, field))
        .transpose()
}

fn required_usize_i64(value: usize, field: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericOutOfRange(field))
}

fn execute<P: Params>(
    conn: &Connection,
    operation: &'static str,
    sql: &str,
    parameters: P,
) -> Result<usize, StoreError> {
    conn.execute(sql, parameters)
        .map_err(|error| backend(operation, &error))
}

fn optional_i64(value: Option<u64>, context: &'static str) -> Result<Option<i64>, StoreError> {
    value
        .map(|value| i64::try_from(value).map_err(|_| StoreError::NumericOutOfRange(context)))
        .transpose()
}

fn usize_i64(value: usize, context: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::NumericOutOfRange(context))
}

fn is_valid_id(id: &str) -> bool {
    let mut characters = id.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (2..=80).contains(&id.len())
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && characters.all(|character| {
            character == '-' || character.is_ascii_lowercase() || character.is_ascii_digit()
        })
}

fn is_sha256_hex(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn cookbook_source_kind_name(value: CookbookSourceKind) -> &'static str {
    match value {
        CookbookSourceKind::ImageSet => "image_set",
        CookbookSourceKind::Pdf => "pdf",
        CookbookSourceKind::Manual => "manual",
        CookbookSourceKind::Web => "web",
    }
}

fn cookbook_import_status_name(value: CookbookImportStatus) -> &'static str {
    match value {
        CookbookImportStatus::Uploaded => "uploaded",
        CookbookImportStatus::OcrReady => "ocr_ready",
        CookbookImportStatus::Mapped => "mapped",
        CookbookImportStatus::Reviewed => "reviewed",
        CookbookImportStatus::Committed => "committed",
    }
}

fn page_kind_name(value: CookbookPageKind) -> &'static str {
    match value {
        CookbookPageKind::Cover => "cover",
        CookbookPageKind::Title => "title",
        CookbookPageKind::Contents => "contents",
        CookbookPageKind::ChapterOpener => "chapter_opener",
        CookbookPageKind::Essay => "essay",
        CookbookPageKind::Reference => "reference",
        CookbookPageKind::Recipe => "recipe",
        CookbookPageKind::Supplier => "supplier",
        CookbookPageKind::Index => "index",
        CookbookPageKind::Acknowledgements => "acknowledgements",
        CookbookPageKind::Blank => "blank",
        CookbookPageKind::Unknown => "unknown",
    }
}

fn cookbook_page_review_status_name(value: CookbookPageReviewStatus) -> &'static str {
    match value {
        CookbookPageReviewStatus::Pending => "pending",
        CookbookPageReviewStatus::Accepted => "accepted",
        CookbookPageReviewStatus::NeedsCrop => "needs_crop",
        CookbookPageReviewStatus::NeedsOcrFix => "needs_ocr_fix",
        CookbookPageReviewStatus::Ignored => "ignored",
    }
}

fn cookbook_section_kind_name(value: CookbookSectionKind) -> &'static str {
    match value {
        CookbookSectionKind::FrontMatter => "front_matter",
        CookbookSectionKind::Chapter => "chapter",
        CookbookSectionKind::Essay => "essay",
        CookbookSectionKind::Reference => "reference",
        CookbookSectionKind::Recipes => "recipes",
        CookbookSectionKind::BackMatter => "back_matter",
    }
}

fn cookbook_content_block_kind_name(value: CookbookContentBlockKind) -> &'static str {
    match value {
        CookbookContentBlockKind::Paragraph => "paragraph",
        CookbookContentBlockKind::Recipe => "recipe",
        CookbookContentBlockKind::RecipeHeadnote => "recipe_headnote",
        CookbookContentBlockKind::IngredientGlossaryEntry => "ingredient_glossary_entry",
        CookbookContentBlockKind::Menu => "menu",
        CookbookContentBlockKind::Supplier => "supplier",
        CookbookContentBlockKind::IndexEntry => "index_entry",
        CookbookContentBlockKind::Caption => "caption",
        CookbookContentBlockKind::Callout => "callout",
    }
}

fn share_scope_name(value: ShareScope) -> &'static str {
    match value {
        ShareScope::Personal => "personal",
        ShareScope::Family => "family",
        ShareScope::Users => "users",
    }
}

fn meal_type_name(value: crate::model::MealType) -> &'static str {
    match value {
        crate::model::MealType::Breakfast => "breakfast",
        crate::model::MealType::Lunch => "lunch",
        crate::model::MealType::Dinner => "dinner",
    }
}

fn pantry_category_name(value: crate::model::PantryCategory) -> &'static str {
    match value {
        crate::model::PantryCategory::Raw => "raw",
        crate::model::PantryCategory::Prepared => "prepared",
        crate::model::PantryCategory::Leftover => "leftover",
    }
}

fn extraction_status_name(value: crate::model::RecipeExtractionStatus) -> &'static str {
    match value {
        crate::model::RecipeExtractionStatus::Draft => "draft",
        crate::model::RecipeExtractionStatus::NeedsReview => "needs_review",
        crate::model::RecipeExtractionStatus::Verified => "verified",
    }
}

fn quantity_kind_name(value: crate::model::IngredientQuantityKind) -> &'static str {
    match value {
        crate::model::IngredientQuantityKind::Exact => "exact",
        crate::model::IngredientQuantityKind::Range => "range",
        crate::model::IngredientQuantityKind::AsNeeded => "as_needed",
        crate::model::IngredientQuantityKind::Unknown => "unknown",
    }
}

fn quantity_review_status_name(
    value: crate::model::IngredientQuantityReviewStatus,
) -> &'static str {
    match value {
        crate::model::IngredientQuantityReviewStatus::Parsed => "parsed",
        crate::model::IngredientQuantityReviewStatus::NeedsReview => "needs_review",
    }
}

fn backend(operation: &'static str, error: &duckdb::Error) -> StoreError {
    StoreError::Backend {
        operation,
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_match_the_existing_zig_rules() {
        assert!(is_valid_id("our-korean-kitchen"));
        assert!(is_valid_id("2nd-book"));
        assert!(!is_valid_id("Uppercase"));
        assert!(!is_valid_id("has_underscore"));
        assert!(!is_valid_id("a"));
    }
}
