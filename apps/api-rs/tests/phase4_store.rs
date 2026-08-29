#![cfg(feature = "duckdb-store")]

use duckdb::Connection;
use recitopia_api_rs::{
    config::{DatabaseConfig, StoreMode},
    duckdb_store::DuckStore,
    model::{
        AcceptPageContentInput, CookbookContentBlock, CookbookContentBlockKind,
        CookbookContentBlockPatch, CookbookCrossReference, CookbookGlossaryEntry, CookbookImport,
        CookbookImportStatus, CookbookIndexEntry, CookbookMenu, CookbookMenuRecipe, CookbookPage,
        CookbookPageKind, CookbookPagePatch, CookbookPageReviewStatus, CookbookSection,
        CookbookSectionKind, CookbookSourceImport, CookbookSourceKind, CookbookSupplier,
    },
    store::{ReadStore, StoreError, WriteStore},
};

const FIXTURE_SQL: &str = include_str!("fixtures/phase2_catalogue.sql");
const NOW: &str = "2026-07-10T12:00:00.000Z";

fn fixture_store() -> (tempfile::TempDir, DuckStore) {
    let directory = tempfile::tempdir().expect("temporary fixture directory");
    let path = directory.path().join("phase4.duckdb");
    {
        let connection = Connection::open(&path).expect("create fixture database");
        connection
            .execute_batch(FIXTURE_SQL)
            .expect("load phase 4 fixture");
    }
    let store = DuckStore::open(&DatabaseConfig {
        path,
        mode: StoreMode::ReadWrite,
    })
    .expect("open fixture read-write");
    (directory, store)
}

fn source_import(import_id: &str, hashes: [&str; 2]) -> CookbookSourceImport {
    let cookbook_id = "our-korean-kitchen";
    CookbookSourceImport {
        import_record: CookbookImport {
            id: import_id.to_owned(),
            cookbook_id: cookbook_id.to_owned(),
            source_kind: CookbookSourceKind::ImageSet,
            source_path: "fixture/phase-four".to_owned(),
            status: CookbookImportStatus::OcrReady,
            ocr_engine: Some("fixture-ocr".to_owned()),
            created_at: NOW.to_owned(),
            updated_at: NOW.to_owned(),
            review_notes: Some("Phase 4 transaction fixture".to_owned()),
        },
        pages: vec![
            page(import_id, 1, hashes[0], "", CookbookPageKind::Unknown),
            page(
                import_id,
                2,
                hashes[1],
                "Correctable source text",
                CookbookPageKind::Essay,
            ),
        ],
        sections: vec![CookbookSection {
            id: format!("{import_id}-section"),
            cookbook_id: cookbook_id.to_owned(),
            parent_section_id: None,
            title: "Fixture section".to_owned(),
            kind: CookbookSectionKind::Essay,
            position: 20,
            page_start: Some(40),
            page_end: Some(41),
        }],
        content_blocks: vec![CookbookContentBlock {
            id: format!("{import_id}-block"),
            cookbook_id: cookbook_id.to_owned(),
            section_id: Some(format!("{import_id}-section")),
            page_start: Some(40),
            page_end: Some(40),
            position: 20,
            kind: CookbookContentBlockKind::Paragraph,
            title: Some("Fixture introduction".to_owned()),
            text: "Original block text".to_owned(),
            has_text: true,
            confidence: Some(0.92),
            source_json: "{\"source\":\"fixture\"}".to_owned(),
        }],
        menus: vec![CookbookMenu {
            id: format!("{import_id}-menu"),
            cookbook_id: cookbook_id.to_owned(),
            source_block_id: Some(format!("{import_id}-block")),
            title: "Fixture menu".to_owned(),
            theme: Some("Weeknight".to_owned()),
            notes: None,
            recipes: vec![CookbookMenuRecipe {
                recipe_id: "recipe-1".to_owned(),
                position: 1,
                role: Some("main".to_owned()),
                serving_notes: None,
            }],
        }],
        glossary_entries: vec![CookbookGlossaryEntry {
            id: format!("{import_id}-glossary"),
            cookbook_id: cookbook_id.to_owned(),
            source_block_id: Some(format!("{import_id}-block")),
            title: "Gochujang".to_owned(),
            aliases: vec!["chilli paste".to_owned()],
            native_names: Vec::new(),
            description: "Fermented chilli paste".to_owned(),
            storage_notes: Some("Refrigerate".to_owned()),
            substitution_notes: None,
            page_start: Some(40),
            page_end: Some(40),
        }],
        suppliers: vec![CookbookSupplier {
            id: format!("{import_id}-supplier"),
            cookbook_id: cookbook_id.to_owned(),
            source_block_id: Some(format!("{import_id}-block")),
            name: "Fixture grocer".to_owned(),
            url: Some("https://example.test/grocer".to_owned()),
            region: Some("London".to_owned()),
            notes: None,
            source_page: Some(40),
            review_status: CookbookPageReviewStatus::Pending,
        }],
        index_entries: vec![CookbookIndexEntry {
            id: format!("{import_id}-index"),
            cookbook_id: cookbook_id.to_owned(),
            term: "rice".to_owned(),
            subterm: None,
            target_page_label: Some("40".to_owned()),
            target_page_number: Some(40),
            target_recipe_id: Some("recipe-1".to_owned()),
            illustration: false,
        }],
        cross_references: vec![CookbookCrossReference {
            id: format!("{import_id}-reference"),
            cookbook_id: cookbook_id.to_owned(),
            from_kind: "glossary".to_owned(),
            from_id: format!("{import_id}-glossary"),
            to_kind: "recipe".to_owned(),
            to_id: Some("recipe-1".to_owned()),
            label: Some("Used by".to_owned()),
            relation_kind: "mentions".to_owned(),
        }],
    }
}

