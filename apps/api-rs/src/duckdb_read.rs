use duckdb::{Connection, Params, Row, params};
use serde::de::DeserializeOwned;

use crate::{
    model::{
        Author, Catalogue, CookLogEntry, Cookbook, CookbookContentBlock, CookbookContentBlockKind,
        CookbookCrossReference, CookbookGlossaryEntry, CookbookImport, CookbookImportProgress,
        CookbookImportStatus, CookbookIndexEntry, CookbookMenu, CookbookMenuRecipe, CookbookPage,
        CookbookPageKind, CookbookPageReviewStatus, CookbookPageText, CookbookSection,
        CookbookSectionKind, CookbookSourceKind, CookbookSupplier, Family, ImportIssue,
        ImportJobState, ImportPipelineStage, Ingredient, IngredientQuantityKind,
        IngredientQuantityReviewStatus, InstructionStep, MealPlanEntry, MealType, PantryCategory,
        PantryItem, Recipe, RecipeAlternateName, RecipeExtractionStatus, RecipeImage, RecipeImport,
        RecipeImportStatus, RecipeNote, RecipeSourcePageSpan, ShareScope, Substitution, User,
    },
    store::{CookbookPageImage, CookbookPipelineSource, StoreError},
};

const CURRENT_USER_ID: &str = "avery-river";
const SUMMARY_TEXT_PREVIEW_BYTES: usize = 420;

pub(crate) fn catalogue_summary(conn: &Connection) -> Result<Catalogue, StoreError> {
    Ok(Catalogue {
        current_user_id: Some(CURRENT_USER_ID.to_owned()),
        families: read_families(conn)?,
        users: read_users(conn)?,
        authors: read_authors(conn)?,
        cookbooks: read_cookbooks(conn)?,
        recipes: read_recipes(conn)?,
        cookbook_imports: read_cookbook_imports(conn)?,
        cookbook_pages: read_cookbook_pages_summary(conn)?,
        cookbook_sections: read_cookbook_sections(conn)?,
        cookbook_content_blocks: read_cookbook_content_blocks_summary(conn)?,
        cookbook_menus: read_cookbook_menus(conn)?,
        cookbook_glossary_entries: read_cookbook_glossary_entries(conn)?,
        cookbook_suppliers: read_cookbook_suppliers(conn)?,
        cookbook_index_entries: read_cookbook_index_entries(conn)?,
        cookbook_cross_references: read_cookbook_cross_references(conn)?,
    })
}

pub(crate) fn pantry_items(conn: &Connection) -> Result<Vec<PantryItem>, StoreError> {
    query_rows(
        conn,
        "read pantry items",
        r"select
             id, item, display_name, quantity, unit, category, source_recipe_id, notes,
             expires_at, added_at, owner_user_id, family_id
           from pantry_items
           order by category, display_name",
        [],
        pantry_item,
    )
}

pub(crate) fn pantry_item_by_id(conn: &Connection, id: &str) -> Result<PantryItem, StoreError> {
    query_optional(
        conn,
        "read pantry item",
        r"select
             id, item, display_name, quantity, unit, category, source_recipe_id, notes,
             expires_at, added_at, owner_user_id, family_id
           from pantry_items
           where id = $1",
        params![id],
        pantry_item,
    )?
    .ok_or(StoreError::PantryItemNotFound)
}

fn pantry_item(row: &Row<'_>) -> duckdb::Result<PantryItem> {
    let category = row.get::<_, String>(5)?;
    Ok(PantryItem {
        id: row.get(0)?,
        item: row.get(1)?,
        display_name: row.get(2)?,
        quantity: row.get(3)?,
        unit: row.get(4)?,
        category: pantry_category(&category),
        source_recipe_id: row.get(6)?,
        notes: row.get(7)?,
        expires_at: row.get(8)?,
        added_at: row.get(9)?,
        owner_user_id: row.get(10)?,
        family_id: row.get(11)?,
    })
}

pub(crate) fn meal_plan_entries(conn: &Connection) -> Result<Vec<MealPlanEntry>, StoreError> {
    query_rows(
        conn,
        "read meal plan entries",
        r"select id, date, meal_type, recipe_id, servings, notes, owner_user_id, family_id
           from meal_plan_entries
           order by date, meal_type",
        [],
        meal_plan_entry,
    )
}

pub(crate) fn meal_plan_entry_by_id(
    conn: &Connection,
    id: &str,
) -> Result<MealPlanEntry, StoreError> {
    query_optional(
        conn,
        "read meal plan entry",
        r"select id, date, meal_type, recipe_id, servings, notes, owner_user_id, family_id
           from meal_plan_entries
           where id = $1",
        params![id],
        meal_plan_entry,
    )?
    .ok_or_else(|| StoreError::Unavailable("meal plan entry was not persisted".to_owned()))
}

fn meal_plan_entry(row: &Row<'_>) -> duckdb::Result<MealPlanEntry> {
    let meal_type = row.get::<_, String>(2)?;
    Ok(MealPlanEntry {
        id: row.get(0)?,
        date: row.get(1)?,
        meal_type: meal_type_value(&meal_type),
        recipe_id: row.get(3)?,
        servings: row.get(4)?,
        notes: row.get(5)?,
        owner_user_id: row.get(6)?,
        family_id: row.get(7)?,
    })
}

pub(crate) fn cook_log_entries(conn: &Connection) -> Result<Vec<CookLogEntry>, StoreError> {
    let rows = query_rows(
        conn,
        "read cook log entries",
        r"select id, recipe_id, made_at, servings_made, servings_eaten, leftover_servings, notes
           from cook_log_entries
           order by made_at desc",
        [],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<f64>>(3)?,
                row.get::<_, Option<f64>>(4)?,
                row.get::<_, Option<f64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    )?;

    rows.into_iter()
        .map(
            |(id, recipe_id, made_at, servings_made, servings_eaten, leftover_servings, notes)| {
                Ok(CookLogEntry {
                    substitutions: read_cook_log_substitutions(conn, &id)?,
                    id,
                    recipe_id,
                    made_at,
                    servings_made,
                    servings_eaten,
                    leftover_servings,
                    notes,
                })
            },
        )
        .collect()
}

fn read_cook_log_substitutions(
    conn: &Connection,
    cook_log_id: &str,
) -> Result<Vec<Substitution>, StoreError> {
    query_rows(
        conn,
        "read cook log substitutions",
        r"select id, ingredient_id, original_item, substitute_text
           from cook_log_substitutions
           where cook_log_id = $1
           order by id",
        params![cook_log_id],
        |row| {
            Ok(Substitution {
                id: row.get(0)?,
                ingredient_id: row.get(1)?,
                original_item: row.get(2)?,
                substitute_text: row.get(3)?,
            })
        },
    )
}

