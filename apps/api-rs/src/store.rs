use thiserror::Error;

use crate::{
    config::StoreMode,
    model::{
        AcceptPageContentInput, Catalogue, CookLogEntry, Cookbook, CookbookContentBlock,
        CookbookContentBlockPatch, CookbookImport, CookbookImportProgress, CookbookPage,
        CookbookPagePatch, CookbookPageText, CookbookSection, CookbookSourceImport, ImportIssue,
        MarkMadeInput, MealPlanEntry, PantryItem, PantryItemPatch, Recipe, RecipeImport,
        RecipeValidationError,
    },
};

pub trait StoreProbe: Send + Sync {
    /// Runs the minimum query needed to prove that the store is responsive.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the connection or readiness query fails.
    fn ping(&self) -> Result<(), StoreError>;
}

#[allow(clippy::missing_errors_doc)]
pub trait ReadStore: StoreProbe {
    /// Loads the compact catalogue representation used by the main web view.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when any catalogue table or nested row cannot be read.
    fn catalogue_summary(&self) -> Result<Catalogue, StoreError>;

    /// Loads all pantry rows in the same order as the Zig service.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the pantry query fails.
    fn pantry_items(&self) -> Result<Vec<PantryItem>, StoreError>;

    /// Loads meal-plan entries in date and meal-type order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the meal-plan query fails.
    fn meal_plan_entries(&self) -> Result<Vec<MealPlanEntry>, StoreError>;

    /// Loads cook-log entries newest first, including substitutions.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the cook-log query fails.
    fn cook_log_entries(&self) -> Result<Vec<CookLogEntry>, StoreError>;

    /// Loads full OCR text and source JSON for one cookbook page.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::CookbookPageNotFound`] when the id is unknown, or
    /// another [`StoreError`] when the query fails.
    fn cookbook_page_text(&self, page_id: &str) -> Result<CookbookPageText, StoreError>;

    /// Resolves the original image path and cache hash for one cookbook page.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::CookbookPageNotFound`] when the id is unknown, or
    /// another [`StoreError`] when the query fails.
    fn cookbook_page_image(&self, page_id: &str) -> Result<CookbookPageImage, StoreError>;

    /// Loads full content blocks for one cookbook in document order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the content-block query fails. An unknown
    /// cookbook intentionally returns an empty vector for Zig parity.
    fn cookbook_content_blocks(
        &self,
        cookbook_id: &str,
    ) -> Result<Vec<CookbookContentBlock>, StoreError>;

    fn cookbook_pipeline_source(
        &self,
        import_id: &str,
    ) -> Result<CookbookPipelineSource, StoreError> {
        let _ = import_id;
        Err(StoreError::Unavailable(
            "cookbook pipeline source reads are unsupported".to_owned(),
        ))
    }

    fn latest_cookbook_pipeline_source(
        &self,
        cookbook_id: &str,
    ) -> Result<CookbookPipelineSource, StoreError> {
        let _ = cookbook_id;
        Err(StoreError::Unavailable(
            "cookbook diagnostic source reads are unsupported".to_owned(),
        ))
    }

    fn cookbook_import_progress(
        &self,
        import_id: &str,
    ) -> Result<CookbookImportProgress, StoreError> {
        let _ = import_id;
        Err(StoreError::CookbookImportProgressNotFound)
    }

    fn recipe_import(&self, import_id: &str) -> Result<RecipeImport, StoreError> {
        let _ = import_id;
        Err(StoreError::RecipeImportNotFound)
    }
}

#[allow(clippy::missing_errors_doc)]
pub trait WriteStore: Send + Sync {
    fn create_cookbook(&self, cookbook: Cookbook) -> Result<Cookbook, StoreError>;
    fn create_recipe(&self, recipe: Recipe, cache_updated_at: &str) -> Result<Recipe, StoreError>;
    fn update_recipe(&self, recipe: Recipe, cache_updated_at: &str) -> Result<Recipe, StoreError>;
    fn delete_recipe(&self, id: &str) -> Result<(), StoreError>;
    fn mark_recipe_made(
        &self,
        id: &str,
        made_at: &str,
        details: MarkMadeInput,
        cache_updated_at: &str,
    ) -> Result<Recipe, StoreError>;
    fn add_pantry_item(&self, item: PantryItem) -> Result<PantryItem, StoreError>;
    fn patch_pantry_item(&self, id: &str, patch: PantryItemPatch)
    -> Result<PantryItem, StoreError>;
    fn delete_pantry_item(&self, id: &str) -> Result<(), StoreError>;
    fn add_meal_plan_entry(&self, entry: MealPlanEntry) -> Result<MealPlanEntry, StoreError>;
    fn delete_meal_plan_entry(&self, id: &str) -> Result<(), StoreError>;
    fn cookbook_exists(&self, id: &str) -> Result<bool, StoreError> {
        let _ = id;
        Err(StoreError::Unavailable(
            "cookbook lookup is unsupported".to_owned(),
        ))
    }
    fn cookbook_page_image_hash_exists(&self, image_hash: &str) -> Result<bool, StoreError> {
        let _ = image_hash;
        Err(StoreError::Unavailable(
            "cookbook image lookup is unsupported".to_owned(),
        ))
    }
    fn create_cookbook_source_import(
        &self,
        source: CookbookSourceImport,
    ) -> Result<(), StoreError> {
        let _ = source;
        Err(StoreError::Unavailable(
            "cookbook source import is unsupported".to_owned(),
        ))
    }
    fn patch_cookbook_page(
        &self,
        id: &str,
        patch: CookbookPagePatch,
    ) -> Result<CookbookPage, StoreError> {
        let _ = (id, patch);
        Err(StoreError::Unavailable(
            "cookbook page patch is unsupported".to_owned(),
        ))
    }
    fn patch_cookbook_content_block(
        &self,
        id: &str,
        patch: CookbookContentBlockPatch,
    ) -> Result<CookbookContentBlock, StoreError> {
        let _ = (id, patch);
        Err(StoreError::Unavailable(
            "cookbook content block patch is unsupported".to_owned(),
        ))
    }
    fn accept_cookbook_page_content(
        &self,
        id: &str,
        input: AcceptPageContentInput,
    ) -> Result<CookbookContentBlock, StoreError> {
        let _ = (id, input);
        Err(StoreError::Unavailable(
            "cookbook page acceptance is unsupported".to_owned(),
        ))
    }