fn page(
    import_id: &str,
    image_index: u32,
    image_hash: &str,
    ocr_text: &str,
    page_kind: CookbookPageKind,
) -> CookbookPage {
    CookbookPage {
        id: format!("{import_id}-page-{image_index}"),
        cookbook_id: "our-korean-kitchen".to_owned(),
        import_id: import_id.to_owned(),
        image_index,
        printed_page_label: Some((image_index + 39).to_string()),
        printed_page_number: Some(image_index + 39),
        image_path: format!("/fixture/{import_id}-{image_index}.jpg"),
        image_hash: Some(image_hash.to_owned()),
        ocr_text: ocr_text.to_owned(),
        ocr_json: "{\"boxes\":[1]}".to_owned(),
        has_ocr_text: !ocr_text.is_empty(),
        average_confidence: Some(0.92),
        minimum_confidence: Some(0.8),
        page_kind,
        review_status: CookbookPageReviewStatus::Pending,
    }
}

#[test]
fn source_import_round_trips_every_phase_four_record_type() {
    let (_directory, store) = fixture_store();
    let first = "a".repeat(64);
    let second = "b".repeat(64);
    let source = source_import("phase-four-import", [&first, &second]);

    store
        .create_cookbook_source_import(source)
        .expect("create source import");
    let catalogue = store.catalogue_summary().expect("read catalogue");
    assert!(
        catalogue
            .cookbook_imports
            .iter()
            .any(|import| import.id == "phase-four-import")
    );
    assert_eq!(
        catalogue
            .cookbook_pages
            .iter()
            .filter(|page| page.import_id == "phase-four-import")
            .count(),
        2
    );
    assert!(
        catalogue
            .cookbook_glossary_entries
            .iter()
            .any(|entry| entry.id == "phase-four-import-glossary")
    );
    assert!(
        catalogue
            .cookbook_cross_references
            .iter()
            .any(|entry| entry.id == "phase-four-import-reference")
    );
}

#[test]
fn duplicate_hashes_and_child_failures_roll_back_the_whole_import() {
    let (_directory, store) = fixture_store();
    let duplicate = "d".repeat(64);
    let source = source_import("duplicate-import", [&duplicate, &duplicate]);
    assert!(matches!(
        store.create_cookbook_source_import(source),
        Err(StoreError::DuplicateCookbookPageImage)
    ));

    let first = "e".repeat(64);
    let second = "f".repeat(64);
    let mut rollback = source_import("rollback-import", [&first, &second]);
    let duplicate_recipe = rollback.menus[0].recipes[0].clone();
    rollback.menus[0].recipes.push(duplicate_recipe);
    assert!(matches!(
        store.create_cookbook_source_import(rollback),
        Err(StoreError::Backend { .. })
    ));
    let catalogue = store.catalogue_summary().unwrap();
    assert!(
        catalogue
            .cookbook_imports
            .iter()
            .all(|import| import.id != "duplicate-import" && import.id != "rollback-import")
    );
    assert!(
        catalogue
            .cookbook_pages
            .iter()
            .all(|page| page.import_id != "duplicate-import" && page.import_id != "rollback-import")
    );
}

#[test]
fn corrections_and_page_acceptance_preserve_sources_and_are_atomic() {
    let (_directory, store) = fixture_store();
    let first = "1".repeat(64);
    let second = "2".repeat(64);
    store
        .create_cookbook_source_import(source_import("review-import", [&first, &second]))
        .unwrap();

    let page_id = "review-import-page-2";
    let page = store
        .patch_cookbook_page(
            page_id,
            CookbookPagePatch {
                page_kind: Some(CookbookPageKind::Essay),
                review_status: Some(CookbookPageReviewStatus::NeedsOcrFix),
                ocr_text: Some("Corrected two-column introduction".to_owned()),
            },
        )
        .expect("patch page");
    assert_eq!(page.ocr_json, "{\"boxes\":[1]}");
    assert_eq!(page.review_status, CookbookPageReviewStatus::NeedsOcrFix);

    let block = store
        .patch_cookbook_content_block(
            "review-import-block",
            CookbookContentBlockPatch {
                text: Some("Corrected connective text".to_owned()),
                title: Some(String::new()),
            },
        )
        .expect("patch block");
    assert_eq!(block.text, "Corrected connective text");
    assert_eq!(block.title, None);
    assert_eq!(block.source_json, "{\"source\":\"fixture\"}");

    let accepted = store
        .accept_cookbook_page_content(
            page_id,
            AcceptPageContentInput {
                kind: Some(CookbookContentBlockKind::Callout),
                title: Some("Introduction".to_owned()),
            },
        )
        .expect("accept page content");
    assert_eq!(accepted.id, "review-import-page-2-content");
    assert_eq!(
        accepted.section_id.as_deref(),
        Some("review-import-section")
    );
    assert_eq!(accepted.page_start, Some(41));
    assert_eq!(accepted.kind, CookbookContentBlockKind::Callout);
    assert!(matches!(
        store.accept_cookbook_page_content(page_id, AcceptPageContentInput::default()),
        Err(StoreError::CookbookPageAlreadyAccepted)
    ));
    assert!(matches!(
        store.accept_cookbook_page_content(
            "review-import-page-1",
            AcceptPageContentInput::default()
        ),
        Err(StoreError::CookbookPageHasNoText)
    ));
    let accepted_page = store
        .catalogue_summary()
        .unwrap()
        .cookbook_pages
        .into_iter()
        .find(|page| page.id == page_id)
        .unwrap();
    assert_eq!(
        accepted_page.review_status,
        CookbookPageReviewStatus::Accepted
    );
}