pub(crate) fn cookbook_page_text(
    conn: &Connection,
    page_id: &str,
) -> Result<CookbookPageText, StoreError> {
    query_optional(
        conn,
        "read cookbook page text",
        "select id, ocr_text, ocr_json from cookbook_pages where id = $1",
        params![page_id],
        |row| {
            Ok(CookbookPageText {
                id: row.get(0)?,
                ocr_text: row.get(1)?,
                ocr_json: row.get(2)?,
            })
        },
    )?
    .ok_or(StoreError::CookbookPageNotFound)
}

pub(crate) fn cookbook_page_by_id(
    conn: &Connection,
    page_id: &str,
) -> Result<CookbookPage, StoreError> {
    query_optional(
        conn,
        "read cookbook page",
        r"select
             id, cookbook_id, import_id, image_index, printed_page_label, printed_page_number,
             image_path, image_hash, ocr_text, ocr_json, average_confidence, minimum_confidence,
             page_kind, review_status
           from cookbook_pages
           where id = $1",
        params![page_id],
        |row| {
            let ocr_text: String = row.get(8)?;
            let page_kind: String = row.get(12)?;
            let review_status: String = row.get(13)?;
            Ok(CookbookPage {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                import_id: row.get(2)?,
                image_index: row.get(3)?,
                printed_page_label: row.get(4)?,
                printed_page_number: row.get(5)?,
                image_path: row.get(6)?,
                image_hash: row.get(7)?,
                has_ocr_text: has_visible_text(&ocr_text),
                ocr_text,
                ocr_json: row.get(9)?,
                average_confidence: row.get(10)?,
                minimum_confidence: row.get(11)?,
                page_kind: cookbook_page_kind(&page_kind),
                review_status: cookbook_page_review_status(&review_status),
            })
        },
    )?
    .ok_or(StoreError::CookbookPageNotFound)
}

pub(crate) fn cookbook_page_image(
    conn: &Connection,
    page_id: &str,
) -> Result<CookbookPageImage, StoreError> {
    query_optional(
        conn,
        "read cookbook page image",
        "select image_path, image_hash from cookbook_pages where id = $1",
        params![page_id],
        |row| {
            Ok(CookbookPageImage {
                image_path: row.get(0)?,
                image_hash: row.get(1)?,
            })
        },
    )?
    .ok_or(StoreError::CookbookPageNotFound)
}

pub(crate) fn cookbook_content_blocks(
    conn: &Connection,
    cookbook_id: &str,
) -> Result<Vec<CookbookContentBlock>, StoreError> {
    let rows = query_rows(
        conn,
        "read cookbook content blocks",
        r"select
             id, cookbook_id, section_id, page_start, page_end, position, kind,
             title, text, confidence, source_json
           from cookbook_content_blocks
           where cookbook_id = $1
           order by position, id",
        params![cookbook_id],
        raw_content_block,
    )?;
    Ok(rows.into_iter().map(RawContentBlock::into_full).collect())
}

pub(crate) fn cookbook_content_block_by_id(
    conn: &Connection,
    block_id: &str,
) -> Result<CookbookContentBlock, StoreError> {
    query_optional(
        conn,
        "read cookbook content block",
        r"select
             id, cookbook_id, section_id, page_start, page_end, position, kind,
             title, text, confidence, source_json
           from cookbook_content_blocks
           where id = $1",
        params![block_id],
        raw_content_block,
    )?
    .map(RawContentBlock::into_full)
    .ok_or(StoreError::CookbookContentBlockNotFound)
}

fn read_families(conn: &Connection) -> Result<Vec<Family>, StoreError> {
    query_rows(
        conn,
        "read families",
        "select id, name, pantry_shared, meal_plan_shared from families order by name",
        [],
        |row| {
            Ok(Family {
                id: row.get(0)?,
                name: row.get(1)?,
                pantry_shared: row.get(2)?,
                meal_plan_shared: row.get(3)?,
            })
        },
    )
}

fn read_users(conn: &Connection) -> Result<Vec<User>, StoreError> {
    query_rows(
        conn,
        "read users",
        "select id, display_name, email, family_id from users order by display_name",
        [],
        |row| {
            Ok(User {
                id: row.get(0)?,
                display_name: row.get(1)?,
                email: row.get(2)?,
                family_id: row.get(3)?,
            })
        },
    )
}

fn read_authors(conn: &Connection) -> Result<Vec<Author>, StoreError> {
    query_rows(
        conn,
        "read authors",
        "select id, name, website from authors order by name",
        [],
        |row| {
            Ok(Author {
                id: row.get(0)?,
                name: row.get(1)?,
                website: row.get(2)?,
            })
        },
    )
}

#[derive(Debug)]
struct RawCookbook {
    id: String,
    title: String,
    isbn: Option<String>,
    publisher: Option<String>,
    published_year: Option<u16>,
    cover_image_url: Option<String>,
    owner_user_id: Option<String>,
    family_id: Option<String>,
    share_scope: String,
}

fn read_cookbooks(conn: &Connection) -> Result<Vec<Cookbook>, StoreError> {
    let rows = query_rows(
        conn,
        "read cookbooks",
        r"select
             id, title, isbn, publisher, published_year, cover_image_url,
             owner_user_id, family_id, coalesce(share_scope, 'personal')
           from cookbooks
           order by title",
        [],
        raw_cookbook,
    )?;

    rows.into_iter()
        .map(|row| hydrate_cookbook(conn, row))
        .collect()
}

pub(crate) fn cookbook_by_id(conn: &Connection, id: &str) -> Result<Cookbook, StoreError> {
    let row = query_optional(
        conn,
        "read cookbook",
        r"select
             id, title, isbn, publisher, published_year, cover_image_url,
             owner_user_id, family_id, coalesce(share_scope, 'personal')
           from cookbooks
           where id = $1",
        params![id],
        raw_cookbook,
    )?
    .ok_or_else(|| StoreError::Unavailable("cookbook was not persisted".to_owned()))?;
    hydrate_cookbook(conn, row)
}

fn raw_cookbook(row: &Row<'_>) -> duckdb::Result<RawCookbook> {
    Ok(RawCookbook {
        id: row.get(0)?,
        title: row.get(1)?,
        isbn: row.get(2)?,
        publisher: row.get(3)?,
        published_year: row.get(4)?,
        cover_image_url: row.get(5)?,
        owner_user_id: row.get(6)?,
        family_id: row.get(7)?,
        share_scope: row.get(8)?,
    })
}

