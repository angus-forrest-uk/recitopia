use std::sync::{Mutex, MutexGuard};

use duckdb::{AccessMode, Config, Connection};

use crate::{
    config::{DatabaseConfig, StoreMode},
    duckdb_read, duckdb_write,
    model::{
        AcceptPageContentInput, Catalogue, CookLogEntry, Cookbook, CookbookContentBlock,
        CookbookContentBlockPatch, CookbookImportProgress, CookbookPage, CookbookPagePatch,
        CookbookPageText, CookbookSourceImport, ImportIssue, MarkMadeInput, MealPlanEntry,
        PantryItem, PantryItemPatch, Recipe, RecipeImport,
    },
    store::{
        CookbookExtractionPersistResult, CookbookPageImage, CookbookPipelineResult,
        CookbookPipelineSource, ReadStore, StoreError, StoreMetadata, StoreProbe, WriteStore,
    },
};

pub struct DuckStore {
    connection: Mutex<Connection>,
    metadata: StoreMetadata,
}

impl DuckStore {
    /// Opens `DuckDB` and records the minimum metadata needed by the shadow API.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the database cannot be configured or opened,
    /// or when its version, schema, or readiness probes fail.
    pub fn open(config: &DatabaseConfig) -> Result<Self, StoreError> {
        let connection = if config.is_memory() {
            Connection::open_in_memory().map_err(|error| backend("open", &error))?
        } else {
            let access_mode = match config.mode {
                StoreMode::ReadOnly => AccessMode::ReadOnly,
                StoreMode::ReadWrite => AccessMode::ReadWrite,
            };
            let flags = Config::default()
                .access_mode(access_mode)
                .and_then(|flags| flags.enable_external_access(false))
                .map_err(|error| backend("configure", &error))?;
            Connection::open_with_flags(&config.path, flags)
                .map_err(|error| backend("open", &error))?
        };

        let duckdb_version = connection
            .version()
            .map_err(|error| backend("version probe", &error))?;
        let table_count_i64 = connection
            .query_row(
                "select count(*) from information_schema.tables where table_schema = 'main'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(|error| backend("schema probe", &error))?;
        let table_count = u64::try_from(table_count_i64)
            .map_err(|_| StoreError::InvalidTableCount(table_count_i64))?;

        let store = Self {
            connection: Mutex::new(connection),
            metadata: StoreMetadata {
                duckdb_version,
                table_count,
                mode: config.mode,
            },
        };
        store.ping()?;
        Ok(store)
    }

    #[must_use]
    pub fn metadata(&self) -> &StoreMetadata {
        &self.metadata
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

impl StoreProbe for DuckStore {
    fn ping(&self) -> Result<(), StoreError> {
        let value = self
            .connection()?
            .query_row("select 1", [], |row| row.get::<_, i64>(0))
            .map_err(|error| backend("readiness probe", &error))?;
        if value != 1 {
            return Err(StoreError::UnexpectedPing(value));
        }
        Ok(())
    }
}

impl ReadStore for DuckStore {
    fn catalogue_summary(&self) -> Result<Catalogue, StoreError> {
        let connection = self.connection()?;
        duckdb_read::catalogue_summary(&connection)
    }

    fn pantry_items(&self) -> Result<Vec<PantryItem>, StoreError> {
        let connection = self.connection()?;
        duckdb_read::pantry_items(&connection)
    }

    fn meal_plan_entries(&self) -> Result<Vec<MealPlanEntry>, StoreError> {
        let connection = self.connection()?;
        duckdb_read::meal_plan_entries(&connection)
    }

    fn cook_log_entries(&self) -> Result<Vec<CookLogEntry>, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cook_log_entries(&connection)
    }

    fn cookbook_page_text(&self, page_id: &str) -> Result<CookbookPageText, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cookbook_page_text(&connection, page_id)
    }

    fn cookbook_page_image(&self, page_id: &str) -> Result<CookbookPageImage, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cookbook_page_image(&connection, page_id)
    }

    fn cookbook_content_blocks(
        &self,
        cookbook_id: &str,
    ) -> Result<Vec<CookbookContentBlock>, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cookbook_content_blocks(&connection, cookbook_id)
    }

    fn cookbook_pipeline_source(
        &self,
        import_id: &str,
    ) -> Result<CookbookPipelineSource, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cookbook_pipeline_source(&connection, import_id)
    }

    fn latest_cookbook_pipeline_source(
        &self,
        cookbook_id: &str,
    ) -> Result<CookbookPipelineSource, StoreError> {
        let connection = self.connection()?;
        duckdb_read::latest_cookbook_pipeline_source(&connection, cookbook_id)
    }

    fn cookbook_import_progress(
        &self,
        import_id: &str,
    ) -> Result<CookbookImportProgress, StoreError> {
        let connection = self.connection()?;
        duckdb_read::cookbook_import_progress(&connection, import_id)
    }

    fn recipe_import(&self, import_id: &str) -> Result<RecipeImport, StoreError> {
        let connection = self.connection()?;
        duckdb_read::recipe_import(&connection, import_id)
    }
}

