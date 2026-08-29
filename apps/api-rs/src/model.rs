use serde::{Deserialize, Serialize, Serializer};
use thiserror::Error;

use crate::zig_compat::wyhash;

#[allow(clippy::ref_option)] // Serde's serialize_with callback borrows the field's exact type.
fn serialize_optional_f64_like_zig<S>(value: &Option<f64>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match value {
        None => serializer.serialize_none(),
        Some(value) if value.is_finite() && value.fract() == 0.0 => {
            match value.to_string().parse::<i64>() {
                Ok(integer) => serializer.serialize_some(&integer),
                Err(_) => serializer.serialize_some(value),
            }
        }
        Some(value) => serializer.serialize_some(value),
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HealthResponse {
    pub ok: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HealthUnavailableResponse {
    pub ok: bool,
    pub error: &'static str,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ErrorResponse {
    pub error: &'static str,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct OkResponse {
    pub ok: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Author {
    pub id: String,
    pub name: String,
    pub website: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Family {
    pub id: String,
    pub name: String,
    pub pantry_shared: bool,
    pub meal_plan_shared: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub family_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ShareScope {
    #[default]
    Personal,
    Family,
    Users,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cookbook {
    pub id: String,
    pub title: String,
    pub author_ids: Vec<String>,
    pub isbn: Option<String>,
    pub publisher: Option<String>,
    pub published_year: Option<u16>,
    pub cover_image_url: Option<String>,
    pub owner_user_id: Option<String>,
    pub family_id: Option<String>,
    #[serde(default)]
    pub share_scope: ShareScope,
    #[serde(default)]
    pub shared_with_user_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeImage {
    pub id: String,
    pub url: String,
    pub alt: String,
    pub credit: Option<String>,
    #[serde(default)]
    pub is_primary: bool,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngredientQuantityKind {
    #[default]
    Exact,
    Range,
    AsNeeded,
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IngredientQuantityReviewStatus {
    #[default]
    Parsed,
    NeedsReview,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingredient {
    pub id: String,
    pub position: Option<u32>,
    pub display_name: String,
    pub item: String,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub quantity: Option<f64>,
    pub quantity_text: Option<String>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub quantity_min: Option<f64>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub quantity_max: Option<f64>,
    #[serde(default)]
    pub quantity_kind: IngredientQuantityKind,
    #[serde(default)]
    pub quantity_review_status: IngredientQuantityReviewStatus,
    pub quantity_review_reason: Option<String>,
    pub unit: Option<String>,
    pub preparation: Option<String>,
    pub section: Option<String>,
    #[serde(default)]
    pub optional: bool,
    pub alternative_text: Option<String>,
    pub source_line: Option<u32>,
    pub source_page_id: Option<String>,
    pub unit_cost_cents: Option<u64>,
    pub estimated_cost_cents: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionStep {
    pub id: String,
    pub position: u32,
    pub section: Option<String>,
    pub text: String,
    pub source_page_id: Option<String>,
    pub source_line_start: Option<u32>,
    pub source_line_end: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeNote {
    pub id: String,
    pub text: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecipeAlternateName {
    pub kind: String,
    pub value: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeSourcePageSpan {
    pub page_id: Option<String>,
    pub printed_page_number: Option<u32>,
    pub line_start: Option<u32>,
    pub line_end: Option<u32>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub confidence: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeExtractionStatus {
    Draft,
    NeedsReview,
    #[default]
    Verified,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    #[serde(default)]
    pub alternate_names: Vec<RecipeAlternateName>,
    pub cookbook_id: String,
    pub author_ids: Vec<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub source_label: String,
    pub headnote: Option<String>,
    pub serving_context: Option<String>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub yield_quantity: Option<f64>,
    pub yield_unit: Option<String>,
    pub prep_minutes: Option<u32>,
    pub cook_minutes: Option<u32>,
    pub total_minutes: Option<u32>,
    pub cuisine: Option<String>,
    pub category: Option<String>,
    pub tags: Vec<String>,
    #[serde(default)]
    pub searchable_text: String,
    pub source_block_id: Option<String>,
    #[serde(default)]
    pub source_page_spans: Vec<RecipeSourcePageSpan>,
    #[serde(default)]
    pub component_recipe_ids: Vec<String>,
    pub pictured_page_number: Option<u32>,
    #[serde(default)]
    pub extraction_status: RecipeExtractionStatus,
    pub images: Vec<RecipeImage>,
    pub ingredients: Vec<Ingredient>,
    pub steps: Vec<InstructionStep>,
    pub notes: Vec<RecipeNote>,
    pub last_made_at: Option<String>,
    #[serde(default)]
    pub times_made: u32,
    pub cost_cents: Option<u64>,
    pub cost_per_serving_cents: Option<u64>,
    #[serde(default = "default_recipe_cache_key")]
    pub cache_key: String,
    pub cache_updated_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecipeImportStatus {
    Processing,
    DraftReady,
    Failed,
    Committed,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportIssueSeverity {
    Info,
    #[default]
    Warning,
    Error,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ImportIssue {
    pub field: String,
    pub message: String,
    #[serde(default)]
    pub severity: ImportIssueSeverity,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeImport {
    pub id: String,
    pub status: RecipeImportStatus,
    pub file_name: String,
    pub mime_type: String,
    pub image_path: String,
    pub ocr_engine: String,
    #[serde(default)]
    pub ocr_text: String,
    #[serde(default = "empty_json_object")]
    pub ocr_json: String,
    pub draft: Option<Recipe>,
    #[serde(default)]
    pub validation_issues: Vec<ImportIssue>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageRecipeImportInput {
    pub file_name: String,
    pub mime_type: String,
    pub image_base64: String,
    pub cookbook_id: String,
    #[serde(default)]
    pub author_ids: Vec<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub source_label: Option<String>,
    pub ocr_text_override: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookRecipeDraftInput {
    pub cookbook_id: String,
    pub source_block_id: Option<String>,
    pub page_id: Option<String>,
    #[serde(default)]
    pub page_ids: Vec<String>,
    pub source_label: Option<String>,
}

fn default_recipe_cache_key() -> String {
    "uncached".to_owned()
}

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum RecipeValidationError {
    #[error("recipe has no ingredients")]
    MissingIngredient,
    #[error("recipe has no instruction steps")]
    MissingStep,
    #[error("recipe yield must be finite and greater than zero")]
    InvalidYield,
    #[error("recipe page range is reversed")]
    InvalidPageRange,
}

impl Recipe {
    /// Validates the same recipe invariants enforced by the Zig model.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeValidationError`] when ingredients or steps are absent,
    /// the yield is invalid, or the page range is reversed.
    pub fn validate(&self) -> Result<(), RecipeValidationError> {
        if self.ingredients.is_empty() {
            return Err(RecipeValidationError::MissingIngredient);
        }
        if self.steps.is_empty() {
            return Err(RecipeValidationError::MissingStep);
        }
        if self
            .yield_quantity
            .is_some_and(|quantity| !quantity.is_finite() || quantity <= 0.0)
        {
            return Err(RecipeValidationError::InvalidYield);
        }
        if self
            .page_start
            .zip(self.page_end)
            .is_some_and(|(start, end)| end < start)
        {
            return Err(RecipeValidationError::InvalidPageRange);
        }
        Ok(())
    }

    /// Recomputes persisted cost, duration, search, and cache fields.
    ///
    /// # Errors
    ///
    /// Returns [`RecipeValidationError`] when [`Recipe::validate`] fails.
    pub fn recompute(mut self, cache_updated_at: &str) -> Result<Self, RecipeValidationError> {
        self.validate()?;

        let cost_cents = self.ingredients.iter().fold(0_u64, |total, ingredient| {
            total.saturating_add(ingredient.derived_cost_cents())
        });
        self.total_minutes = self.total_minutes.or_else(|| {
            let total = self
                .prep_minutes
                .unwrap_or_default()
                .saturating_add(self.cook_minutes.unwrap_or_default());
            (total > 0).then_some(total)
        });
        self.cost_cents = Some(cost_cents);
        self.cost_per_serving_cents = self
            .yield_quantity
            .and_then(|quantity| u64_as_f64(cost_cents).map(|cost| cost / quantity))
            .and_then(rounded_nonnegative_u64);
        self.searchable_text.clone_from(&self.title);
        self.cache_key = self.derived_cache_key();
        self.cache_updated_at = Some(cache_updated_at.to_owned());
        Ok(self)
    }

    fn derived_cache_key(&self) -> String {
        let input_size = self.id.len()
            + self.title.len()
            + self
                .ingredients
                .iter()
                .map(|ingredient| ingredient.display_name.len())
                .sum::<usize>()
            + self.steps.iter().map(|step| step.text.len()).sum::<usize>();
        let mut input = Vec::with_capacity(input_size);
        input.extend_from_slice(self.id.as_bytes());
        input.extend_from_slice(self.title.as_bytes());
        for ingredient in &self.ingredients {
            input.extend_from_slice(ingredient.display_name.as_bytes());
        }
        for step in &self.steps {
            input.extend_from_slice(step.text.as_bytes());
        }
        let suffix = match wyhash(0, &input) % 4 {
            0 => 'a',
            1 => 'b',
            2 => 'c',
            _ => 'd',
        };
        format!("cache-{suffix}")
    }
}

impl Ingredient {
    fn derived_cost_cents(&self) -> u64 {
        if let Some(cost) = self.estimated_cost_cents {
            return cost;
        }
        self.unit_cost_cents
            .zip(self.quantity)
            .and_then(|(unit_cost, quantity)| u64_as_f64(unit_cost).map(|cost| cost * quantity))
            .and_then(rounded_nonnegative_u64)
            .unwrap_or_default()
    }
}

fn rounded_nonnegative_u64(value: f64) -> Option<u64> {
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    value.round().to_string().parse().ok()
}

fn u64_as_f64(value: u64) -> Option<f64> {
    value.to_string().parse().ok()
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookSourceKind {
    #[default]
    ImageSet,
    Pdf,
    Manual,
    Web,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookImportStatus {
    Uploaded,
    #[default]
    OcrReady,
    Mapped,
    Reviewed,
    Committed,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookImport {
    pub id: String,
    pub cookbook_id: String,
    pub source_kind: CookbookSourceKind,
    pub source_path: String,
    pub status: CookbookImportStatus,
    pub ocr_engine: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub review_notes: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookPageKind {
    Cover,
    Title,
    Contents,
    ChapterOpener,
    Essay,
    Reference,
    Recipe,
    Supplier,
    Index,
    Acknowledgements,
    Blank,
    #[default]
    Unknown,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookPageReviewStatus {
    #[default]
    Pending,
    Accepted,
    NeedsCrop,
    NeedsOcrFix,
    Ignored,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookPage {
    pub id: String,
    pub cookbook_id: String,
    pub import_id: String,
    pub image_index: u32,
    pub printed_page_label: Option<String>,
    pub printed_page_number: Option<u32>,
    pub image_path: String,
    pub image_hash: Option<String>,
    pub ocr_text: String,
    pub ocr_json: String,
    pub has_ocr_text: bool,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub average_confidence: Option<f64>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub minimum_confidence: Option<f64>,
    pub page_kind: CookbookPageKind,
    pub review_status: CookbookPageReviewStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookPageText {
    pub id: String,
    pub ocr_text: String,
    pub ocr_json: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookSectionKind {
    FrontMatter,
    Chapter,
    Essay,
    Reference,
    Recipes,
    BackMatter,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookSection {
    pub id: String,
    pub cookbook_id: String,
    pub parent_section_id: Option<String>,
    pub title: String,
    pub kind: CookbookSectionKind,
    pub position: u32,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CookbookContentBlockKind {
    #[default]
    Paragraph,
    Recipe,
    RecipeHeadnote,
    IngredientGlossaryEntry,
    Menu,
    Supplier,
    IndexEntry,
    Caption,
    Callout,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookContentBlock {
    pub id: String,
    pub cookbook_id: String,
    pub section_id: Option<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub position: u32,
    pub kind: CookbookContentBlockKind,
    pub title: Option<String>,
    pub text: String,
    #[serde(default)]
    pub has_text: bool,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub confidence: Option<f64>,
    pub source_json: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookMenuRecipe {
    pub recipe_id: String,
    pub position: u32,
    pub role: Option<String>,
    pub serving_notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookMenu {
    pub id: String,
    pub cookbook_id: String,
    pub source_block_id: Option<String>,
    pub title: String,
    pub theme: Option<String>,
    pub notes: Option<String>,
    pub recipes: Vec<CookbookMenuRecipe>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookGlossaryEntry {
    pub id: String,
    pub cookbook_id: String,
    pub source_block_id: Option<String>,
    pub title: String,
    pub aliases: Vec<String>,
    pub native_names: Vec<String>,
    pub description: String,
    pub storage_notes: Option<String>,
    pub substitution_notes: Option<String>,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookSupplier {
    pub id: String,
    pub cookbook_id: String,
    pub source_block_id: Option<String>,
    pub name: String,
    pub url: Option<String>,
    pub region: Option<String>,
    pub notes: Option<String>,
    pub source_page: Option<u32>,
    pub review_status: CookbookPageReviewStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookIndexEntry {
    pub id: String,
    pub cookbook_id: String,
    pub term: String,
    pub subterm: Option<String>,
    pub target_page_label: Option<String>,
    pub target_page_number: Option<u32>,
    pub target_recipe_id: Option<String>,
    pub illustration: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookCrossReference {
    pub id: String,
    pub cookbook_id: String,
    pub from_kind: String,
    pub from_id: String,
    pub to_kind: String,
    pub to_id: Option<String>,
    pub label: Option<String>,
    pub relation_kind: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookPageImageUploadInput {
    pub file_name: String,
    pub mime_type: String,
    pub image_base64: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookPageImageUpload {
    pub image_path: String,
    pub image_hash: String,
    pub size_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookImagePageInput {
    pub image_index: u32,
    #[serde(default)]
    pub printed_page_label: Option<String>,
    #[serde(default)]
    pub printed_page_number: Option<u32>,
    pub image_path: String,
    #[serde(default)]
    pub image_hash: Option<String>,
    #[serde(default)]
    pub ocr_text: String,
    #[serde(default = "empty_json_object")]
    pub ocr_json: String,
    #[serde(default)]
    pub average_confidence: Option<f64>,
    #[serde(default)]
    pub minimum_confidence: Option<f64>,
    #[serde(default)]
    pub page_kind: CookbookPageKind,
    #[serde(default)]
    pub review_status: CookbookPageReviewStatus,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookSourceImportInput {
    pub cookbook_id: String,
    pub source_path: String,
    #[serde(default)]
    pub status: CookbookImportStatus,
    #[serde(default)]
    pub ocr_engine: Option<String>,
    #[serde(default)]
    pub review_notes: Option<String>,
    pub pages: Vec<CookbookImagePageInput>,
    #[serde(default)]
    pub sections: Vec<CookbookSection>,
    #[serde(default)]
    pub content_blocks: Vec<CookbookContentBlock>,
    #[serde(default)]
    pub menus: Vec<CookbookMenu>,
    #[serde(default)]
    pub glossary_entries: Vec<CookbookGlossaryEntry>,
    #[serde(default)]
    pub suppliers: Vec<CookbookSupplier>,
    #[serde(default)]
    pub index_entries: Vec<CookbookIndexEntry>,
    #[serde(default)]
    pub cross_references: Vec<CookbookCrossReference>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CookbookSourceImport {
    pub import_record: CookbookImport,
    pub pages: Vec<CookbookPage>,
    pub sections: Vec<CookbookSection>,
    pub content_blocks: Vec<CookbookContentBlock>,
    pub menus: Vec<CookbookMenu>,
    pub glossary_entries: Vec<CookbookGlossaryEntry>,
    pub suppliers: Vec<CookbookSupplier>,
    pub index_entries: Vec<CookbookIndexEntry>,
    pub cross_references: Vec<CookbookCrossReference>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookImageSetImportSummary {
    pub import_record: CookbookImport,
    pub page_count: usize,
    pub section_count: usize,
    pub content_block_count: usize,
    #[serde(default)]
    pub recipe_count: usize,
    pub menu_count: usize,
    pub glossary_entry_count: usize,
    pub supplier_count: usize,
    pub index_entry_count: usize,
    pub cross_reference_count: usize,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportJobState {
    Running,
    Complete,
    Failed,
    Canceled,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportPipelineStage {
    Queued,
    LoadingPages,
    OcrPages,
    SourceMap,
    LlmPlan,
    LlmSection,
    Normalizing,
    Persisting,
    Complete,
    Failed,
    Canceled,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookImportProgress {
    pub import_id: String,
    pub state: ImportJobState,
    pub stage: ImportPipelineStage,
    pub message: String,
    pub current: Option<usize>,
    pub total: Option<usize>,
    #[serde(default)]
    pub processed_count: usize,
    #[serde(default)]
    pub skipped_count: usize,
    #[serde(default)]
    pub failed_count: usize,
    #[serde(default)]
    pub section_count: usize,
    #[serde(default)]
    pub content_block_count: usize,
    #[serde(default)]
    pub recipe_count: usize,
    pub current_section_index: Option<usize>,
    pub section_total: Option<usize>,
    pub current_section_title: Option<String>,
    pub extraction_engine: Option<String>,
    #[serde(rename = "error")]
    pub error_message: Option<String>,
}

impl CookbookImportProgress {
    #[must_use]
    pub fn queued(import_id: impl Into<String>) -> Self {
        Self {
            import_id: import_id.into(),
            state: ImportJobState::Running,
            stage: ImportPipelineStage::Queued,
            message: "Queued cookbook import processing.".to_owned(),
            current: None,
            total: None,
            processed_count: 0,
            skipped_count: 0,
            failed_count: 0,
            section_count: 0,
            content_block_count: 0,
            recipe_count: 0,
            current_section_index: None,
            section_total: None,
            current_section_title: None,
            extraction_engine: None,
            error_message: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroductionPageDiagnosticArtifacts {
    pub work_dir: String,
    pub ocr_text_path: String,
    pub ocr_output_path: String,
    pub source_map_input_path: String,
    pub source_map_output_path: String,
    pub llm_input_path: String,
    pub llm_output_path: String,
    pub llm_verbose_dir: String,
    pub result_path: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntroductionPageDiagnostic {
    pub job_id: String,
    pub cookbook_id: String,
    pub page_id: String,
    pub selected_by: String,
    pub image_index: u32,
    pub stored_printed_page_number: Option<u32>,
    pub detected_printed_page_number: Option<u32>,
    pub ocr_engine: String,
    pub ocr_layout_mode: Option<String>,
    pub ocr_column_detection: Option<String>,
    pub extraction_engine: String,
    pub source_map_section_count: usize,
    pub source_map_content_block_count: usize,
    pub extracted_recipe_count: usize,
    pub extracted_content_block_count: usize,
    pub checks_passed: bool,
    pub issues: Vec<String>,
    pub expected_ocr_order: Vec<String>,
    pub ocr_text_preview: String,
    pub extracted_block_preview: String,
    pub artifacts: IntroductionPageDiagnosticArtifacts,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookPagePatch {
    pub page_kind: Option<CookbookPageKind>,
    pub review_status: Option<CookbookPageReviewStatus>,
    pub ocr_text: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookbookContentBlockPatch {
    pub text: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcceptPageContentInput {
    pub kind: Option<CookbookContentBlockKind>,
    pub title: Option<String>,
}

fn empty_json_object() -> String {
    "{}".to_owned()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalogue {
    pub current_user_id: Option<String>,
    pub families: Vec<Family>,
    pub users: Vec<User>,
    pub authors: Vec<Author>,
    pub cookbooks: Vec<Cookbook>,
    pub recipes: Vec<Recipe>,
    pub cookbook_imports: Vec<CookbookImport>,
    pub cookbook_pages: Vec<CookbookPage>,
    pub cookbook_sections: Vec<CookbookSection>,
    pub cookbook_content_blocks: Vec<CookbookContentBlock>,
    pub cookbook_menus: Vec<CookbookMenu>,
    pub cookbook_glossary_entries: Vec<CookbookGlossaryEntry>,
    pub cookbook_suppliers: Vec<CookbookSupplier>,
    pub cookbook_index_entries: Vec<CookbookIndexEntry>,
    pub cookbook_cross_references: Vec<CookbookCrossReference>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PantryCategory {
    #[default]
    Raw,
    Prepared,
    Leftover,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PantryItem {
    pub id: String,
    pub item: String,
    pub display_name: String,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    pub category: PantryCategory,
    pub source_recipe_id: Option<String>,
    pub notes: Option<String>,
    pub expires_at: Option<String>,
    pub added_at: String,
    pub owner_user_id: Option<String>,
    pub family_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PantryItemInput {
    pub item: String,
    pub display_name: String,
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    #[serde(default)]
    pub category: PantryCategory,
    pub source_recipe_id: Option<String>,
    pub notes: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PantryItemPatch {
    pub quantity: Option<f64>,
    pub unit: Option<String>,
    pub category: Option<PantryCategory>,
    pub notes: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MealType {
    Breakfast,
    Lunch,
    Dinner,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MealPlanEntry {
    pub id: String,
    pub date: String,
    pub meal_type: MealType,
    pub recipe_id: String,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub servings: Option<f64>,
    pub notes: Option<String>,
    pub owner_user_id: Option<String>,
    pub family_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MealPlanEntryInput {
    pub date: String,
    pub meal_type: MealType,
    pub recipe_id: String,
    pub servings: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Substitution {
    pub id: String,
    pub ingredient_id: String,
    pub original_item: String,
    pub substitute_text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SubstitutionInput {
    pub ingredient_id: String,
    #[serde(default)]
    pub original_item: String,
    pub substitute_text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookLogEntry {
    pub id: String,
    pub recipe_id: String,
    pub made_at: String,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub servings_made: Option<f64>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub servings_eaten: Option<f64>,
    #[serde(serialize_with = "serialize_optional_f64_like_zig")]
    pub leftover_servings: Option<f64>,
    pub notes: Option<String>,
    #[serde(default)]
    pub substitutions: Vec<Substitution>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MarkMadeInput {
    pub made_at: Option<String>,
    pub servings_made: Option<f64>,
    pub servings_eaten: Option<f64>,
    pub leftover_servings: Option<f64>,
    pub notes: Option<String>,
    #[serde(default)]
    pub substitutions: Vec<SubstitutionInput>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct OptionalNumber {
        #[serde(serialize_with = "serialize_optional_f64_like_zig")]
        value: Option<f64>,
    }

    #[test]
    fn optional_floats_match_zig_json_number_formatting() {
        assert_eq!(
            serde_json::to_value(OptionalNumber { value: Some(3.0) }).unwrap(),
            serde_json::json!({ "value": 3 })
        );
        assert_eq!(
            serde_json::to_value(OptionalNumber { value: Some(1.4) }).unwrap(),
            serde_json::json!({ "value": 1.4 })
        );
        assert_eq!(
            serde_json::to_value(OptionalNumber { value: None }).unwrap(),
            serde_json::json!({ "value": null })
        );
    }

    #[test]
    fn recipe_defaults_and_recomputation_match_the_zig_model() {
        let recipe = serde_json::from_value::<Recipe>(serde_json::json!({
            "id": "test-recipe",
            "title": "Test Recipe",
            "cookbookId": "test-book",
            "authorIds": ["author-1"],
            "sourceLabel": "Test Book, p. 1",
            "tags": [],
            "images": [],
            "ingredients": [{
                "id": "ingredient-1",
                "displayName": "2 test ingredients",
                "item": "test ingredient",
                "quantity": 2,
                "unitCostCents": 25
            }],
            "steps": [{"id": "step-1", "position": 1, "text": "Cook it."}],
            "notes": []
        }))
        .expect("recipe defaults");

        assert_eq!(recipe.extraction_status, RecipeExtractionStatus::Verified);
        assert_eq!(recipe.cache_key, "uncached");
        assert!(!recipe.ingredients[0].optional);
        let recomputed = recipe
            .recompute("2026-07-10T08:00:00.000Z")
            .expect("valid recipe");
        assert_eq!(recomputed.cost_cents, Some(50));
        assert_eq!(recomputed.searchable_text, "Test Recipe");
        assert!(recomputed.cache_key.starts_with("cache-"));
    }
}