fn hydrate_cookbook(conn: &Connection, row: RawCookbook) -> Result<Cookbook, StoreError> {
    let author_ids = read_string_list(
        conn,
        "read cookbook authors",
        "select author_id from cookbook_authors where cookbook_id = $1 order by position",
        &row.id,
    )?;
    let shared_with_user_ids = read_string_list(
        conn,
        "read cookbook shares",
        "select user_id from cookbook_shares where cookbook_id = $1 order by user_id",
        &row.id,
    )?;
    Ok(Cookbook {
        id: row.id,
        title: row.title,
        author_ids,
        isbn: row.isbn,
        publisher: row.publisher,
        published_year: row.published_year,
        cover_image_url: row.cover_image_url,
        owner_user_id: row.owner_user_id,
        family_id: row.family_id,
        share_scope: share_scope(&row.share_scope),
        shared_with_user_ids,
    })
}

fn read_cookbook_imports(conn: &Connection) -> Result<Vec<CookbookImport>, StoreError> {
    query_rows(
        conn,
        "read cookbook imports",
        r"select id, cookbook_id, source_kind, source_path, status, ocr_engine,
             created_at, updated_at, review_notes
           from cookbook_imports
           order by created_at, id",
        [],
        |row| {
            let source_kind = row.get::<_, String>(2)?;
            let status = row.get::<_, String>(4)?;
            Ok(CookbookImport {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                source_kind: cookbook_source_kind(&source_kind),
                source_path: row.get(3)?,
                status: cookbook_import_status(&status),
                ocr_engine: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                review_notes: row.get(8)?,
            })
        },
    )
}

pub(crate) fn cookbook_pipeline_source(
    conn: &Connection,
    import_id: &str,
) -> Result<CookbookPipelineSource, StoreError> {
    let import_record = cookbook_import_by_id(conn, import_id)?;
    let cookbook = cookbook_by_id(conn, &import_record.cookbook_id)
        .map_err(|_| StoreError::CookbookNotFound)?;
    let pages = cookbook_pages_for_import(conn, import_id)?;
    if pages.is_empty() {
        return Err(StoreError::CookbookImportHasNoPages);
    }
    let sections = cookbook_sections_for_import(conn, &import_record.cookbook_id, import_id)?;
    let content_blocks =
        cookbook_content_blocks_for_import(conn, &import_record.cookbook_id, import_id)?;
    Ok(CookbookPipelineSource {
        import_record,
        cookbook,
        pages,
        sections,
        content_blocks,
    })
}

pub(crate) fn latest_cookbook_pipeline_source(
    conn: &Connection,
    cookbook_id: &str,
) -> Result<CookbookPipelineSource, StoreError> {
    let import_id = query_optional(
        conn,
        "read latest cookbook import",
        r"select id from cookbook_imports
           where cookbook_id = $1
           order by created_at desc, id desc
           limit 1",
        params![cookbook_id],
        |row| row.get::<_, String>(0),
    )?
    .ok_or(StoreError::CookbookImportNotFound)?;
    cookbook_pipeline_source(conn, &import_id)
}

fn cookbook_import_by_id(conn: &Connection, import_id: &str) -> Result<CookbookImport, StoreError> {
    query_optional(
        conn,
        "read cookbook import",
        r"select id, cookbook_id, source_kind, source_path, status, ocr_engine,
             created_at, updated_at, review_notes
           from cookbook_imports where id = $1",
        params![import_id],
        |row| {
            let source_kind = row.get::<_, String>(2)?;
            let status = row.get::<_, String>(4)?;
            Ok(CookbookImport {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                source_kind: cookbook_source_kind(&source_kind),
                source_path: row.get(3)?,
                status: cookbook_import_status(&status),
                ocr_engine: row.get(5)?,
                created_at: row.get(6)?,
                updated_at: row.get(7)?,
                review_notes: row.get(8)?,
            })
        },
    )?
    .ok_or(StoreError::CookbookImportNotFound)
}

fn cookbook_pages_for_import(
    conn: &Connection,
    import_id: &str,
) -> Result<Vec<CookbookPage>, StoreError> {
    query_rows(
        conn,
        "read cookbook import pages",
        r"select
             id, cookbook_id, import_id, image_index, printed_page_label, printed_page_number,
             image_path, image_hash, ocr_text, ocr_json, average_confidence, minimum_confidence,
             page_kind, review_status
           from cookbook_pages
           where import_id = $1
           order by image_index, id",
        params![import_id],
        full_cookbook_page,
    )
}

fn full_cookbook_page(row: &Row<'_>) -> duckdb::Result<CookbookPage> {
    let ocr_text: String = row.get(8)?;
    let page_kind: String = row.get(12)?;
    let review_status: String = row.get(13)?;
    Ok(CookbookPage {
        id: row.get(0)?,
        cookbook_id: row.get(1)?,
        import_id: row.get(2)?,
        image_index: row.get(3)?,
        printed_page_label: row.get(4)?,
        printed_page_number: row.get(5)?,
        image_path: row.get(6)?,
        image_hash: row.get(7)?,
        has_ocr_text: has_visible_text(&ocr_text),
        ocr_text,
        ocr_json: row.get(9)?,
        average_confidence: row.get(10)?,
        minimum_confidence: row.get(11)?,
        page_kind: cookbook_page_kind(&page_kind),
        review_status: cookbook_page_review_status(&review_status),
    })
}

fn cookbook_sections_for_import(
    conn: &Connection,
    cookbook_id: &str,
    import_id: &str,
) -> Result<Vec<CookbookSection>, StoreError> {
    let prefix = format!("{import_id}-section-%");
    query_rows(
        conn,
        "read generated cookbook sections",
        r"select id, cookbook_id, parent_section_id, title, kind, position, page_start, page_end
           from cookbook_sections
           where cookbook_id = $1 and id like $2
           order by position, id",
        params![cookbook_id, prefix],
        |row| {
            let kind = row.get::<_, String>(4)?;
            Ok(CookbookSection {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                parent_section_id: row.get(2)?,
                title: row.get(3)?,
                kind: cookbook_section_kind(&kind),
                position: row.get(5)?,
                page_start: row.get(6)?,
                page_end: row.get(7)?,
            })
        },
    )
}

fn cookbook_content_blocks_for_import(
    conn: &Connection,
    cookbook_id: &str,
    import_id: &str,
) -> Result<Vec<CookbookContentBlock>, StoreError> {
    let block_prefix = format!("{import_id}-block-%");
    let context_prefix = format!("{import_id}-context-%");
    let rows = query_rows(
        conn,
        "read generated cookbook content blocks",
        r"select
             id, cookbook_id, section_id, page_start, page_end, position, kind,
             title, text, confidence, source_json
           from cookbook_content_blocks
           where cookbook_id = $1 and (id like $2 or id like $3)
           order by position, id",
        params![cookbook_id, block_prefix, context_prefix],
        raw_content_block,
    )?;
    Ok(rows.into_iter().map(RawContentBlock::into_full).collect())
}