    fn upsert_cookbook_import_progress(
        &self,
        progress: CookbookImportProgress,
        updated_at: &str,
    ) -> Result<(), StoreError> {
        let _ = (progress, updated_at);
        Err(StoreError::Unavailable(
            "cookbook import progress writes are unsupported".to_owned(),
        ))
    }

    fn persist_cookbook_pipeline(
        &self,
        result: CookbookPipelineResult,
        cache_updated_at: &str,
    ) -> Result<CookbookExtractionPersistResult, StoreError> {
        let _ = (result, cache_updated_at);
        Err(StoreError::Unavailable(
            "cookbook pipeline persistence is unsupported".to_owned(),
        ))
    }

    fn create_recipe_import(
        &self,
        recipe_import: RecipeImport,
    ) -> Result<RecipeImport, StoreError> {
        let _ = recipe_import;
        Err(StoreError::Unavailable(
            "recipe imports are unsupported".to_owned(),
        ))
    }

    fn update_recipe_import_draft(
        &self,
        import_id: &str,
        recipe: Recipe,
        issues: Vec<ImportIssue>,
        updated_at: &str,
    ) -> Result<RecipeImport, StoreError> {
        let _ = (import_id, recipe, issues, updated_at);
        Err(StoreError::Unavailable(
            "recipe import draft updates are unsupported".to_owned(),
        ))
    }

    fn commit_recipe_import(
        &self,
        import_id: &str,
        recipe: Recipe,
        issues: Vec<ImportIssue>,
        updated_at: &str,
    ) -> Result<Recipe, StoreError> {
        let _ = (import_id, recipe, issues, updated_at);
        Err(StoreError::Unavailable(
            "recipe import commits are unsupported".to_owned(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CookbookPipelineSource {
    pub import_record: CookbookImport,
    pub cookbook: Cookbook,
    pub pages: Vec<CookbookPage>,
    pub sections: Vec<CookbookSection>,
    pub content_blocks: Vec<CookbookContentBlock>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CookbookPipelineResult {
    pub import_record: CookbookImport,
    pub pages: Vec<CookbookPage>,
    pub sections: Vec<CookbookSection>,
    pub content_blocks: Vec<CookbookContentBlock>,
    pub recipes: Vec<Recipe>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CookbookExtractionPersistResult {
    pub recipe_count: usize,
    pub skipped_recipe_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CookbookPageImage {
    pub image_path: String,
    pub image_hash: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoreMetadata {
    pub duckdb_version: String,
    pub table_count: u64,
    pub mode: StoreMode,
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("store backend failed during {operation}: {detail}")]
    Backend {
        operation: &'static str,
        detail: String,
    },
    #[error("DuckDB returned an invalid table count: {0}")]
    InvalidTableCount(i64),
    #[error("DuckDB readiness query returned {0}, expected 1")]
    UnexpectedPing(i64),
    #[error("DuckDB connection lock is poisoned")]
    LockPoisoned,
    #[error("cookbook page not found")]
    CookbookPageNotFound,
    #[error("cookbook not found")]
    CookbookNotFound,
    #[error("cookbook import not found")]
    CookbookImportNotFound,
    #[error("cookbook import has no pages")]
    CookbookImportHasNoPages,
    #[error("cookbook import progress not found")]
    CookbookImportProgressNotFound,
    #[error("invalid cookbook import")]
    InvalidCookbookImport,
    #[error("duplicate cookbook page image")]
    DuplicateCookbookPageImage,
    #[error("cookbook content block not found")]
    CookbookContentBlockNotFound,
    #[error("cookbook page has no OCR text")]
    CookbookPageHasNoText,
    #[error("cookbook page content was already accepted")]
    CookbookPageAlreadyAccepted,
    #[error("cookbook already exists")]
    CookbookAlreadyExists,
    #[error("invalid cookbook")]
    InvalidCookbook,
    #[error("recipe already exists")]
    RecipeAlreadyExists,
    #[error("recipe not found")]
    RecipeNotFound,
    #[error("recipe import not found")]
    RecipeImportNotFound,
    #[error("recipe import already exists")]
    RecipeImportAlreadyExists,
    #[error("invalid recipe: {0}")]
    InvalidRecipe(#[from] RecipeValidationError),
    #[error("pantry item not found")]
    PantryItemNotFound,
    #[error("numeric value is out of range for {0}")]
    NumericOutOfRange(&'static str),
    #[error("invalid JSON in {context}: {detail}")]
    InvalidJson {
        context: &'static str,
        detail: String,
    },
    #[error("store unavailable: {0}")]
    Unavailable(String),
}
