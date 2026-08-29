use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    fs,
    io::AsyncReadExt,
    process::Command,
    sync::Semaphore,
    time::{MissedTickBehavior, interval},
};

use crate::{
    config::PipelineConfig,
    jobs::CancellationSignal,
    model::{
        Cookbook, CookbookContentBlock, CookbookContentBlockKind, CookbookImport,
        CookbookImportStatus, CookbookPage, CookbookPageKind, CookbookPageReviewStatus,
        CookbookSection, CookbookSectionKind, CookbookSourceKind, ImportIssue, ImportIssueSeverity,
        ImportJobState, ImportPipelineStage, Ingredient, InstructionStep,
        IntroductionPageDiagnostic, IntroductionPageDiagnosticArtifacts, Recipe,
        RecipeExtractionStatus, RecipeImport, RecipeImportStatus, RecipeSourcePageSpan,
    },
    runtime::now_iso8601,
    store::{CookbookPipelineResult, CookbookPipelineSource},
};

const MAX_MAPPER_STDOUT_BYTES: u64 = 32 << 20;
const MAX_MAPPER_STDERR_BYTES: u64 = 128 << 20;
const INTRODUCTION_EXPECTED_ORDER: &[&str] = &[
    "Our Korean kitchen is an unusual one",
    "While I am still passionate",
    "Years ago, when Jina first introduced",
    "food stalls, corner stores",
    "Good food and cooking is so ingrained",
    "Back in London, with a fridge full",
    "Fortunately, in the years",
];

pub type ProgressReporter = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;