#[allow(clippy::similar_names)]
pub(crate) fn cookbook_import_progress(
    conn: &Connection,
    import_id: &str,
) -> Result<CookbookImportProgress, StoreError> {
    query_optional(
        conn,
        "read cookbook import progress",
        r"select import_id, state, stage, message, current_count, total_count,
             processed_count, skipped_count, failed_count, section_count,
             content_block_count, recipe_count, current_section_index, section_total,
             current_section_title, extraction_engine, error_message
           from cookbook_import_jobs where import_id = $1",
        params![import_id],
        |row| {
            let state: String = row.get(1)?;
            let stage: String = row.get(2)?;
            Ok(CookbookImportProgress {
                import_id: row.get(0)?,
                state: import_job_state(&state),
                stage: import_pipeline_stage(&stage),
                message: row.get(3)?,
                current: optional_usize(row.get(4)?),
                total: optional_usize(row.get(5)?),
                processed_count: nonnegative_usize(row.get(6)?),
                skipped_count: nonnegative_usize(row.get(7)?),
                failed_count: nonnegative_usize(row.get(8)?),
                section_count: nonnegative_usize(row.get(9)?),
                content_block_count: nonnegative_usize(row.get(10)?),
                recipe_count: nonnegative_usize(row.get(11)?),
                current_section_index: optional_usize(row.get(12)?),
                section_total: optional_usize(row.get(13)?),
                current_section_title: row.get(14)?,
                extraction_engine: row.get(15)?,
                error_message: row.get(16)?,
            })
        },
    )?
    .ok_or(StoreError::CookbookImportProgressNotFound)
}

pub(crate) fn recipe_import(
    conn: &Connection,
    import_id: &str,
) -> Result<RecipeImport, StoreError> {
    let row = query_optional(
        conn,
        "read recipe import",
        r"select id, status, file_name, mime_type, image_path, ocr_engine, ocr_text,
             ocr_json, draft_json, validation_issues_json, created_at, updated_at
           from recipe_imports where id = $1",
        params![import_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<String>>(8)?,
                row.get::<_, String>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        },
    )?
    .ok_or(StoreError::RecipeImportNotFound)?;
    let draft = row
        .8
        .as_deref()
        .map(|value| json_value(value, "recipe import draft"))
        .transpose()?;
    let validation_issues =
        json_value::<Vec<ImportIssue>>(&row.9, "recipe import validation issues")?;
    Ok(RecipeImport {
        id: row.0,
        status: recipe_import_status(&row.1),
        file_name: row.2,
        mime_type: row.3,
        image_path: row.4,
        ocr_engine: row.5,
        ocr_text: row.6,
        ocr_json: row.7,
        draft,
        validation_issues,
        created_at: row.10,
        updated_at: row.11,
    })
}

#[derive(Debug)]
struct RawCookbookPage {
    id: String,
    cookbook_id: String,
    import_id: String,
    image_index: u32,
    printed_page_label: Option<String>,
    printed_page_number: Option<u32>,
    image_path: String,
    image_hash: Option<String>,
    ocr_text: String,
    average_confidence: Option<f64>,
    minimum_confidence: Option<f64>,
    page_kind: String,
    review_status: String,
}

fn read_cookbook_pages_summary(conn: &Connection) -> Result<Vec<CookbookPage>, StoreError> {
    let rows = query_rows(
        conn,
        "read cookbook pages",
        r"select
             id, cookbook_id, import_id, image_index, printed_page_label, printed_page_number,
             image_path, image_hash, ocr_text, average_confidence, minimum_confidence,
             page_kind, review_status
           from cookbook_pages
           order by cookbook_id, image_index, id",
        [],
        |row| {
            Ok(RawCookbookPage {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                import_id: row.get(2)?,
                image_index: row.get(3)?,
                printed_page_label: row.get(4)?,
                printed_page_number: row.get(5)?,
                image_path: row.get(6)?,
                image_hash: row.get(7)?,
                ocr_text: row.get(8)?,
                average_confidence: row.get(9)?,
                minimum_confidence: row.get(10)?,
                page_kind: row.get(11)?,
                review_status: row.get(12)?,
            })
        },
    )?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let has_ocr_text = has_visible_text(&row.ocr_text);
            CookbookPage {
                id: row.id,
                cookbook_id: row.cookbook_id,
                import_id: row.import_id,
                image_index: row.image_index,
                printed_page_label: row.printed_page_label,
                printed_page_number: row.printed_page_number,
                image_path: row.image_path,
                image_hash: row.image_hash,
                ocr_text: summary_text_preview(&row.ocr_text),
                ocr_json: "{}".to_owned(),
                has_ocr_text,
                average_confidence: row.average_confidence,
                minimum_confidence: row.minimum_confidence,
                page_kind: cookbook_page_kind(&row.page_kind),
                review_status: cookbook_page_review_status(&row.review_status),
            }
        })
        .collect())
}

fn read_cookbook_sections(conn: &Connection) -> Result<Vec<CookbookSection>, StoreError> {
    query_rows(
        conn,
        "read cookbook sections",
        r"select id, cookbook_id, parent_section_id, title, kind, position, page_start, page_end
           from cookbook_sections
           order by cookbook_id, position, id",
        [],
        |row| {
            let kind = row.get::<_, String>(4)?;
            Ok(CookbookSection {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                parent_section_id: row.get(2)?,
                title: row.get(3)?,
                kind: cookbook_section_kind(&kind),
                position: row.get(5)?,
                page_start: row.get(6)?,
                page_end: row.get(7)?,
            })
        },
    )
}

#[derive(Debug)]
struct RawContentBlock {
    id: String,
    cookbook_id: String,
    section_id: Option<String>,
    page_start: Option<u32>,
    page_end: Option<u32>,
    position: u32,
    kind: String,
    title: Option<String>,
    text: String,
    confidence: Option<f64>,
    source_json: String,
}

impl RawContentBlock {
    fn into_summary(self) -> CookbookContentBlock {
        let has_text = has_visible_text(&self.text);
        CookbookContentBlock {
            id: self.id,
            cookbook_id: self.cookbook_id,
            section_id: self.section_id,
            page_start: self.page_start,
            page_end: self.page_end,
            position: self.position,
            kind: cookbook_content_block_kind(&self.kind),
            title: self.title,
            text: summary_text_preview(&self.text),
            has_text,
            confidence: self.confidence,
            source_json: "{}".to_owned(),
        }
    }