impl WriteStore for DuckStore {
    fn create_cookbook(&self, cookbook: Cookbook) -> Result<Cookbook, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::create_cookbook(&mut connection, &cookbook)
    }

    fn create_recipe(&self, recipe: Recipe, cache_updated_at: &str) -> Result<Recipe, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::create_recipe(&mut connection, recipe, cache_updated_at)
    }

    fn update_recipe(&self, recipe: Recipe, cache_updated_at: &str) -> Result<Recipe, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::update_recipe(&mut connection, recipe, cache_updated_at)
    }

    fn delete_recipe(&self, id: &str) -> Result<(), StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::delete_recipe(&mut connection, id)
    }

    fn mark_recipe_made(
        &self,
        id: &str,
        made_at: &str,
        details: MarkMadeInput,
        cache_updated_at: &str,
    ) -> Result<Recipe, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::mark_recipe_made(&mut connection, id, made_at, details, cache_updated_at)
    }

    fn add_pantry_item(&self, item: PantryItem) -> Result<PantryItem, StoreError> {
        let connection = self.connection()?;
        duckdb_write::add_pantry_item(&connection, &item)
    }

    fn patch_pantry_item(
        &self,
        id: &str,
        patch: PantryItemPatch,
    ) -> Result<PantryItem, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::patch_pantry_item(&mut connection, id, patch)
    }

    fn delete_pantry_item(&self, id: &str) -> Result<(), StoreError> {
        let connection = self.connection()?;
        duckdb_write::delete_pantry_item(&connection, id)
    }

    fn add_meal_plan_entry(&self, entry: MealPlanEntry) -> Result<MealPlanEntry, StoreError> {
        let connection = self.connection()?;
        duckdb_write::add_meal_plan_entry(&connection, &entry)
    }

    fn delete_meal_plan_entry(&self, id: &str) -> Result<(), StoreError> {
        let connection = self.connection()?;
        duckdb_write::delete_meal_plan_entry(&connection, id)
    }

    fn cookbook_exists(&self, id: &str) -> Result<bool, StoreError> {
        let connection = self.connection()?;
        duckdb_write::cookbook_exists(&connection, id)
    }

    fn cookbook_page_image_hash_exists(&self, image_hash: &str) -> Result<bool, StoreError> {
        let connection = self.connection()?;
        duckdb_write::cookbook_page_image_hash_exists(&connection, image_hash)
    }

    fn create_cookbook_source_import(
        &self,
        source: CookbookSourceImport,
    ) -> Result<(), StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::create_cookbook_source_import(&mut connection, &source)
    }

    fn patch_cookbook_page(
        &self,
        id: &str,
        patch: CookbookPagePatch,
    ) -> Result<CookbookPage, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::patch_cookbook_page(&mut connection, id, patch)
    }

    fn patch_cookbook_content_block(
        &self,
        id: &str,
        patch: CookbookContentBlockPatch,
    ) -> Result<CookbookContentBlock, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::patch_cookbook_content_block(&mut connection, id, patch)
    }

    fn accept_cookbook_page_content(
        &self,
        id: &str,
        input: AcceptPageContentInput,
    ) -> Result<CookbookContentBlock, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::accept_cookbook_page_content(&mut connection, id, input)
    }

    fn upsert_cookbook_import_progress(
        &self,
        progress: CookbookImportProgress,
        updated_at: &str,
    ) -> Result<(), StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::upsert_cookbook_import_progress(&mut connection, &progress, updated_at)
    }

    fn persist_cookbook_pipeline(
        &self,
        result: CookbookPipelineResult,
        cache_updated_at: &str,
    ) -> Result<CookbookExtractionPersistResult, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::persist_cookbook_pipeline(&mut connection, result, cache_updated_at)
    }

    fn create_recipe_import(
        &self,
        recipe_import: RecipeImport,
    ) -> Result<RecipeImport, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::create_recipe_import(&mut connection, &recipe_import)
    }

    fn update_recipe_import_draft(
        &self,
        import_id: &str,
        recipe: Recipe,
        issues: Vec<ImportIssue>,
        updated_at: &str,
    ) -> Result<RecipeImport, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::update_recipe_import_draft(
            &mut connection,
            import_id,
            recipe,
            issues,
            updated_at,
        )
    }

    fn commit_recipe_import(
        &self,
        import_id: &str,
        recipe: Recipe,
        issues: Vec<ImportIssue>,
        updated_at: &str,
    ) -> Result<Recipe, StoreError> {
        let mut connection = self.connection()?;
        duckdb_write::commit_recipe_import(&mut connection, import_id, recipe, issues, updated_at)
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
    fn opens_and_probes_an_in_memory_database() {
        let store = DuckStore::open(&DatabaseConfig {
            path: ":memory:".into(),
            mode: StoreMode::ReadWrite,
        })
        .expect("open in-memory database");

        store.ping().expect("database ping");
        assert!(!store.metadata().duckdb_version.is_empty());
        assert_eq!(store.metadata().table_count, 0);
    }

    #[test]
    fn opens_a_file_database_without_write_access() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("fixture.duckdb");
        {
            let connection = Connection::open(&path).expect("create fixture database");
            connection
                .execute_batch("create table recipes(id varchar primary key)")
                .expect("create fixture table");
        }

        let store = DuckStore::open(&DatabaseConfig {
            path,
            mode: StoreMode::ReadOnly,
        })
        .expect("open fixture read-only");

        assert_eq!(store.metadata().table_count, 1);
        let write_result = store
            .connection()
            .expect("database lock")
            .execute_batch("create table should_not_exist(id integer)");
        assert!(
            write_result.is_err(),
            "read-only shadow store accepted a write"
        );
    }

    #[test]
    fn read_only_mode_does_not_create_a_missing_database() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("missing.duckdb");

        let result = DuckStore::open(&DatabaseConfig {
            path: path.clone(),
            mode: StoreMode::ReadOnly,
        });

        assert!(result.is_err());
        assert!(!path.exists());
    }
}