#[derive(Clone, Debug, Default)]
pub struct ProgressUpdate {
    pub state: Option<ImportJobState>,
    pub stage: Option<ImportPipelineStage>,
    pub message: Option<String>,
    pub current: Option<usize>,
    pub total: Option<usize>,
    pub processed_count: Option<usize>,
    pub skipped_count: Option<usize>,
    pub failed_count: Option<usize>,
    pub section_count: Option<usize>,
    pub content_block_count: Option<usize>,
    pub recipe_count: Option<usize>,
    pub current_section_index: Option<usize>,
    pub section_total: Option<usize>,
    pub current_section_title: Option<String>,
    pub clear_current_section_title: bool,
    pub extraction_engine: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct PipelineRun {
    pub persistence: CookbookPipelineResult,
    pub processed_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub ocr_engine: Option<String>,
    pub extraction_engine: String,
}

#[derive(Clone, Debug)]
pub struct RecipeDraftSource {
    pub import_id: String,
    pub file_name: String,
    pub mime_type: String,
    pub image_path: String,
    pub ocr_text: String,
    pub ocr_json: String,
    pub ocr_engine: String,
    pub cookbook: Cookbook,
    pub page_start: Option<u32>,
    pub page_end: Option<u32>,
    pub source_label: String,
    pub source_block_id: Option<String>,
    pub source_page_spans: Vec<RecipeSourcePageSpan>,
    pub timestamp: String,
}

#[derive(Clone, Debug)]
pub struct PipelineService {
    config: PipelineConfig,
    client: Client,
    permits: Arc<Semaphore>,
}

impl PipelineService {
    /// Creates the worker orchestrator used by detached API jobs.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if the HTTP client cannot be configured.
    pub fn new(config: PipelineConfig) -> Result<Self, PipelineError> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(2 * 60 * 60))
            .build()
            .map_err(PipelineError::Http)?;
        Ok(Self {
            permits: Arc::new(Semaphore::new(config.concurrency)),
            config,
            client,
        })
    }

    /// Writes the cooperative cancel markers consumed by OCR/DeepSeek workers.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] if a marker directory or file cannot be written.
    pub async fn request_cancel(&self, job_id: &str) -> Result<(), PipelineError> {
        let root = import_root();
        for path in [
            root.join("cookbook-ocr").join(job_id).join("cancel"),
            root.join("cookbook-extraction").join(job_id).join("cancel"),
            root.join("diagnostics").join(job_id).join("cancel"),
        ] {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).await?;
            }
            fs::write(path, b"cancel\n").await?;
        }
        Ok(())
    }

    /// Reloads a persisted introduction diagnostic after an API restart.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] when the id is unsafe or the artifact cannot be read.
    pub async fn load_introduction_diagnostic(
        &self,
        job_id: &str,
    ) -> Result<IntroductionPageDiagnostic, PipelineError> {
        if !safe_job_id(job_id) {
            return Err(PipelineError::DiagnosticPageNotFound);
        }
        let path = import_root()
            .join("diagnostics")
            .join(job_id)
            .join("introduction-page")
            .join("06-diagnostic-result.json");
        let bytes = fs::read(path).await?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Runs OCR for a single recipe image, using the persistent server first.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] when both the server and subprocess fallback fail.
    pub async fn ocr_recipe_image(
        &self,
        id: &str,
        image_path: &str,
        cancellation: &CancellationSignal,
    ) -> Result<(String, String, String), PipelineError> {
        let page = OcrPageRequest {
            id: id.to_owned(),
            image_path: image_path.to_owned(),
        };
        let result = match self
            .ocr_batch(std::slice::from_ref(&page), cancellation)
            .await
        {
            Ok(mut results) => results.pop().ok_or(PipelineError::OcrProducedNoText)?.1?,
            Err(error) => {
                tracing::warn!(
                    event = "ocr_server_recipe_unavailable",
                    error = %error,
                    fallback = "subprocess"
                );
                self.ocr_subprocess(&page, cancellation).await?
            }
        };
        if result.text.trim().is_empty() {
            return Err(PipelineError::OcrProducedNoText);
        }
        Ok((result.engine, result.text, result.raw_json))
    }

    /// Converts curated OCR text into an editable recipe import draft.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] when `DeepSeek` is unavailable or returns invalid JSON.
    pub async fn create_recipe_draft(
        &self,
        source: RecipeDraftSource,
        cancellation: &CancellationSignal,
    ) -> Result<RecipeImport, PipelineError> {
        if !crate::config::llm_configured() {
            return Err(PipelineError::DeepSeekNotConfigured);
        }
        ensure_not_canceled(cancellation)?;
        let curated_source_block_id = source.source_block_id.clone();
        let work_dir = import_root().join("recipe-imports").join(&source.import_id);
        fs::create_dir_all(&work_dir).await?;
        let verbose_dir = work_dir.join("verbose");
        fs::create_dir_all(&verbose_dir).await?;
        let request_path = work_dir.join("mapper-request.json");
        let output_path = work_dir.join("mapper-output.json");
        let cancel_path = work_dir.join("cancel");
        remove_if_exists(&cancel_path).await?;
        let request = json!({
            "ocrText": source.ocr_text,
            "cookbookId": source.cookbook.id,
            "authorIds": source.cookbook.author_ids,
            "pageStart": source.page_start,
            "pageEnd": source.page_end,
            "sourceLabel": source.source_label,
            "sourceBlockId": source.source_block_id,
            "sourcePageSpans": source.source_page_spans,
        });
        fs::write(&request_path, serde_json::to_vec(&request)?).await?;
        let env = [
            ("RECITOPIA_CANCEL_FILE", cancel_path.as_os_str()),
            ("RECITOPIA_VERBOSE_LOG_DIR", verbose_dir.as_os_str()),
        ];
        let output = run_command(
            &self.config.deepseek_python,
            &[
                self.config.deepseek_recipe_script.as_os_str(),
                request_path.as_os_str(),
            ],
            Some(&env),
            cancellation,
            None,
        )
        .await?;
        for (index, line) in output.stderr.lines().enumerate() {
            tracing::info!(
                event = "deepseek_recipe_mapper_subprocess_log",
                level = "VERBOSE",
                context = source.source_label,
                line = index + 1,
                text = line
            );
        }
        if !output.success {
            return Err(PipelineError::WorkerFailed {
                worker: "deepseek recipe mapper",
                detail: output.stderr,
            });
        }
        fs::write(output_path, &output.stdout).await?;
        let recipe: Recipe = serde_json::from_slice(&output.stdout)?;
        let mut recipes = vec![recipe];
        let synthetic_import = CookbookImport {
            id: source.import_id.clone(),
            cookbook_id: source.cookbook.id.clone(),
            source_kind: crate::model::CookbookSourceKind::Manual,
            source_path: source.image_path.clone(),
            status: CookbookImportStatus::OcrReady,
            ocr_engine: Some(source.ocr_engine.clone()),
            created_at: source.timestamp.clone(),
            updated_at: source.timestamp.clone(),
            review_notes: None,
        };
        normalize_recipes(&synthetic_import, &source.cookbook, &mut recipes);
        let mut draft = recipes.pop().ok_or(PipelineError::MapperReturnedNoRecipe)?;
        draft.source_block_id = curated_source_block_id;
        let validation_issues = validate_draft(&draft);
        Ok(RecipeImport {
            id: source.import_id,
            status: RecipeImportStatus::DraftReady,
            file_name: source.file_name,
            mime_type: source.mime_type,
            image_path: source.image_path,
            ocr_engine: source.ocr_engine,
            ocr_text: source.ocr_text,
            ocr_json: source.ocr_json,
            draft: Some(draft),
            validation_issues,
            created_at: source.timestamp.clone(),
            updated_at: source.timestamp,
        })
    }

    /// Runs OCR, deterministic source mapping, and `DeepSeek` extraction without
    /// owning persistence. The caller commits the returned graph atomically.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] for worker, filesystem, JSON, or cancellation failures.
    pub async fn run_cookbook(
        &self,
        mut source: CookbookPipelineSource,
        refresh_ocr: bool,
        cancellation: &CancellationSignal,
        report: ProgressReporter,
    ) -> Result<PipelineRun, PipelineError> {
        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::Queued),
            message: Some("Waiting for a pipeline worker.".to_owned()),
            ..ProgressUpdate::default()
        });
        let _permit = tokio::select! {
            permit = self.permits.clone().acquire_owned() => permit.map_err(|_| PipelineError::Unavailable)?,
            () = cancellation.cancelled() => return Err(PipelineError::Canceled),
        };
        ensure_not_canceled(cancellation)?;
        source.pages.sort_by_key(|page| page.image_index);
        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::LoadingPages),
            message: Some("Loading cookbook pages.".to_owned()),
            total: Some(source.pages.len()),
            ..ProgressUpdate::default()
        });

        let (processed_count, skipped_count, failed_count, ocr_engine) = self
            .run_page_ocr(
                &mut source.pages,
                refresh_ocr,
                cancellation,
                Arc::clone(&report),
            )
            .await?;
        ensure_not_canceled(cancellation)?;

        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::SourceMap),
            message: Some("Building cookbook sections and source map.".to_owned()),
            current: Some(source.pages.len()),
            total: Some(source.pages.len()),
            processed_count: Some(processed_count),
            skipped_count: Some(skipped_count),
            failed_count: Some(failed_count),
            ..ProgressUpdate::default()
        });
        let (sections, source_blocks) = build_source_map(&source.import_record, &source.pages);

        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::DeepseekPlan),
            message: Some("Planning recipe and context extraction.".to_owned()),
            section_count: Some(sections.len()),
            content_block_count: Some(source_blocks.len()),
            current_section_index: Some(0),
            section_total: Some(sections.len()),
            ..ProgressUpdate::default()
        });
        ensure_not_canceled(cancellation)?;
        let extraction = self
            .extract_cookbook(
                &source.import_record,
                &source.cookbook,
                &source.pages,
                &sections,
                &source_blocks,
                cancellation,
                Arc::clone(&report),
            )
            .await?;

        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::Normalizing),
            message: Some("Normalizing extracted recipes and context.".to_owned()),
            section_count: Some(sections.len()),
            content_block_count: Some(extraction.content_blocks.len()),
            recipe_count: Some(extraction.recipes.len()),
            current_section_index: Some(sections.len()),
            section_total: Some(sections.len()),
            clear_current_section_title: true,
            extraction_engine: Some(extraction.engine.clone()),
            ..ProgressUpdate::default()
        });
        ensure_not_canceled(cancellation)?;

        let now = now_iso8601();
        let mut import_record = source.import_record;
        import_record.updated_at = now;
        import_record.ocr_engine.clone_from(&ocr_engine);
        import_record.review_notes = Some(format!(
            "OCR process: {processed_count} processed, {skipped_count} skipped, {failed_count} failed. Source map: {} sections, {} context blocks. Extraction: {} recipes via {}.",
            sections.len(),
            extraction.content_blocks.len(),
            extraction.recipes.len(),
            extraction.engine
        ));
        import_record.status = if extraction.recipes.is_empty() {
            if sections.is_empty() && extraction.content_blocks.is_empty() {
                CookbookImportStatus::OcrReady
            } else {
                CookbookImportStatus::Mapped
            }
        } else {
            CookbookImportStatus::Committed
        };

        Ok(PipelineRun {
            persistence: CookbookPipelineResult {
                import_record,
                pages: source.pages,
                sections,
                content_blocks: extraction.content_blocks,
                recipes: extraction.recipes,
            },
            processed_count,
            skipped_count,
            failed_count,
            ocr_engine,
            extraction_engine: extraction.engine,
        })
    }

    /// Runs the real pipeline against a bounded representative page set and
    /// returns the dry-run result without persisting catalogue data.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] for missing pages or worker failures.
    pub async fn run_diagnostic(
        &self,
        mut source: CookbookPipelineSource,
        job_id: &str,
        cancellation: &CancellationSignal,
        report: ProgressReporter,
    ) -> Result<PipelineRun, PipelineError> {
        source.pages.sort_by_key(|page| page.image_index);
        let mut selected = Vec::new();
        for kind in [
            CookbookPageKind::Contents,
            CookbookPageKind::Essay,
            CookbookPageKind::ChapterOpener,
            CookbookPageKind::Recipe,
            CookbookPageKind::Reference,
            CookbookPageKind::Index,
        ] {
            if let Some(page) = source.pages.iter().find(|page| page.page_kind == kind) {
                if !selected
                    .iter()
                    .any(|selected: &CookbookPage| selected.id == page.id)
                {
                    selected.push(page.clone());
                }
            }
        }
        for page in &source.pages {
            if selected.len() >= 6 {
                break;
            }
            if !selected
                .iter()
                .any(|selected: &CookbookPage| selected.id == page.id)
            {
                selected.push(page.clone());
            }
        }
        if selected.is_empty() {
            return Err(PipelineError::DiagnosticPageNotFound);
        }
        selected.sort_by_key(|page| page.image_index);
        for (index, page) in selected.iter_mut().enumerate() {
            page.id = format!("{job_id}-page-{}", page.image_index);
            page.import_id = job_id.to_owned();
            if index < 2 {
                page.ocr_text.clear();
                page.ocr_json = "{}".to_owned();
                page.has_ocr_text = false;
            }
        }
        let timestamp = now_iso8601();
        source.import_record = CookbookImport {
            id: job_id.to_owned(),
            cookbook_id: source.cookbook.id.clone(),
            source_kind: CookbookSourceKind::Manual,
            source_path: "diagnostic://real-cookbook-pages".to_owned(),
            status: CookbookImportStatus::OcrReady,
            ocr_engine: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            review_notes: Some("Pipeline diagnostic dry run.".to_owned()),
        };
        source.pages = selected;
        source.sections.clear();
        source.content_blocks.clear();
        self.run_cookbook(source, false, cancellation, report).await
    }

    /// Runs the page-four introduction diagnostic and writes every boundary
    /// payload to durable files for journal-side investigation.
    ///
    /// # Errors
    ///
    /// Returns [`PipelineError`] for a missing page, worker failure, or artifact I/O error.
    pub async fn run_introduction_diagnostic(
        &self,
        source: CookbookPipelineSource,
        job_id: &str,
        requested_image_index: u32,
        cancellation: &CancellationSignal,
        report: ProgressReporter,
    ) -> Result<IntroductionPageDiagnostic, PipelineError> {
        let target = source
            .pages
            .iter()
            .find(|page| page.image_index == requested_image_index)
            .or_else(|| {
                source.pages.iter().find(|page| {
                    page.ocr_text
                        .to_ascii_lowercase()
                        .contains("our korean kitchen is an unusual one")
                })
            })
            .cloned()
            .ok_or(PipelineError::DiagnosticPageNotFound)?;
        let stored_printed_page_number = target.printed_page_number;
        let page_id = target.id.clone();
        let selected_by = if target.image_index == requested_image_index {
            "image_index"
        } else {
            "ocr_intro_text"
        };
        tracing::info!(
            event = "introduction_page_diagnostic_start",
            job_id,
            cookbook_id = source.cookbook.id,
            page_id,
            image_index = target.image_index,
            selected_by
        );

        let work_dir = import_root()
            .join("diagnostics")
            .join(job_id)
            .join("introduction-page");
        fs::create_dir_all(&work_dir).await?;
        let timestamp = now_iso8601();
        let mut diagnostic_page = target;
        diagnostic_page.id = format!("{job_id}-introduction-page");
        diagnostic_page.import_id = job_id.to_owned();
        diagnostic_page.ocr_text.clear();
        diagnostic_page.ocr_json = "{}".to_owned();
        diagnostic_page.has_ocr_text = false;
        let diagnostic_import = CookbookImport {
            id: job_id.to_owned(),
            cookbook_id: source.cookbook.id.clone(),
            source_kind: CookbookSourceKind::Manual,
            source_path: "diagnostic://introduction-page".to_owned(),
            status: CookbookImportStatus::OcrReady,
            ocr_engine: None,
            created_at: timestamp.clone(),
            updated_at: timestamp,
            review_notes: Some("Introduction page column/order diagnostic dry run.".to_owned()),
        };
        let diagnostic_source = CookbookPipelineSource {
            import_record: diagnostic_import,
            cookbook: source.cookbook.clone(),
            pages: vec![diagnostic_page],
            sections: Vec::new(),
            content_blocks: Vec::new(),
        };
        let run = self
            .run_cookbook(diagnostic_source, true, cancellation, report)
            .await?;
        let page = run
            .persistence
            .pages
            .first()
            .cloned()
            .ok_or(PipelineError::DiagnosticPageNotFound)?;
        let ocr_text_path = work_dir.join("01-paddle-ocr-text.txt");
        let ocr_output_path = work_dir.join("01-paddle-ocr-output.json");
        let source_map_input_path = work_dir.join("02-source-map-input.json");
        let source_map_output_path = work_dir.join("03-source-map-output.json");
        let deepseek_input_path = work_dir.join("04-deepseek-input.json");
        let deepseek_output_path = work_dir.join("05-deepseek-output.json");
        let result_path = work_dir.join("06-diagnostic-result.json");
        fs::write(&ocr_text_path, &page.ocr_text).await?;
        fs::write(&ocr_output_path, &page.ocr_json).await?;
        fs::write(
            &source_map_input_path,
            serde_json::to_vec_pretty(&json!({
                "import": run.persistence.import_record,
                "pages": run.persistence.pages,
                "expectedOcrOrder": INTRODUCTION_EXPECTED_ORDER,
            }))?,
        )
        .await?;
        fs::write(
            &source_map_output_path,
            serde_json::to_vec_pretty(&json!({
                "sections": run.persistence.sections,
                "contentBlocks": run.persistence.content_blocks,
            }))?,
        )
        .await?;
        fs::write(
            &deepseek_input_path,
            serde_json::to_vec_pretty(&mapper_request(
                &run.persistence.import_record,
                &source.cookbook,
                &run.persistence.pages,
                &run.persistence.sections,
                &run.persistence.content_blocks,
            ))?,
        )
        .await?;
        fs::write(
            &deepseek_output_path,
            serde_json::to_vec_pretty(&json!({
                "engine": run.extraction_engine,
                "recipes": run.persistence.recipes,
                "contentBlocks": run.persistence.content_blocks,
            }))?,
        )
        .await?;

        let mut issues = introduction_order_issues(&page.ocr_text);
        if run.persistence.sections.is_empty() {
            issues.push("Source map returned no sections.".to_owned());
        }
        if run.persistence.content_blocks.is_empty() {
            issues.push("DeepSeek returned no introduction context block.".to_owned());
        }
        if !run.persistence.recipes.is_empty() {
            issues.push("Introduction page was incorrectly extracted as a recipe.".to_owned());
        }
        let layout = serde_json::from_str::<Value>(&page.ocr_json).ok();
        let layout_mode = layout
            .as_ref()
            .and_then(|value| value.pointer("/layout/mode"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let column_detection = layout
            .as_ref()
            .and_then(|value| value.pointer("/layout/detection"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let extracted_preview = run
            .persistence
            .content_blocks
            .first()
            .map_or_else(String::new, |block| preview(&block.text, 1_200));
        let verbose_dir = import_root()
            .join("cookbook-extraction")
            .join(job_id)
            .join("verbose");
        let result = IntroductionPageDiagnostic {
            job_id: job_id.to_owned(),
            cookbook_id: source.cookbook.id,
            page_id,
            selected_by: selected_by.to_owned(),
            image_index: page.image_index,
            stored_printed_page_number,
            detected_printed_page_number: page.printed_page_number,
            ocr_engine: run.ocr_engine.unwrap_or_else(|| "unknown".to_owned()),
            ocr_layout_mode: layout_mode,
            ocr_column_detection: column_detection,
            extraction_engine: run.extraction_engine,
            source_map_section_count: run.persistence.sections.len(),
            source_map_content_block_count: run.persistence.content_blocks.len(),
            extracted_recipe_count: run.persistence.recipes.len(),
            extracted_content_block_count: run.persistence.content_blocks.len(),
            checks_passed: issues.is_empty(),
            issues,
            expected_ocr_order: INTRODUCTION_EXPECTED_ORDER
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            ocr_text_preview: preview(&page.ocr_text, 1_800),
            extracted_block_preview: extracted_preview,
            artifacts: IntroductionPageDiagnosticArtifacts {
                work_dir: display_path(&work_dir),
                ocr_text_path: display_path(&ocr_text_path),
                ocr_output_path: display_path(&ocr_output_path),
                source_map_input_path: display_path(&source_map_input_path),
                source_map_output_path: display_path(&source_map_output_path),
                deepseek_input_path: display_path(&deepseek_input_path),
                deepseek_output_path: display_path(&deepseek_output_path),
                deepseek_verbose_dir: display_path(&verbose_dir),
                result_path: display_path(&result_path),
            },
        };
        fs::write(&result_path, serde_json::to_vec_pretty(&result)?).await?;
        tracing::info!(
            event = "introduction_page_diagnostic_complete",
            job_id,
            cookbook_id = result.cookbook_id,
            page_id = result.page_id,
            checks_passed = result.checks_passed,
            issues = result.issues.len(),
            extraction_engine = result.extraction_engine,
            blocks = result.extracted_content_block_count,
            recipes = result.extracted_recipe_count
        );
        Ok(result)
    }

    async fn run_page_ocr(
        &self,
        pages: &mut [CookbookPage],
        refresh_ocr: bool,
        cancellation: &CancellationSignal,
        report: ProgressReporter,
    ) -> Result<(usize, usize, usize, Option<String>), PipelineError> {
        let pending = pages
            .iter()
            .enumerate()
            .filter(|(_, page)| refresh_ocr || page.ocr_text.trim().is_empty())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut processed = 0_usize;
        let skipped = pages.len() - pending.len();
        let mut failed = 0_usize;
        let mut engine = None;
        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::OcrPages),
            message: Some(if pending.is_empty() {
                "OCR already exists for every page.".to_owned()
            } else if refresh_ocr {
                "Refreshing page OCR.".to_owned()
            } else {
                "Running page OCR.".to_owned()
            }),
            current: Some(skipped),
            total: Some(pages.len()),
            processed_count: Some(0),
            skipped_count: Some(skipped),
            failed_count: Some(0),
            ..ProgressUpdate::default()
        });

        for batch in pending.chunks(self.config.ocr_batch_page_limit) {
            ensure_not_canceled(cancellation)?;
            let request_pages = batch
                .iter()
                .map(|index| OcrPageRequest {
                    id: pages[*index].id.clone(),
                    image_path: pages[*index].image_path.clone(),
                })
                .collect::<Vec<_>>();
            let by_id = match self.ocr_batch(&request_pages, cancellation).await {
                Ok(results) => results.into_iter().collect::<HashMap<_, _>>(),
                Err(error) => {
                    tracing::warn!(
                        event = "ocr_server_batch_unavailable",
                        pages = request_pages.len(),
                        error = %error,
                        fallback = "subprocess"
                    );
                    let mut fallback = HashMap::with_capacity(request_pages.len());
                    for page in &request_pages {
                        fallback.insert(
                            page.id.clone(),
                            self.ocr_subprocess(page, cancellation).await,
                        );
                    }
                    fallback
                }
            };
            for index in batch {
                let page = &mut pages[*index];
                match by_id.get(&page.id) {
                    Some(Ok(result)) => {
                        page.ocr_text.clone_from(&result.text);
                        page.ocr_json.clone_from(&result.raw_json);
                        page.has_ocr_text = !result.text.trim().is_empty();
                        page.page_kind = classify_page(&result.text);
                        page.review_status = if page.has_ocr_text {
                            CookbookPageReviewStatus::Pending
                        } else {
                            CookbookPageReviewStatus::NeedsOcrFix
                        };
                        if let Some(number) = result.printed_page_number {
                            page.printed_page_number = Some(number);
                            page.printed_page_label = Some(number.to_string());
                        }
                        engine.get_or_insert_with(|| result.engine.clone());
                        processed += 1;
                    }
                    Some(Err(error)) => {
                        failed += 1;
                        page.review_status = CookbookPageReviewStatus::NeedsOcrFix;
                        page.page_kind = CookbookPageKind::Unknown;
                        page.ocr_json = json!({
                            "error": error.to_string(),
                            "imagePath": page.image_path,
                        })
                        .to_string();
                        tracing::error!(
                            event = "cookbook_page_ocr_failed",
                            page_id = page.id,
                            image_path = page.image_path,
                            error = %error
                        );
                    }
                    None => {
                        failed += 1;
                        page.review_status = CookbookPageReviewStatus::NeedsOcrFix;
                        tracing::error!(event = "ocr_server_page_missing", page_id = page.id);
                    }
                }
                report(ProgressUpdate {
                    stage: Some(ImportPipelineStage::OcrPages),
                    message: Some(format!(
                        "Processed OCR for page {} of {}.",
                        processed + skipped + failed,
                        pages.len()
                    )),
                    current: Some(processed + skipped + failed),
                    total: Some(pages.len()),
                    processed_count: Some(processed),
                    skipped_count: Some(skipped),
                    failed_count: Some(failed),
                    ..ProgressUpdate::default()
                });
            }
        }

        validate_printed_page_numbers(pages);
        if processed == 0 && skipped == 0 {
            return Err(PipelineError::OcrProducedNoText);
        }
        Ok((processed, skipped, failed, engine))
    }

    async fn ocr_batch(
        &self,
        pages: &[OcrPageRequest],
        cancellation: &CancellationSignal,
    ) -> Result<Vec<(String, Result<OcrPageResult, PipelineError>)>, PipelineError> {
        let url = format!(
            "{}/ocr/batch",
            self.config.ocr_server_url.trim_end_matches('/')
        );
        let response = tokio::select! {
            response = self.client.post(url).json(&OcrBatchRequest { pages }).send() => response.map_err(PipelineError::Http)?,
            () = cancellation.cancelled() => return Err(PipelineError::Canceled),
        };
        let response = response.error_for_status().map_err(PipelineError::Http)?;
        let payload = response
            .json::<OcrBatchResponse>()
            .await
            .map_err(PipelineError::Http)?;
        Ok(payload
            .results
            .into_iter()
            .map(|result| {
                let id = result.id.clone();
                (id, result.into_result(&payload.engine))
            })
            .collect())
    }

    async fn ocr_subprocess(
        &self,
        page: &OcrPageRequest,
        cancellation: &CancellationSignal,
    ) -> Result<OcrPageResult, PipelineError> {
        let output = run_command(
            &self.config.ocr_python,
            &[self.config.ocr_script.as_os_str(), page.image_path.as_ref()],
            None,
            cancellation,
            None,
        )
        .await?;
        if !output.success {
            return Err(PipelineError::WorkerFailed {
                worker: "ocr",
                detail: output.stderr,
            });
        }
        let payload: Value = serde_json::from_slice(&output.stdout)?;
        Ok(OcrPageResult {
            engine: payload
                .get("engine")
                .and_then(Value::as_str)
                .unwrap_or("paddleocr")
                .to_owned(),
            text: payload
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            printed_page_number: payload
                .get("printedPageNumber")
                .and_then(Value::as_u64)
                .and_then(|number| u32::try_from(number).ok()),
            raw_json: payload.to_string(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn extract_cookbook(
        &self,
        import_record: &CookbookImport,
        cookbook: &Cookbook,
        pages: &[CookbookPage],
        sections: &[CookbookSection],
        source_blocks: &[CookbookContentBlock],
        cancellation: &CancellationSignal,
        report: ProgressReporter,
    ) -> Result<Extraction, PipelineError> {
        if !crate::config::llm_configured() {
            tracing::warn!(
                event = "deepseek_cookbook_extraction_unavailable",
                import_id = import_record.id,
                fallback = "context_only",
                error = "DeepSeekNotConfigured"
            );
            return Ok(Extraction {
                recipes: Vec::new(),
                content_blocks: context_only_blocks(source_blocks),
                engine: "context-only".to_owned(),
            });
        }

        let work_dir = import_root()
            .join("cookbook-extraction")
            .join(&import_record.id);
        fs::create_dir_all(&work_dir).await?;
        let verbose_dir = work_dir.join("verbose");
        fs::create_dir_all(&verbose_dir).await?;
        let request_path = work_dir.join("cookbook-mapper-request.json");
        let progress_path = work_dir.join("progress.jsonl");
        let cancel_path = work_dir.join("cancel");
        remove_if_exists(&progress_path).await?;
        remove_if_exists(&cancel_path).await?;

        let request = mapper_request(import_record, cookbook, pages, sections, source_blocks);
        let request_bytes = serde_json::to_vec(&request)?;
        fs::write(&request_path, &request_bytes).await?;
        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::DeepseekSection),
            message: Some("Extracting cookbook sections with DeepSeek.".to_owned()),
            current_section_index: Some(0),
            section_total: Some(sections.len().max(1)),
            ..ProgressUpdate::default()
        });
        tracing::info!(
            event = "deepseek_cookbook_mapper_start",
            import_id = import_record.id,
            cookbook_id = import_record.cookbook_id,
            sections = sections.len(),
            pages = pages.len(),
            request_bytes = request_bytes.len()
        );

        let env = [
            ("RECITOPIA_CANCEL_FILE", cancel_path.as_os_str()),
            ("RECITOPIA_PROGRESS_FILE", progress_path.as_os_str()),
            ("RECITOPIA_VERBOSE_LOG_DIR", verbose_dir.as_os_str()),
        ];
        let output = run_command(
            &self.config.deepseek_python,
            &[
                self.config.deepseek_cookbook_script.as_os_str(),
                request_path.as_os_str(),
            ],
            Some(&env),
            cancellation,
            Some((&progress_path, Arc::clone(&report))),
        )
        .await?;
        for (index, line) in output.stderr.lines().enumerate() {
            tracing::info!(
                event = "deepseek_cookbook_mapper_subprocess_log",
                level = "VERBOSE",
                context = import_record.id,
                line = index + 1,
                text = line
            );
        }
        if !output.success {
            return Err(PipelineError::WorkerFailed {
                worker: "deepseek cookbook mapper",
                detail: output.stderr,
            });
        }
        let output_path = work_dir.join("cookbook-mapper-output.json");
        fs::write(&output_path, &output.stdout).await?;
        let mut parsed: DeepSeekOutput = serde_json::from_slice(&output.stdout)?;
        normalize_recipes(import_record, cookbook, &mut parsed.recipes);
        normalize_content_blocks(import_record, &mut parsed.content_blocks);
        if parsed.content_blocks.is_empty() {
            parsed.content_blocks = context_only_blocks(source_blocks);
        }
        tracing::info!(
            event = "deepseek_cookbook_mapper_complete",
            import_id = import_record.id,
            stdout_bytes = output.stdout.len(),
            stderr_bytes = output.stderr.len(),
            recipes = parsed.recipes.len(),
            content_blocks = parsed.content_blocks.len()
        );
        Ok(Extraction {
            recipes: parsed.recipes,
            content_blocks: parsed.content_blocks,
            engine: "deepseek".to_owned(),
        })
    }
}

#[derive(Debug)]
struct Extraction {
    recipes: Vec<Recipe>,
    content_blocks: Vec<CookbookContentBlock>,
    engine: String,
}

#[derive(Clone, Debug)]
struct OcrPageResult {
    engine: String,
    text: String,
    printed_page_number: Option<u32>,
    raw_json: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OcrPageRequest {
    id: String,
    image_path: String,
}

#[derive(Serialize)]
struct OcrBatchRequest<'a> {
    pages: &'a [OcrPageRequest],
}

#[derive(Deserialize)]
struct OcrBatchResponse {
    engine: String,
    results: Vec<OcrBatchItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OcrBatchItem {
    id: String,
    #[serde(default)]
    ok: bool,
    engine: Option<String>,
    #[serde(default)]
    text: String,
    printed_page_number: Option<u32>,
    raw: Option<Value>,
    error: Option<String>,
}

impl OcrBatchItem {
    fn into_result(self, default_engine: &str) -> Result<OcrPageResult, PipelineError> {
        if !self.ok {
            return Err(PipelineError::WorkerFailed {
                worker: "ocr page",
                detail: self.error.unwrap_or_else(|| "unknown OCR error".to_owned()),
            });
        }
        let raw_json = self
            .raw
            .as_ref()
            .map_or_else(|| "{}".to_owned(), Value::to_string);
        Ok(OcrPageResult {
            engine: self.engine.unwrap_or_else(|| default_engine.to_owned()),
            text: self.text,
            printed_page_number: self.printed_page_number,
            raw_json,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeepSeekOutput {
    #[serde(default)]
    recipes: Vec<Recipe>,
    #[serde(default)]
    content_blocks: Vec<CookbookContentBlock>,
}

#[derive(Debug)]
struct CommandOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: String,
}

async fn run_command(
    program: &Path,
    arguments: &[&std::ffi::OsStr],
    environment: Option<&[(&str, &std::ffi::OsStr)]>,
    cancellation: &CancellationSignal,
    progress: Option<(&Path, ProgressReporter)>,
) -> Result<CommandOutput, PipelineError> {
    ensure_not_canceled(cancellation)?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(environment) = environment {
        command.envs(environment.iter().map(|(key, value)| (*key, *value)));
    }
    let mut child = command.spawn().map_err(PipelineError::Io)?;
    let stdout = child.stdout.take().ok_or(PipelineError::Unavailable)?;
    let stderr = child.stderr.take().ok_or(PipelineError::Unavailable)?;
    let stdout_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stdout
            .take(MAX_MAPPER_STDOUT_BYTES)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let stderr_reader = tokio::spawn(async move {
        let mut bytes = Vec::new();
        stderr
            .take(MAX_MAPPER_STDERR_BYTES)
            .read_to_end(&mut bytes)
            .await
            .map(|_| bytes)
    });
    let mut ticker = interval(Duration::from_millis(300));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut progress_lines = 0_usize;
    let status = loop {
        tokio::select! {
            status = child.wait() => break status.map_err(PipelineError::Io)?,
            () = cancellation.cancelled() => {
                if let Some(environment) = environment {
                    if let Some((_, path)) = environment.iter().find(|(key, _)| *key == "RECITOPIA_CANCEL_FILE") {
                        let _ = fs::write(Path::new(path), b"cancel\n").await;
                    }
                }
                let _ = child.kill().await;
                let _ = child.wait().await;
                return Err(PipelineError::Canceled);
            }
            _ = ticker.tick() => {
                if let Some((path, reporter)) = &progress {
                    pump_progress_file(path, &mut progress_lines, reporter).await;
                }
            }
        }
    };
    if let Some((path, reporter)) = &progress {
        pump_progress_file(path, &mut progress_lines, reporter).await;
    }
    let stdout = stdout_reader.await.map_err(PipelineError::Join)??;
    let stderr = stderr_reader.await.map_err(PipelineError::Join)??;
    Ok(CommandOutput {
        success: status.success(),
        stdout,
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

async fn pump_progress_file(path: &Path, consumed: &mut usize, report: &ProgressReporter) {
    let Ok(contents) = fs::read_to_string(path).await else {
        return;
    };
    for line in contents.lines().skip(*consumed) {
        *consumed += 1;
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(total) = value.get("total").and_then(Value::as_u64) else {
            continue;
        };
        if total == 0 {
            continue;
        }
        let completed = value
            .get("completed")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        report(ProgressUpdate {
            stage: Some(ImportPipelineStage::DeepseekSection),
            message: Some("Extracting cookbook sections with DeepSeek.".to_owned()),
            current_section_index: usize::try_from(completed).ok(),
            section_total: usize::try_from(total).ok(),
            current_section_title: value
                .get("sectionTitle")
                .and_then(Value::as_str)
                .map(str::to_owned),
            ..ProgressUpdate::default()
        });
    }
}

fn mapper_request(
    import_record: &CookbookImport,
    cookbook: &Cookbook,
    pages: &[CookbookPage],
    sections: &[CookbookSection],
    blocks: &[CookbookContentBlock],
) -> Value {
    json!({
        "importId": import_record.id,
        "cookbookId": import_record.cookbook_id,
        "sourcePath": import_record.source_path,
        "cookbookTitle": cookbook.title,
        "authorIds": cookbook.author_ids,
        "sections": sections.iter().map(|section| json!({
            "id": section.id,
            "title": section.title,
            "kind": section_kind_name(section.kind),
            "pageStart": section.page_start,
            "pageEnd": section.page_end,
        })).collect::<Vec<_>>(),
        "contentBlocks": blocks.iter().map(|block| json!({
            "id": block.id,
            "sectionId": block.section_id,
            "pageStart": block.page_start,
            "pageEnd": block.page_end,
            "kind": content_block_kind_name(block.kind),
            "title": block.title,
            "text": block.text,
        })).collect::<Vec<_>>(),
        "pages": pages.iter().map(|page| json!({
            "id": page.id,
            "imageIndex": page.image_index,
            "printedPageLabel": page.printed_page_label,
            "printedPageNumber": page.printed_page_number,
            "pageKind": page_kind_name(page.page_kind),
            "ocrText": page.ocr_text,
        })).collect::<Vec<_>>(),
    })
}

fn build_source_map(
    import_record: &CookbookImport,
    pages: &[CookbookPage],
) -> (Vec<CookbookSection>, Vec<CookbookContentBlock>) {
    if pages.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut entries = pages
        .iter()
        .filter(|page| page.page_kind == CookbookPageKind::Contents)
        .flat_map(|page| contents_entries(&page.ocr_text))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.printed_page);
    entries.dedup_by(|left, right| {
        left.printed_page == right.printed_page
            || normalize_heading(&left.title) == normalize_heading(&right.title)
    });

    let mut starts = Vec::new();
    if entries.len() >= 3 {
        for entry in entries {
            let start = find_section_start(pages, &entry.title, entry.printed_page)
                .unwrap_or(entry.printed_page);
            starts.push((entry.title, start));
        }
    } else {
        for page in pages {
            if page.page_kind == CookbookPageKind::ChapterOpener {
                starts.push((page_heading(page), page_number(page)));
            }
        }
    }
    starts.sort_by_key(|(_, page)| *page);
    starts.dedup_by_key(|(_, page)| *page);

    let first_page = pages.iter().map(page_number).min().unwrap_or(1);
    let last_page = pages.iter().map(page_number).max().unwrap_or(first_page);
    let mut sections = Vec::new();
    if starts.is_empty() {
        sections.push(CookbookSection {
            id: format!("{}-section-1-document", import_record.id),
            cookbook_id: import_record.cookbook_id.clone(),
            parent_section_id: None,
            title: "Document".to_owned(),
            kind: CookbookSectionKind::FrontMatter,
            position: 1,
            page_start: Some(first_page),
            page_end: Some(last_page),
        });
    } else {
        let mut position = 1_u32;
        if starts[0].1 > first_page {
            sections.push(CookbookSection {
                id: format!("{}-section-front-matter", import_record.id),
                cookbook_id: import_record.cookbook_id.clone(),
                parent_section_id: None,
                title: "Front Matter".to_owned(),
                kind: CookbookSectionKind::FrontMatter,
                position,
                page_start: Some(first_page),
                page_end: Some(starts[0].1.saturating_sub(1)),
            });
            position += 1;
        }
        for (index, (title, start)) in starts.iter().enumerate() {
            let next = starts
                .get(index + 1)
                .map_or(last_page, |(_, page)| page.saturating_sub(1));
            sections.push(CookbookSection {
                id: format!(
                    "{}-section-{}-{}",
                    import_record.id,
                    index + 1,
                    slugify(title, "section")
                ),
                cookbook_id: import_record.cookbook_id.clone(),
                parent_section_id: None,
                title: title.clone(),
                kind: infer_section_kind(title),
                position,
                page_start: Some((*start).min(last_page)),
                page_end: Some(next.max(*start).min(last_page)),
            });
            position += 1;
        }
    }

    let mut blocks = Vec::new();
    for page in pages {
        if page.ocr_text.trim().is_empty()
            || matches!(
                page.page_kind,
                CookbookPageKind::Recipe | CookbookPageKind::Blank
            )
        {
            continue;
        }
        let number = page_number(page);
        let section_id = sections
            .iter()
            .find(|section| {
                section.page_start.is_some_and(|start| start <= number)
                    && section.page_end.is_none_or(|end| end >= number)
            })
            .map(|section| section.id.clone());
        blocks.push(CookbookContentBlock {
            id: format!("{}-block-page-{}", import_record.id, page.image_index),
            cookbook_id: import_record.cookbook_id.clone(),
            section_id,
            page_start: Some(number),
            page_end: Some(number),
            position: u32::try_from(blocks.len() + 1).unwrap_or(u32::MAX),
            kind: page_content_kind(page.page_kind),
            title: Some(page_heading(page)),
            text: page.ocr_text.clone(),
            has_text: true,
            confidence: page.average_confidence,
            source_json: json!({"source": "rust-source-map", "pageId": page.id}).to_string(),
        });
    }
    (sections, blocks)
}

#[derive(Debug)]
struct ContentsEntry {
    title: String,
    printed_page: u32,
}

fn contents_entries(text: &str) -> Vec<ContentsEntry> {
    let words = text.split_whitespace().collect::<Vec<_>>();
    let mut title_words = Vec::new();
    let mut entries = Vec::new();
    for word in words {
        let numeric = word.trim_matches(|character: char| !character.is_ascii_digit());
        let is_number = !numeric.is_empty()
            && numeric.len() <= 3
            && word.chars().filter(char::is_ascii_digit).count() == numeric.len();
        if is_number {
            if let Ok(number) = numeric.parse::<u32>() {
                let mut title = title_words.join(" ");
                title = title
                    .trim_matches(|character: char| !character.is_alphanumeric())
                    .to_owned();
                if let Some(stripped) = title.strip_prefix("Contents ") {
                    title = stripped.to_owned();
                }
                if (2..=100).contains(&title.len()) && number > 0 {
                    entries.push(ContentsEntry {
                        title,
                        printed_page: number,
                    });
                    title_words.clear();
                    continue;
                }
            }
        }
        title_words.push(word);
        if title_words.len() > 14 {
            title_words.remove(0);
        }
    }
    entries
}

fn find_section_start(pages: &[CookbookPage], title: &str, printed_page: u32) -> Option<u32> {
    let needle = normalize_heading(title);
    pages
        .iter()
        .find(|page| {
            let preview = page.ocr_text.chars().take(600).collect::<String>();
            let haystack = normalize_heading(&preview);
            page.printed_page_number == Some(printed_page)
                || (page.page_kind != CookbookPageKind::Contents
                    && !needle.is_empty()
                    && haystack.contains(&needle))
        })
        .map(page_number)
}

fn validate_printed_page_numbers(pages: &mut [CookbookPage]) {
    let detected = pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| page.printed_page_number.map(|number| (index, number)))
        .collect::<Vec<_>>();
    let mut rejected = HashSet::new();
    for pair in detected.windows(2) {
        let (left_index, left) = pair[0];
        let (right_index, right) = pair[1];
        let scan_delta = right_index.saturating_sub(left_index);
        let printed_delta = right.saturating_sub(left);
        let maximum_delta =
            u32::try_from(scan_delta.saturating_mul(4).saturating_add(4)).unwrap_or(u32::MAX);
        if right < left || printed_delta > maximum_delta {
            rejected.insert(right_index);
        }
    }
    for index in rejected {
        pages[index].printed_page_number = None;
        pages[index].printed_page_label = None;
    }

    let anchors = pages
        .iter()
        .enumerate()
        .filter_map(|(index, page)| page.printed_page_number.map(|number| (index, number)))
        .collect::<Vec<_>>();
    for pair in anchors.windows(2) {
        let (left_index, left) = pair[0];
        let (right_index, right) = pair[1];
        let scan_delta = right_index.saturating_sub(left_index);
        let printed_delta = right.saturating_sub(left) as usize;
        if scan_delta <= 1 || printed_delta < scan_delta || printed_delta > scan_delta * 4 {
            continue;
        }
        for offset in 1..scan_delta {
            if pages[left_index + offset].printed_page_number.is_none() {
                let estimate =
                    left as usize + (printed_delta * offset + scan_delta / 2) / scan_delta;
                if let Ok(estimate) = u32::try_from(estimate) {
                    pages[left_index + offset].printed_page_number = Some(estimate);
                    pages[left_index + offset].printed_page_label = Some(estimate.to_string());
                }
            }
        }
    }
}

fn classify_page(text: &str) -> CookbookPageKind {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CookbookPageKind::Blank;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("acknowledgement") || lower.contains("acknowledgment") {
        return CookbookPageKind::Acknowledgements;
    }
    if lower.starts_with("index") || lower.contains(" index ") && lower.len() < 8_000 {
        return CookbookPageKind::Index;
    }
    if lower.contains("suppliers") && lower.len() < 8_000 {
        return CookbookPageKind::Supplier;
    }
    if contents_entries(trimmed).len() >= 3 {
        return CookbookPageKind::Contents;
    }
    let first = trimmed.lines().next().unwrap_or_default().trim();
    if first.len() <= 3 && first.chars().all(|character| character.is_ascii_digit()) {
        return CookbookPageKind::ChapterOpener;
    }
    if lower.contains("serves ")
        || lower.contains("makes ")
        || lower.contains("ingredients") && lower.contains("method")
    {
        return CookbookPageKind::Recipe;
    }
    if lower.len() < 160 && (lower.contains("our ") || lower.contains("kitchen")) {
        return CookbookPageKind::Title;
    }
    CookbookPageKind::Essay
}

fn normalize_recipes(
    import_record: &CookbookImport,
    cookbook: &Cookbook,
    recipes: &mut Vec<Recipe>,
) {
    let mut seen = HashSet::new();
    recipes.retain_mut(|recipe| {
        recipe.id = slugify(&recipe.id, &slugify(&recipe.title, "imported-recipe"));
        if !seen.insert(recipe.id.clone()) {
            return false;
        }
        recipe.cookbook_id.clone_from(&import_record.cookbook_id);
        if recipe.author_ids.is_empty() {
            recipe.author_ids.clone_from(&cookbook.author_ids);
        }
        recipe.source_block_id = Some(format!("{}-recipe-{}", import_record.id, recipe.id));
        if recipe.tags.is_empty() {
            recipe.tags = vec!["imported".to_owned(), "needs-review".to_owned()];
        }
        if recipe.ingredients.is_empty() {
            recipe.ingredients.push(Ingredient {
                id: "ingredient-1".to_owned(),
                position: Some(1),
                display_name: "Review OCR ingredients".to_owned(),
                item: "Review OCR ingredients".to_owned(),
                quantity: None,
                quantity_text: None,
                quantity_min: None,
                quantity_max: None,
                quantity_kind: crate::model::IngredientQuantityKind::Unknown,
                quantity_review_status: crate::model::IngredientQuantityReviewStatus::NeedsReview,
                quantity_review_reason: Some("No ingredient rows were extracted.".to_owned()),
                unit: None,
                preparation: None,
                section: None,
                optional: false,
                alternative_text: None,
                source_line: None,
                source_page_id: None,
                unit_cost_cents: None,
                estimated_cost_cents: None,
            });
        }
        if recipe.steps.is_empty() {
            recipe.steps.push(InstructionStep {
                id: "step-1".to_owned(),
                position: 1,
                section: None,
                text: "Review OCR method.".to_owned(),
                source_page_id: None,
                source_line_start: None,
                source_line_end: None,
            });
        }
        for (index, ingredient) in recipe.ingredients.iter_mut().enumerate() {
            ingredient.id = format!("ingredient-{}", index + 1);
            ingredient.position = u32::try_from(index + 1).ok();
        }
        for (index, step) in recipe.steps.iter_mut().enumerate() {
            step.id = format!("step-{}", index + 1);
            step.position = u32::try_from(index + 1).unwrap_or(u32::MAX);
        }
        recipe.extraction_status = RecipeExtractionStatus::NeedsReview;
        recipe.notes.clear();
        recipe.last_made_at = None;
        recipe.times_made = 0;
        true
    });
}

fn normalize_content_blocks(import_record: &CookbookImport, blocks: &mut [CookbookContentBlock]) {
    for (index, block) in blocks.iter_mut().enumerate() {
        block.id = format!("{}-context-{}", import_record.id, index + 1);
        block.cookbook_id.clone_from(&import_record.cookbook_id);
        block.position = u32::try_from(index + 1).unwrap_or(u32::MAX);
        block.has_text = !block.text.trim().is_empty();
    }
}

#[must_use]
pub fn validate_draft(recipe: &Recipe) -> Vec<ImportIssue> {
    let mut issues = Vec::new();
    if recipe.title.trim().is_empty() {
        issues.push(ImportIssue {
            field: "title".to_owned(),
            message: "Recipe title needs review.".to_owned(),
            severity: ImportIssueSeverity::Error,
        });
    }
    if recipe.ingredients.is_empty() {
        issues.push(ImportIssue {
            field: "ingredients".to_owned(),
            message: "No ingredients were extracted.".to_owned(),
            severity: ImportIssueSeverity::Error,
        });
    }
    if recipe.steps.is_empty() {
        issues.push(ImportIssue {
            field: "steps".to_owned(),
            message: "No instructions were extracted.".to_owned(),
            severity: ImportIssueSeverity::Error,
        });
    }
    for ingredient in &recipe.ingredients {
        if ingredient.quantity_review_status
            == crate::model::IngredientQuantityReviewStatus::NeedsReview
        {
            issues.push(ImportIssue {
                field: format!("ingredients.{}.quantity", ingredient.id),
                message: ingredient
                    .quantity_review_reason
                    .clone()
                    .unwrap_or_else(|| "Ingredient quantity needs review.".to_owned()),
                severity: ImportIssueSeverity::Warning,
            });
        }
    }
    issues
}

fn context_only_blocks(blocks: &[CookbookContentBlock]) -> Vec<CookbookContentBlock> {
    blocks
        .iter()
        .filter(|block| {
            !matches!(
                block.kind,
                CookbookContentBlockKind::Recipe | CookbookContentBlockKind::RecipeHeadnote
            )
        })
        .cloned()
        .enumerate()
        .map(|(index, mut block)| {
            block.position = u32::try_from(index + 1).unwrap_or(u32::MAX);
            block
        })
        .collect()
}

fn page_number(page: &CookbookPage) -> u32 {
    page.printed_page_number.unwrap_or(page.image_index)
}

fn page_heading(page: &CookbookPage) -> String {
    page.ocr_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map_or_else(
            || format!("Page {}", page_number(page)),
            |line| line.chars().take(96).collect(),
        )
}

fn normalize_heading(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || character.is_ascii_whitespace())
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_start_matches(|character: char| character.is_ascii_digit() || character == ' ')
        .to_owned()
}

fn slugify(value: &str, fallback: &str) -> String {
    let mut slug = String::new();
    let mut separator = false;
    for character in value.to_ascii_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            if separator && !slug.is_empty() {
                slug.push('-');
            }
            slug.push(character);
            separator = false;
        } else {
            separator = true;
        }
        if slug.len() >= 80 {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.len() < 2 {
        fallback.to_owned()
    } else {
        slug
    }
}

fn infer_section_kind(title: &str) -> CookbookSectionKind {
    let lower = title.to_ascii_lowercase();
    if lower.contains("acknowledg") {
        CookbookSectionKind::BackMatter
    } else if lower.contains("supplier") || lower.contains("index") || lower.contains("pantry") {
        CookbookSectionKind::Reference
    } else if title.split_whitespace().next().is_some_and(|word| {
        word.len() == 2 && word.chars().all(|character| character.is_ascii_digit())
    }) {
        CookbookSectionKind::Chapter
    } else if lower.contains("introduction") {
        CookbookSectionKind::FrontMatter
    } else {
        CookbookSectionKind::Recipes
    }
}

const fn page_content_kind(kind: CookbookPageKind) -> CookbookContentBlockKind {
    match kind {
        CookbookPageKind::Supplier => CookbookContentBlockKind::Supplier,
        CookbookPageKind::Index => CookbookContentBlockKind::IndexEntry,
        CookbookPageKind::Recipe => CookbookContentBlockKind::Recipe,
        _ => CookbookContentBlockKind::Paragraph,
    }
}

const fn page_kind_name(kind: CookbookPageKind) -> &'static str {
    match kind {
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

const fn section_kind_name(kind: CookbookSectionKind) -> &'static str {
    match kind {
        CookbookSectionKind::FrontMatter => "front_matter",
        CookbookSectionKind::Chapter => "chapter",
        CookbookSectionKind::Essay => "essay",
        CookbookSectionKind::Reference => "reference",
        CookbookSectionKind::Recipes => "recipes",
        CookbookSectionKind::BackMatter => "back_matter",
    }
}

const fn content_block_kind_name(kind: CookbookContentBlockKind) -> &'static str {
    match kind {
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

fn ensure_not_canceled(cancellation: &CancellationSignal) -> Result<(), PipelineError> {
    if cancellation.is_canceled() {
        Err(PipelineError::Canceled)
    } else {
        Ok(())
    }
}

fn introduction_order_issues(text: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let mut previous = 0_usize;
    for marker in INTRODUCTION_EXPECTED_ORDER {
        let Some(relative) = text[previous..].find(marker) else {
            issues.push(format!("OCR text is missing expected passage: {marker}"));
            continue;
        };
        previous += relative + marker.len();
    }
    issues
}

fn preview(text: &str, maximum_characters: usize) -> String {
    text.chars().take(maximum_characters).collect()
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn import_root() -> PathBuf {
    PathBuf::from(
        std::env::var_os("RECITOPIA_RUST_IMPORT_DIR").unwrap_or_else(|| {
            std::env::var_os("RECITOPIA_IMPORT_DIR").unwrap_or_else(|| "../../data/imports".into())
        }),
    )
}

fn safe_job_id(value: &str) -> bool {
    value.len() >= 2
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

async fn remove_if_exists(path: &Path) -> Result<(), PipelineError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PipelineError::Io(error)),
    }
}

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("pipeline was canceled")]
    Canceled,
    #[error("pipeline worker service is unavailable")]
    Unavailable,
    #[error("OCR produced no usable page text")]
    OcrProducedNoText,
    #[error("diagnostic source page was not found")]
    DiagnosticPageNotFound,
    #[error("DeepSeek is not configured")]
    DeepSeekNotConfigured,
    #[error("recipe mapper returned no recipe")]
    MapperReturnedNoRecipe,
    #[error("{worker} failed: {detail}")]
    WorkerFailed {
        worker: &'static str,
        detail: String,
    },
    #[error("pipeline HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("pipeline filesystem/process operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("pipeline JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("pipeline reader task failed: {0}")]
    Join(#[from] tokio::task::JoinError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{CookbookSourceKind, ShareScope};

    fn import() -> CookbookImport {
        CookbookImport {
            id: "cookbook-import-test".to_owned(),
            cookbook_id: "book".to_owned(),
            source_kind: CookbookSourceKind::ImageSet,
            source_path: "fixture".to_owned(),
            status: CookbookImportStatus::OcrReady,
            ocr_engine: None,
            created_at: "2026-07-10T00:00:00Z".to_owned(),
            updated_at: "2026-07-10T00:00:00Z".to_owned(),
            review_notes: None,
        }
    }

    fn page(index: u32, kind: CookbookPageKind, text: &str) -> CookbookPage {
        CookbookPage {
            id: format!("page-{index}"),
            cookbook_id: "book".to_owned(),
            import_id: "cookbook-import-test".to_owned(),
            image_index: index,
            printed_page_label: None,
            printed_page_number: None,
            image_path: format!("/{index}.jpg"),
            image_hash: None,
            ocr_text: text.to_owned(),
            ocr_json: "{}".to_owned(),
            has_ocr_text: !text.is_empty(),
            average_confidence: None,
            minimum_confidence: None,
            page_kind: kind,
            review_status: CookbookPageReviewStatus::Pending,
        }
    }

    #[test]
    fn source_map_uses_contents_order_and_never_overlaps_sections() {
        let pages = vec![
            page(
                1,
                CookbookPageKind::Contents,
                "Introduction 7 01 Rice 23 02 Soups 55 Index 266",
            ),
            page(2, CookbookPageKind::Essay, "Introduction\nOpening text"),
            page(3, CookbookPageKind::ChapterOpener, "01\nRice"),
            page(4, CookbookPageKind::Recipe, "Serves 4\nRice"),
            page(5, CookbookPageKind::ChapterOpener, "02\nSoups"),
            page(6, CookbookPageKind::Index, "Index"),
        ];
        let (sections, _) = build_source_map(&import(), &pages);
        assert!(sections.len() >= 3);
        for pair in sections.windows(2) {
            assert!(pair[0].page_end.unwrap() < pair[1].page_start.unwrap());
        }
    }

    #[test]
    fn page_number_interpolation_requires_two_consistent_anchors() {
        let mut pages = vec![
            page(1, CookbookPageKind::Essay, "one"),
            page(2, CookbookPageKind::Essay, "two"),
            page(3, CookbookPageKind::Essay, "three"),
        ];
        pages[0].printed_page_number = Some(7);
        pages[2].printed_page_number = Some(9);
        validate_printed_page_numbers(&mut pages);
        assert_eq!(pages[1].printed_page_number, Some(8));

        let cookbook = Cookbook {
            id: "book".to_owned(),
            title: "Book".to_owned(),
            author_ids: vec!["author".to_owned()],
            isbn: None,
            publisher: None,
            published_year: None,
            cover_image_url: None,
            owner_user_id: None,
            family_id: None,
            share_scope: ShareScope::Personal,
            shared_with_user_ids: Vec::new(),
        };
        let request = mapper_request(&import(), &cookbook, &pages, &[], &[]);
        assert_eq!(request["pages"][1]["printedPageNumber"], 8);
    }
}