    fn into_full(self) -> CookbookContentBlock {
        let has_text = has_visible_text(&self.text);
        CookbookContentBlock {
            id: self.id,
            cookbook_id: self.cookbook_id,
            section_id: self.section_id,
            page_start: self.page_start,
            page_end: self.page_end,
            position: self.position,
            kind: cookbook_content_block_kind(&self.kind),
            title: self.title,
            text: self.text,
            has_text,
            confidence: self.confidence,
            source_json: self.source_json,
        }
    }
}

fn raw_content_block(row: &Row<'_>) -> duckdb::Result<RawContentBlock> {
    Ok(RawContentBlock {
        id: row.get(0)?,
        cookbook_id: row.get(1)?,
        section_id: row.get(2)?,
        page_start: row.get(3)?,
        page_end: row.get(4)?,
        position: row.get(5)?,
        kind: row.get(6)?,
        title: row.get(7)?,
        text: row.get(8)?,
        confidence: row.get(9)?,
        source_json: row.get(10)?,
    })
}

fn read_cookbook_content_blocks_summary(
    conn: &Connection,
) -> Result<Vec<CookbookContentBlock>, StoreError> {
    let rows = query_rows(
        conn,
        "read cookbook content blocks",
        r"select
             id, cookbook_id, section_id, page_start, page_end, position, kind,
             title, text, confidence, source_json
           from cookbook_content_blocks
           order by cookbook_id, position, id",
        [],
        raw_content_block,
    )?;
    Ok(rows
        .into_iter()
        .map(RawContentBlock::into_summary)
        .collect())
}

#[derive(Debug)]
struct RawMenu {
    id: String,
    cookbook_id: String,
    source_block_id: Option<String>,
    title: String,
    theme: Option<String>,
    notes: Option<String>,
}

fn read_cookbook_menus(conn: &Connection) -> Result<Vec<CookbookMenu>, StoreError> {
    let rows = query_rows(
        conn,
        "read cookbook menus",
        r"select id, cookbook_id, source_block_id, title, theme, notes
           from cookbook_menus
           order by cookbook_id, title",
        [],
        |row| {
            Ok(RawMenu {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                source_block_id: row.get(2)?,
                title: row.get(3)?,
                theme: row.get(4)?,
                notes: row.get(5)?,
            })
        },
    )?;

    rows.into_iter()
        .map(|row| {
            Ok(CookbookMenu {
                recipes: read_cookbook_menu_recipes(conn, &row.id)?,
                id: row.id,
                cookbook_id: row.cookbook_id,
                source_block_id: row.source_block_id,
                title: row.title,
                theme: row.theme,
                notes: row.notes,
            })
        })
        .collect()
}

fn read_cookbook_menu_recipes(
    conn: &Connection,
    menu_id: &str,
) -> Result<Vec<CookbookMenuRecipe>, StoreError> {
    query_rows(
        conn,
        "read cookbook menu recipes",
        r"select recipe_id, position, role, serving_notes
           from cookbook_menu_recipes
           where menu_id = $1
           order by position, recipe_id",
        params![menu_id],
        |row| {
            Ok(CookbookMenuRecipe {
                recipe_id: row.get(0)?,
                position: row.get(1)?,
                role: row.get(2)?,
                serving_notes: row.get(3)?,
            })
        },
    )
}

fn read_cookbook_glossary_entries(
    conn: &Connection,
) -> Result<Vec<CookbookGlossaryEntry>, StoreError> {
    #[derive(Debug)]
    struct RawGlossaryEntry {
        id: String,
        cookbook_id: String,
        source_block_id: Option<String>,
        title: String,
        aliases_json: String,
        native_names_json: String,
        description: String,
        storage_notes: Option<String>,
        substitution_notes: Option<String>,
        page_start: Option<u32>,
        page_end: Option<u32>,
    }

    let rows = query_rows(
        conn,
        "read cookbook glossary entries",
        r"select
             id, cookbook_id, source_block_id, title, aliases_json, native_names_json,
             description, storage_notes, substitution_notes, page_start, page_end
           from cookbook_glossary_entries
           order by cookbook_id, title",
        [],
        |row| {
            Ok(RawGlossaryEntry {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                source_block_id: row.get(2)?,
                title: row.get(3)?,
                aliases_json: row.get(4)?,
                native_names_json: row.get(5)?,
                description: row.get(6)?,
                storage_notes: row.get(7)?,
                substitution_notes: row.get(8)?,
                page_start: row.get(9)?,
                page_end: row.get(10)?,
            })
        },
    )?;

    rows.into_iter()
        .map(|row| {
            Ok(CookbookGlossaryEntry {
                aliases: json_string_list(
                    &row.aliases_json,
                    "cookbook_glossary_entries.aliases_json",
                )?,
                native_names: json_string_list(
                    &row.native_names_json,
                    "cookbook_glossary_entries.native_names_json",
                )?,
                id: row.id,
                cookbook_id: row.cookbook_id,
                source_block_id: row.source_block_id,
                title: row.title,
                description: row.description,
                storage_notes: row.storage_notes,
                substitution_notes: row.substitution_notes,
                page_start: row.page_start,
                page_end: row.page_end,
            })
        })
        .collect()
}

fn read_cookbook_suppliers(conn: &Connection) -> Result<Vec<CookbookSupplier>, StoreError> {
    query_rows(
        conn,
        "read cookbook suppliers",
        r"select id, cookbook_id, source_block_id, name, url, region, notes, source_page, review_status
           from cookbook_suppliers
           order by cookbook_id, region, name",
        [],
        |row| {
            let review_status = row.get::<_, String>(8)?;
            Ok(CookbookSupplier {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                source_block_id: row.get(2)?,
                name: row.get(3)?,
                url: row.get(4)?,
                region: row.get(5)?,
                notes: row.get(6)?,
                source_page: row.get(7)?,
                review_status: cookbook_page_review_status(&review_status),
            })
        },
    )
}

fn read_cookbook_index_entries(conn: &Connection) -> Result<Vec<CookbookIndexEntry>, StoreError> {
    query_rows(
        conn,
        "read cookbook index entries",
        r"select
             id, cookbook_id, term, subterm, target_page_label, target_page_number,
             target_recipe_id, illustration
           from cookbook_index_entries
           order by cookbook_id, term, subterm, target_page_number",
        [],
        |row| {
            Ok(CookbookIndexEntry {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                term: row.get(2)?,
                subterm: row.get(3)?,
                target_page_label: row.get(4)?,
                target_page_number: row.get(5)?,
                target_recipe_id: row.get(6)?,
                illustration: row.get(7)?,
            })
        },
    )
}

fn read_cookbook_cross_references(
    conn: &Connection,
) -> Result<Vec<CookbookCrossReference>, StoreError> {
    query_rows(
        conn,
        "read cookbook cross references",
        r"select id, cookbook_id, from_kind, from_id, to_kind, to_id, label, relation_kind
           from cookbook_cross_references
           order by cookbook_id, relation_kind, id",
        [],
        |row| {
            Ok(CookbookCrossReference {
                id: row.get(0)?,
                cookbook_id: row.get(1)?,
                from_kind: row.get(2)?,
                from_id: row.get(3)?,
                to_kind: row.get(4)?,
                to_id: row.get(5)?,
                label: row.get(6)?,
                relation_kind: row.get(7)?,
            })
        },
    )
}

#[derive(Debug)]
struct RawRecipe {
    id: String,
    title: String,
    cookbook_id: String,
    source_label: String,
    page_start: Option<u32>,
    page_end: Option<u32>,
    yield_quantity: Option<f64>,
    yield_unit: Option<String>,
    prep_minutes: Option<u32>,
    cook_minutes: Option<u32>,
    total_minutes: Option<u32>,
    cuisine: Option<String>,
    category: Option<String>,
    searchable_text: String,
    last_made_at: Option<String>,
    times_made: u32,
    cost_cents: Option<u64>,
    cost_per_serving_cents: Option<u64>,
    cache_key: String,
    cache_updated_at: Option<String>,
    subtitle: Option<String>,
    headnote: Option<String>,
    serving_context: Option<String>,
    source_block_id: Option<String>,
    pictured_page_number: Option<u32>,
    extraction_status: String,
}

fn read_recipes(conn: &Connection) -> Result<Vec<Recipe>, StoreError> {
    let rows = query_rows(
        conn,
        "read recipes",
        r"select
             id, title, cookbook_id, source_label, page_start, page_end,
             yield_quantity, yield_unit, prep_minutes, cook_minutes, total_minutes,
             cuisine, category, searchable_text, last_made_at, times_made,
             cost_cents, cost_per_serving_cents, cache_key, cache_updated_at,
             subtitle, headnote, serving_context, source_block_id, pictured_page_number,
             coalesce(extraction_status, 'verified')
           from recipes
           order by title",
        [],
        raw_recipe,
    )?;

    rows.into_iter().map(|row| read_recipe(conn, row)).collect()
}

pub(crate) fn recipe_by_id(conn: &Connection, id: &str) -> Result<Recipe, StoreError> {
    let row = query_optional(
        conn,
        "read recipe",
        r"select
             id, title, cookbook_id, source_label, page_start, page_end,
             yield_quantity, yield_unit, prep_minutes, cook_minutes, total_minutes,
             cuisine, category, searchable_text, last_made_at, times_made,
             cost_cents, cost_per_serving_cents, cache_key, cache_updated_at,
             subtitle, headnote, serving_context, source_block_id, pictured_page_number,
             coalesce(extraction_status, 'verified')
           from recipes
           where id = $1",
        params![id],
        raw_recipe,
    )?
    .ok_or(StoreError::RecipeNotFound)?;
    read_recipe(conn, row)
}

fn raw_recipe(row: &Row<'_>) -> duckdb::Result<RawRecipe> {
    Ok(RawRecipe {
        id: row.get(0)?,
        title: row.get(1)?,
        cookbook_id: row.get(2)?,
        source_label: row.get(3)?,
        page_start: row.get(4)?,
        page_end: row.get(5)?,
        yield_quantity: row.get(6)?,
        yield_unit: row.get(7)?,
        prep_minutes: row.get(8)?,
        cook_minutes: row.get(9)?,
        total_minutes: row.get(10)?,
        cuisine: row.get(11)?,
        category: row.get(12)?,
        searchable_text: row.get(13)?,
        last_made_at: row.get(14)?,
        times_made: row.get(15)?,
        cost_cents: row.get(16)?,
        cost_per_serving_cents: row.get(17)?,
        cache_key: row.get(18)?,
        cache_updated_at: row.get(19)?,
        subtitle: row.get(20)?,
        headnote: row.get(21)?,
        serving_context: row.get(22)?,
        source_block_id: row.get(23)?,
        pictured_page_number: row.get(24)?,
        extraction_status: row.get(25)?,
    })
}

fn read_recipe(conn: &Connection, row: RawRecipe) -> Result<Recipe, StoreError> {
    let recipe_id = &row.id;
    Ok(Recipe {
        alternate_names: read_alternate_names(conn, recipe_id)?,
        author_ids: read_string_list(
            conn,
            "read recipe authors",
            "select author_id from recipe_authors where recipe_id = $1 order by position",
            recipe_id,
        )?,
        tags: read_string_list(
            conn,
            "read recipe tags",
            "select tag from recipe_tags where recipe_id = $1 order by position",
            recipe_id,
        )?,
        source_page_spans: read_source_page_spans(conn, recipe_id)?,
        component_recipe_ids: read_string_list(
            conn,
            "read recipe components",
            "select component_recipe_id from recipe_components where recipe_id = $1 order by position",
            recipe_id,
        )?,
        images: read_images(conn, recipe_id)?,
        ingredients: read_ingredients(conn, recipe_id)?,
        steps: read_steps(conn, recipe_id)?,
        notes: read_notes(conn, recipe_id)?,
        id: row.id,
        title: row.title,
        subtitle: row.subtitle,
        cookbook_id: row.cookbook_id,
        page_start: row.page_start,
        page_end: row.page_end,
        source_label: row.source_label,
        headnote: row.headnote,
        serving_context: row.serving_context,
        yield_quantity: row.yield_quantity,
        yield_unit: row.yield_unit,
        prep_minutes: row.prep_minutes,
        cook_minutes: row.cook_minutes,
        total_minutes: row.total_minutes,
        cuisine: row.cuisine,
        category: row.category,
        searchable_text: row.searchable_text,
        source_block_id: row.source_block_id,
        pictured_page_number: row.pictured_page_number,
        extraction_status: recipe_extraction_status(&row.extraction_status),
        last_made_at: row.last_made_at,
        times_made: row.times_made,
        cost_cents: row.cost_cents,
        cost_per_serving_cents: row.cost_per_serving_cents,
        cache_key: row.cache_key,
        cache_updated_at: row.cache_updated_at,
    })
}

fn read_alternate_names(
    conn: &Connection,
    recipe_id: &str,
) -> Result<Vec<RecipeAlternateName>, StoreError> {
    query_rows(
        conn,
        "read recipe alternate names",
        r"select kind, value
           from recipe_alternate_names
           where recipe_id = $1
           order by position",
        params![recipe_id],
        |row| {
            Ok(RecipeAlternateName {
                kind: row.get(0)?,
                value: row.get(1)?,
            })
        },
    )
}

fn read_source_page_spans(
    conn: &Connection,
    recipe_id: &str,
) -> Result<Vec<RecipeSourcePageSpan>, StoreError> {
    query_rows(
        conn,
        "read recipe source page spans",
        r"select page_id, printed_page_number, line_start, line_end, confidence
           from recipe_source_page_spans
           where recipe_id = $1
           order by position",
        params![recipe_id],
        |row| {
            Ok(RecipeSourcePageSpan {
                page_id: row.get(0)?,
                printed_page_number: row.get(1)?,
                line_start: row.get(2)?,
                line_end: row.get(3)?,
                confidence: row.get(4)?,
            })
        },
    )
}

fn read_ingredients(conn: &Connection, recipe_id: &str) -> Result<Vec<Ingredient>, StoreError> {
    query_rows(
        conn,
        "read recipe ingredients",
        r"select ingredient_id, coalesce(position, 0), display_name, item, quantity, unit,
             preparation, section, coalesce(optional, false), alternative_text, source_line,
             source_page_id, unit_cost_cents, estimated_cost_cents, quantity_text, quantity_min,
             quantity_max, coalesce(quantity_kind, 'exact'), coalesce(quantity_review_status, 'parsed'),
             quantity_review_reason
           from recipe_ingredients
           where recipe_id = $1
           order by coalesce(position, 0), ingredient_id",
        params![recipe_id],
        |row| {
            let quantity_kind = row.get::<_, String>(17)?;
            let quantity_review_status = row.get::<_, String>(18)?;
            Ok(Ingredient {
                id: row.get(0)?,
                position: Some(row.get(1)?),
                display_name: row.get(2)?,
                item: row.get(3)?,
                quantity: row.get(4)?,
                unit: row.get(5)?,
                preparation: row.get(6)?,
                section: row.get(7)?,
                optional: row.get(8)?,
                alternative_text: row.get(9)?,
                source_line: row.get(10)?,
                source_page_id: row.get(11)?,
                unit_cost_cents: row.get(12)?,
                estimated_cost_cents: row.get(13)?,
                quantity_text: row.get(14)?,
                quantity_min: row.get(15)?,
                quantity_max: row.get(16)?,
                quantity_kind: ingredient_quantity_kind(&quantity_kind),
                quantity_review_status: ingredient_quantity_review_status(&quantity_review_status),
                quantity_review_reason: row.get(19)?,
            })
        },
    )
}

fn read_steps(conn: &Connection, recipe_id: &str) -> Result<Vec<InstructionStep>, StoreError> {
    query_rows(
        conn,
        "read recipe steps",
        r"select step_id, position, section, text, source_page_id, source_line_start, source_line_end
           from recipe_steps
           where recipe_id = $1
           order by position",
        params![recipe_id],
        |row| {
            Ok(InstructionStep {
                id: row.get(0)?,
                position: row.get(1)?,
                section: row.get(2)?,
                text: row.get(3)?,
                source_page_id: row.get(4)?,
                source_line_start: row.get(5)?,
                source_line_end: row.get(6)?,
            })
        },
    )
}

fn read_images(conn: &Connection, recipe_id: &str) -> Result<Vec<RecipeImage>, StoreError> {
    query_rows(
        conn,
        "read recipe images",
        r"select image_id, url, alt, credit, is_primary
           from recipe_images
           where recipe_id = $1
           order by is_primary desc, image_id",
        params![recipe_id],
        |row| {
            Ok(RecipeImage {
                id: row.get(0)?,
                url: row.get(1)?,
                alt: row.get(2)?,
                credit: row.get(3)?,
                is_primary: row.get(4)?,
            })
        },
    )
}

fn read_notes(conn: &Connection, recipe_id: &str) -> Result<Vec<RecipeNote>, StoreError> {
    query_rows(
        conn,
        "read recipe notes",
        r"select note_id, text, created_at
           from recipe_notes
           where recipe_id = $1
           order by created_at, note_id",
        params![recipe_id],
        |row| {
            Ok(RecipeNote {
                id: row.get(0)?,
                text: row.get(1)?,
                created_at: row.get(2)?,
            })
        },
    )
}

fn read_string_list(
    conn: &Connection,
    operation: &'static str,
    sql: &str,
    owner_id: &str,
) -> Result<Vec<String>, StoreError> {
    query_rows(conn, operation, sql, params![owner_id], |row| row.get(0))
}

fn json_string_list(value: &str, context: &'static str) -> Result<Vec<String>, StoreError> {
    json_value(value, context)
}

fn json_value<T: DeserializeOwned>(value: &str, context: &'static str) -> Result<T, StoreError> {
    serde_json::from_str(value).map_err(|error| StoreError::InvalidJson {
        context,
        detail: error.to_string(),
    })
}

fn optional_usize(value: Option<i64>) -> Option<usize> {
    value.and_then(|value| usize::try_from(value).ok())
}

fn nonnegative_usize(value: i64) -> usize {
    usize::try_from(value).unwrap_or_default()
}

fn summary_text_preview(text: &str) -> String {
    if text.len() <= SUMMARY_TEXT_PREVIEW_BYTES {
        return text.to_owned();
    }
    let mut end = SUMMARY_TEXT_PREVIEW_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_owned()
}

fn has_visible_text(text: &str) -> bool {
    !text.trim().is_empty()
}

fn query_rows<T, P, F>(
    conn: &Connection,
    operation: &'static str,
    sql: &str,
    params: P,
    map: F,
) -> Result<Vec<T>, StoreError>
where
    P: Params,
    F: FnMut(&Row<'_>) -> duckdb::Result<T>,
{
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| backend(operation, &error))?;
    let rows = statement
        .query_map(params, map)
        .map_err(|error| backend(operation, &error))?;
    rows.collect::<duckdb::Result<Vec<_>>>()
        .map_err(|error| backend(operation, &error))
}

fn query_optional<T, P, F>(
    conn: &Connection,
    operation: &'static str,
    sql: &str,
    params: P,
    map: F,
) -> Result<Option<T>, StoreError>
where
    P: Params,
    F: FnOnce(&Row<'_>) -> duckdb::Result<T>,
{
    let mut statement = conn
        .prepare(sql)
        .map_err(|error| backend(operation, &error))?;
    let mut rows = statement
        .query(params)
        .map_err(|error| backend(operation, &error))?;
    let row = rows.next().map_err(|error| backend(operation, &error))?;
    row.map(map)
        .transpose()
        .map_err(|error| backend(operation, &error))
}

fn backend(operation: &'static str, error: &duckdb::Error) -> StoreError {
    StoreError::Backend {
        operation,
        detail: error.to_string(),
    }
}

fn share_scope(value: &str) -> ShareScope {
    match value {
        "family" => ShareScope::Family,
        "users" => ShareScope::Users,
        _ => ShareScope::Personal,
    }
}

fn cookbook_source_kind(value: &str) -> CookbookSourceKind {
    match value {
        "image_set" => CookbookSourceKind::ImageSet,
        "pdf" => CookbookSourceKind::Pdf,
        "web" => CookbookSourceKind::Web,
        _ => CookbookSourceKind::Manual,
    }
}

fn cookbook_import_status(value: &str) -> CookbookImportStatus {
    match value {
        "ocr_ready" => CookbookImportStatus::OcrReady,
        "mapped" => CookbookImportStatus::Mapped,
        "reviewed" => CookbookImportStatus::Reviewed,
        "committed" => CookbookImportStatus::Committed,
        _ => CookbookImportStatus::Uploaded,
    }
}

fn import_job_state(value: &str) -> ImportJobState {
    match value {
        "running" => ImportJobState::Running,
        "complete" => ImportJobState::Complete,
        "canceled" => ImportJobState::Canceled,
        _ => ImportJobState::Failed,
    }
}

fn import_pipeline_stage(value: &str) -> ImportPipelineStage {
    match value {
        "queued" => ImportPipelineStage::Queued,
        "loading_pages" => ImportPipelineStage::LoadingPages,
        "ocr_pages" => ImportPipelineStage::OcrPages,
        "source_map" => ImportPipelineStage::SourceMap,
        "deepseek_plan" => ImportPipelineStage::DeepseekPlan,
        "deepseek_section" => ImportPipelineStage::DeepseekSection,
        "normalizing" => ImportPipelineStage::Normalizing,
        "persisting" => ImportPipelineStage::Persisting,
        "complete" => ImportPipelineStage::Complete,
        "canceled" => ImportPipelineStage::Canceled,
        _ => ImportPipelineStage::Failed,
    }
}

fn recipe_import_status(value: &str) -> RecipeImportStatus {
    match value {
        "processing" => RecipeImportStatus::Processing,
        "draft_ready" => RecipeImportStatus::DraftReady,
        "committed" => RecipeImportStatus::Committed,
        _ => RecipeImportStatus::Failed,
    }
}

fn cookbook_page_kind(value: &str) -> CookbookPageKind {
    match value {
        "cover" => CookbookPageKind::Cover,
        "title" => CookbookPageKind::Title,
        "contents" => CookbookPageKind::Contents,
        "chapter_opener" => CookbookPageKind::ChapterOpener,
        "essay" => CookbookPageKind::Essay,
        "reference" => CookbookPageKind::Reference,
        "recipe" => CookbookPageKind::Recipe,
        "supplier" => CookbookPageKind::Supplier,
        "index" => CookbookPageKind::Index,
        "acknowledgements" => CookbookPageKind::Acknowledgements,
        "blank" => CookbookPageKind::Blank,
        _ => CookbookPageKind::Unknown,
    }
}

fn cookbook_page_review_status(value: &str) -> CookbookPageReviewStatus {
    match value {
        "accepted" => CookbookPageReviewStatus::Accepted,
        "needs_crop" => CookbookPageReviewStatus::NeedsCrop,
        "needs_ocr_fix" => CookbookPageReviewStatus::NeedsOcrFix,
        "ignored" => CookbookPageReviewStatus::Ignored,
        _ => CookbookPageReviewStatus::Pending,
    }
}

fn cookbook_section_kind(value: &str) -> CookbookSectionKind {
    match value {
        "front_matter" => CookbookSectionKind::FrontMatter,
        "chapter" => CookbookSectionKind::Chapter,
        "essay" => CookbookSectionKind::Essay,
        "reference" => CookbookSectionKind::Reference,
        "back_matter" => CookbookSectionKind::BackMatter,
        _ => CookbookSectionKind::Recipes,
    }
}

fn cookbook_content_block_kind(value: &str) -> CookbookContentBlockKind {
    match value {
        "recipe" => CookbookContentBlockKind::Recipe,
        "recipe_headnote" => CookbookContentBlockKind::RecipeHeadnote,
        "ingredient_glossary_entry" => CookbookContentBlockKind::IngredientGlossaryEntry,
        "menu" => CookbookContentBlockKind::Menu,
        "supplier" => CookbookContentBlockKind::Supplier,
        "index_entry" => CookbookContentBlockKind::IndexEntry,
        "caption" => CookbookContentBlockKind::Caption,
        "callout" => CookbookContentBlockKind::Callout,
        _ => CookbookContentBlockKind::Paragraph,
    }
}

fn ingredient_quantity_kind(value: &str) -> IngredientQuantityKind {
    match value {
        "range" => IngredientQuantityKind::Range,
        "as_needed" => IngredientQuantityKind::AsNeeded,
        "unknown" => IngredientQuantityKind::Unknown,
        _ => IngredientQuantityKind::Exact,
    }
}

fn ingredient_quantity_review_status(value: &str) -> IngredientQuantityReviewStatus {
    match value {
        "needs_review" => IngredientQuantityReviewStatus::NeedsReview,
        _ => IngredientQuantityReviewStatus::Parsed,
    }
}

fn recipe_extraction_status(value: &str) -> RecipeExtractionStatus {
    match value {
        "draft" => RecipeExtractionStatus::Draft,
        "needs_review" => RecipeExtractionStatus::NeedsReview,
        _ => RecipeExtractionStatus::Verified,
    }
}

fn pantry_category(value: &str) -> PantryCategory {
    match value {
        "prepared" => PantryCategory::Prepared,
        "leftover" => PantryCategory::Leftover,
        _ => PantryCategory::Raw,
    }
}

fn meal_type_value(value: &str) -> MealType {
    match value {
        "breakfast" => MealType::Breakfast,
        "lunch" => MealType::Lunch,
        _ => MealType::Dinner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_preview_preserves_utf8_boundaries() {
        let input = format!("{}éafter", "x".repeat(SUMMARY_TEXT_PREVIEW_BYTES - 1));
        let preview = summary_text_preview(&input);

        assert_eq!(preview.len(), SUMMARY_TEXT_PREVIEW_BYTES - 1);
        assert!(input.starts_with(&preview));
    }

    #[test]
    fn visible_text_ignores_only_whitespace() {
        assert!(!has_visible_text(" \n\t"));
        assert!(has_visible_text("  recipe "));
    }
}
