#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::format_push_string,
    clippy::module_name_repetitions,
    clippy::needless_pass_by_value,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::unused_self
)]

use std::{
    collections::{BTreeMap, HashMap},
    env,
    error::Error,
    ffi::OsStr,
    fs,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode},
    time::{SystemTime, UNIX_EPOCH},
};

use recitopia_api_rs::model::{
    Catalogue, CookLogEntry, Cookbook, CookbookContentBlock, CookbookContentBlockKind,
    CookbookPage, Ingredient, IngredientQuantityKind, IngredientQuantityReviewStatus,
    InstructionStep, MealPlanEntry, PantryItem, Recipe,
};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const DEFAULT_API_URL: &str = "http://127.0.0.1:8077";
const WORKSPACE_MARKER: &str = ".recitopia-workspace.json";
const COOK_EXTENSION: &str = "cook";
const MARKDOWN_EXTENSION: &str = "md";
const DATA_DIR: &str = ".recitopia/data";
const COOK_SCHEMA: &str = "recitopia.cook/v1";
const MARKDOWN_SCHEMA: &str = "recitopia.markdown/v1";

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("recitopia-hx: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map_or("open", String::as_str).to_owned();
    if matches!(
        command.as_str(),
        "open" | "materialize" | "lsp" | "help" | "--help" | "-h"
    ) && !args.is_empty()
    {
        args.remove(0);
    }

    match command.as_str() {
        "open" => open_workspace(parse_workspace_args(&args, true)?).await,
        "materialize" => open_workspace(parse_workspace_args(&args, false)?).await,
        "lsp" => {
            let options = parse_lsp_args(&args)?;
            LspServer::new(options)?.run()
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => Err(format!("unknown command {other:?}; run `recitopia-hx help`").into()),
    }
}

fn print_help() {
    println!(
        "recitopia-hx\n\n\
         Usage:\n\
           recitopia-hx open [--api-url URL] [--workspace DIR] [--helix BIN] [--no-helix]\n\
           recitopia-hx materialize [--api-url URL] [--workspace DIR]\n\
           recitopia-hx lsp [--api-url URL]\n\n\
         The `open` command writes a temporary Helix workspace of `.cook` and `.md` files.\n\
         The generated `.helix/languages.toml` starts `recitopia-hx lsp` for navigation,\n\
         diagnostics, and recipe save-back from Recitopia-profile `.cook` files.\n\
         Default API: http://127.0.0.1:8077. Override with RECITOPIA_API_URL or --api-url."
    );
}

#[derive(Clone)]
struct WorkspaceArgs {
    api_url: String,
    workspace: Option<PathBuf>,
    helix_bin: String,
    launch_helix: bool,
}

#[derive(Clone)]
struct LspArgs {
    api_url: String,
}

fn parse_workspace_args(
    args: &[String],
    default_launch_helix: bool,
) -> Result<WorkspaceArgs, String> {
    let mut api_url = env::var("RECITOPIA_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    let mut workspace = None;
    let mut helix_bin = env::var("RECITOPIA_HELIX_BIN").unwrap_or_else(|_| "hx".to_owned());
    let mut launch_helix = default_launch_helix;
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--api-url" => {
                index += 1;
                api_url = args
                    .get(index)
                    .ok_or("--api-url requires a value")?
                    .to_owned();
            }
            "--workspace" => {
                index += 1;
                workspace = Some(PathBuf::from(
                    args.get(index).ok_or("--workspace requires a value")?,
                ));
            }
            "--helix" => {
                index += 1;
                helix_bin = args
                    .get(index)
                    .ok_or("--helix requires a value")?
                    .to_owned();
            }
            "--no-helix" => launch_helix = false,
            unknown => return Err(format!("unknown option {unknown:?}")),
        }
        index += 1;
    }

    Ok(WorkspaceArgs {
        api_url,
        workspace,
        helix_bin,
        launch_helix,
    })
}

fn parse_lsp_args(args: &[String]) -> Result<LspArgs, String> {
    let mut api_url = env::var("RECITOPIA_API_URL").unwrap_or_else(|_| DEFAULT_API_URL.to_owned());
    let mut index = 0;

    while index < args.len() {
        match args[index].as_str() {
            "--api-url" => {
                index += 1;
                api_url = args
                    .get(index)
                    .ok_or("--api-url requires a value")?
                    .to_owned();
            }
            unknown => return Err(format!("unknown option {unknown:?}")),
        }
        index += 1;
    }

    Ok(LspArgs { api_url })
}

async fn open_workspace(args: WorkspaceArgs) -> Result<(), Box<dyn Error>> {
    let client = ApiClient::new(&args.api_url);
    let workspace = match args.workspace {
        Some(path) => path,
        None => default_workspace_path()?,
    };
    materialize_workspace(&client, &workspace).await?;
    println!("wrote Recitopia Helix workspace: {}", workspace.display());

    if args.launch_helix {
        let entry = workspace.join("README.md");
        let status = Command::new(&args.helix_bin)
            .arg(entry)
            .current_dir(&workspace)
            .status()
            .map_err(|error| {
                format!("failed to launch Helix with `{}`: {error}", args.helix_bin)
            })?;
        if !status.success() {
            return Err(format!("Helix exited with {status}").into());
        }
    }

    Ok(())
}

fn default_workspace_path() -> Result<PathBuf, Box<dyn Error>> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?;
    Ok(env::temp_dir().join(format!(
        "recitopia-hx-{}-{}",
        now.as_millis(),
        std::process::id()
    )))
}

#[derive(Clone)]
struct ApiClient {
    base_url: String,
    client: reqwest::Client,
}

impl ApiClient {
    fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            client: reqwest::Client::new(),
        }
    }

    async fn get_json<T>(&self, path: &str) -> Result<T, Box<dyn Error>>
    where
        T: serde::de::DeserializeOwned,
    {
        let response = self.client.get(self.url(path)).send().await?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("GET {path} failed with {status}: {}", compact(&text)).into());
        }
        Ok(response.json().await?)
    }

    async fn put_recipe(&self, recipe: &Recipe) -> Result<Recipe, Box<dyn Error>> {
        let path = format!("/api/recipes/{}", recipe.id);
        let response = self.client.put(self.url(&path)).json(recipe).send().await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            return Err(format!("recipe {:?} was not found by the API", recipe.id).into());
        }
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(format!("PUT {path} failed with {status}: {}", compact(&text)).into());
        }
        Ok(response.json().await?)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn compact(value: &str) -> String {
    let compacted = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if compacted.len() > 220 {
        format!("{}...", &compacted[..220])
    } else {
        compacted
    }
}

async fn materialize_workspace(client: &ApiClient, workspace: &Path) -> Result<(), Box<dyn Error>> {
    let catalogue = client.get_json::<Catalogue>("/api/catalogue").await?;
    let pantry = client.get_json::<Vec<PantryItem>>("/api/pantry").await?;
    let meal_plan = client
        .get_json::<Vec<MealPlanEntry>>("/api/meal-plan")
        .await?;
    let cook_log = client
        .get_json::<Vec<CookLogEntry>>("/api/cook-log")
        .await?;

    fs::create_dir_all(workspace)?;
    fs::create_dir_all(workspace.join(".helix"))?;
    fs::create_dir_all(workspace.join(DATA_DIR).join("recipes"))?;
    fs::create_dir_all(workspace.join(DATA_DIR).join("cookbooks"))?;
    fs::create_dir_all(workspace.join("recipes"))?;
    fs::create_dir_all(workspace.join("cookbooks"))?;
    fs::create_dir_all(workspace.join("source"))?;
    fs::create_dir_all(workspace.join("tags"))?;
    fs::create_dir_all(workspace.join("categories"))?;
    fs::create_dir_all(workspace.join("ingredients"))?;
    fs::create_dir_all(workspace.join("time"))?;

    write_workspace_marker(workspace, &client.base_url)?;
    write_helix_config(workspace, &client.base_url)?;
    write_readme(workspace, &catalogue, &pantry, &meal_plan)?;
    write_search_guide(workspace)?;
    write_catalogue(workspace, &catalogue)?;
    write_json_document(
        workspace.join(DATA_DIR).join("pantry.json"),
        "pantry",
        "Pantry",
        &pantry,
    )?;
    write_json_document(
        workspace.join(DATA_DIR).join("meal-plan.json"),
        "meal_plan",
        "Meal Plan",
        &meal_plan,
    )?;
    write_json_document(
        workspace.join(DATA_DIR).join("history.json"),
        "cook_log",
        "Cook History",
        &cook_log,
    )?;
    write_pantry(workspace, &pantry)?;
    write_meal_plan(workspace, &meal_plan, &catalogue)?;
    write_history(workspace, &cook_log, &catalogue)?;

    for cookbook in &catalogue.cookbooks {
        write_cookbook(workspace, &catalogue, cookbook)?;
        write_json_document(
            workspace
                .join(DATA_DIR)
                .join("cookbooks")
                .join(format!("{}.json", cookbook.id)),
            "cookbook",
            &cookbook.title,
            cookbook,
        )?;
    }
    write_cookbooks_root(workspace, &catalogue)?;
    write_source_pages(workspace, &catalogue)?;
    write_content_blocks(workspace, &catalogue)?;

    for recipe in &catalogue.recipes {
        write_recipe(workspace, &catalogue, recipe)?;
        write_json_document(
            workspace
                .join(DATA_DIR)
                .join("recipes")
                .join(format!("{}.json", recipe.id)),
            "recipe",
            &recipe.title,
            recipe,
        )?;
    }
    write_all_recipes(workspace, &catalogue)?;
    write_recipe_collections(workspace, &catalogue)?;

    Ok(())
}

fn write_workspace_marker(workspace: &Path, api_url: &str) -> Result<(), Box<dyn Error>> {
    let marker = json!({
        "kind": "recitopia-helix-workspace",
        "apiUrl": api_url,
        "generatedBy": "recitopia-hx",
        "recipeExtension": COOK_EXTENSION,
        "contentExtension": MARKDOWN_EXTENSION,
        "recipeSchema": COOK_SCHEMA,
        "contentSchema": MARKDOWN_SCHEMA,
    });
    fs::write(
        workspace.join(WORKSPACE_MARKER),
        format!("{}\n", serde_json::to_string_pretty(&marker)?),
    )?;
    Ok(())
}

fn write_helix_config(workspace: &Path, api_url: &str) -> Result<(), Box<dyn Error>> {
    let current_exe = env::current_exe()?;
    let config = format!(
        "[language-server.recitopia]\n\
         command = \"{}\"\n\
         args = [\"lsp\", \"--api-url\", \"{}\"]\n\n\
         [[language]]\n\
         name = \"recitopia\"\n\
         scope = \"source.recitopia\"\n\
         file-types = [\"cook\", \"md\"]\n\
         roots = [\"{}\"]\n\
         language-servers = [\"recitopia\"]\n\
         auto-format = false\n",
        toml_escape(&current_exe),
        toml_escape(api_url),
        WORKSPACE_MARKER
    );
    fs::write(workspace.join(".helix").join("languages.toml"), config)?;
    Ok(())
}

fn write_readme(
    workspace: &Path,
    catalogue: &Catalogue,
    pantry: &[PantryItem],
    meal_plan: &[MealPlanEntry],
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("index", "recitopia"));
    text.push_str("# Recitopia\n\n");
    text.push_str("A local, file-shaped view of the remote Recitopia catalogue.\n\n");
    text.push_str(&format!(
        "{} cookbooks, {} recipes, {} pantry items, {} planned meal.\n\n",
        catalogue.cookbooks.len(),
        catalogue.recipes.len(),
        pantry.len(),
        meal_plan.len()
    ));
    text.push_str("## Start Here\n\n");
    text.push_str("- [All recipes](all-recipes.md)\n");
    text.push_str("- [Browse by cookbook](cookbooks/README.md)\n");
    text.push_str("- [Browse by tag](tags/README.md)\n");
    text.push_str("- [Browse by category](categories/README.md)\n");
    text.push_str("- [Browse by ingredient](ingredients/README.md)\n");
    text.push_str("- [Browse by time](time/README.md)\n");
    text.push_str("- [Source pages](source/README.md)\n");
    text.push_str("- [Search guide](search.md)\n");
    text.push_str("- [Pantry](pantry.md)\n");
    text.push_str("- [Meal plan](meal-plan.md)\n");
    text.push_str("- [Cook history](history.md)\n\n");
    text.push_str("## Cookbooks\n\n");
    for cookbook in &catalogue.cookbooks {
        let cookbook_path = format!(
            "cookbooks/{}/README.md",
            slug_file(&cookbook.title, &cookbook.id)
        );
        text.push_str(&format!(
            "- [{}]({}) - {} recipes\n",
            cookbook.title,
            cookbook_path,
            count_recipes(catalogue, &cookbook.id)
        ));
    }
    text.push_str("\n## Helpful Filesystem Moves\n\n");
    text.push_str("- Use Helix's file picker to jump around `recipes/`, `cookbooks/`, `tags/`, `ingredients/`, or `time/`.\n");
    text.push_str(
        "- Run `rg -i \"lentil|kimchi|quick\"` in this workspace when you want a content search.\n",
    );
    text.push_str("- Recipe files are canonical `.cook` files under `recipes/`; grouped directories contain links back to them.\n");
    fs::write(workspace.join("README.md"), text)?;
    Ok(())
}

fn write_catalogue(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("catalogue", "catalogue"));
    text.push_str("# Catalogue\n\n");
    text.push_str("Human-facing catalogue summary. Raw JSON lives under `.recitopia/data/`.\n\n");
    text.push_str(&format!(
        "- Cookbooks: {}\n- Recipes: {}\n- Pages: {}\n- Content blocks: {}\n\n",
        catalogue.cookbooks.len(),
        catalogue.recipes.len(),
        catalogue.cookbook_pages.len(),
        catalogue.cookbook_content_blocks.len()
    ));
    text.push_str("## Cookbooks\n\n");
    for cookbook in &catalogue.cookbooks {
        text.push_str(&format!(
            "- [{}](cookbooks/{}/README.md)\n",
            cookbook.title,
            slug_file(&cookbook.title, &cookbook.id)
        ));
    }
    fs::write(workspace.join("catalogue.md"), text)?;
    write_json_document(
        workspace.join(DATA_DIR).join("catalogue.json"),
        "catalogue",
        "Catalogue JSON",
        catalogue,
    )?;
    Ok(())
}

fn write_search_guide(workspace: &Path) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("index", "search"));
    text.push_str("# Search Guide\n\n");
    text.push_str("The workspace is meant to behave like a small POSIX cookbook shelf.\n\n");
    text.push_str("## From Helix\n\n");
    text.push_str("- Open the file picker and type part of a recipe title.\n");
    text.push_str("- Use `/` inside `all-recipes.md` or a cookbook README.\n");
    text.push_str(
        "- Put the cursor on a backticked recipe or cookbook ID and use goto-definition.\n\n",
    );
    text.push_str("## From A Shell In This Directory\n\n");
    text.push_str("```sh\n");
    text.push_str(
        "rg -i \"gochujang|lentil|noodle\" recipes cookbooks tags categories ingredients time\n",
    );
    text.push_str("find recipes -iname '*kimchi*'\n");
    text.push_str("find tags -maxdepth 2 -type l\n");
    text.push_str("```\n");
    fs::write(workspace.join("search.md"), text)?;
    Ok(())
}

fn write_recipe(
    workspace: &Path,
    catalogue: &Catalogue,
    recipe: &Recipe,
) -> Result<(), Box<dyn Error>> {
    let path = workspace.join("recipes").join(recipe_file_name(recipe));
    let mut text = String::new();
    text.push_str(&cook_frontmatter(catalogue, recipe)?);
    if let Some(subtitle) = &recipe.subtitle {
        text.push_str(&format!("> {subtitle}\n\n"));
    }
    if let Some(headnote) = &recipe.headnote {
        for line in headnote.trim().lines() {
            text.push_str(&format!("> {}\n", line.trim()));
        }
        text.push('\n');
    }

    text.push_str("= Ingredients\n\n");
    if recipe.ingredients.is_empty() {
        text.push_str("-- No ingredients recorded.\n");
    } else {
        for ingredient in &recipe.ingredients {
            text.push_str(&format!(
                "-- recitopia:ingredient id={} position={}\n",
                ingredient.id,
                ingredient.position.unwrap_or(0)
            ));
            text.push_str(&cook_ingredient_line(ingredient));
            text.push('\n');
        }
    }

    text.push_str("\n= Method\n\n");
    if recipe.steps.is_empty() {
        text.push_str("-- No method recorded.\n");
    } else {
        for step in &recipe.steps {
            text.push_str(&format!(
                "-- recitopia:step id={} position={}\n",
                step.id, step.position
            ));
            text.push_str(step.text.trim());
            text.push_str("\n\n");
        }
    }

    if !recipe.notes.is_empty() {
        text.push_str("= Notes\n\n");
        for note in &recipe.notes {
            text.push_str(&format!("> {}\n", note.text.trim()));
        }
        text.push('\n');
    }

    text.push_str("[- recitopia:json\n");
    text.push_str(&serde_json::to_string_pretty(recipe)?);
    text.push_str("\n-]\n");
    fs::write(path, text)?;
    Ok(())
}

fn cook_frontmatter(catalogue: &Catalogue, recipe: &Recipe) -> Result<String, Box<dyn Error>> {
    let cookbook_title = catalogue
        .cookbooks
        .iter()
        .find(|cookbook| cookbook.id == recipe.cookbook_id)
        .map(|cookbook| cookbook.title.as_str());
    let mut metadata = json!({
        "schema": COOK_SCHEMA,
        "kind": "recipe",
        "id": &recipe.id,
        "title": &recipe.title,
        "cookbook": {
            "id": &recipe.cookbook_id,
            "title": cookbook_title,
        },
        "source": {
            "label": &recipe.source_label,
            "pages": recipe_source_pages(recipe),
        },
        "tags": &recipe.tags,
    });
    insert_optional_string(&mut metadata, "subtitle", recipe.subtitle.as_deref());
    insert_optional_string(&mut metadata, "category", recipe.category.as_deref());
    insert_optional_string(&mut metadata, "cuisine", recipe.cuisine.as_deref());
    if let Some(yield_quantity) = recipe.yield_quantity {
        metadata["yield"] = json!({
            "quantity": yield_quantity,
            "unit": recipe.yield_unit,
        });
    }
    if recipe.prep_minutes.is_some()
        || recipe.cook_minutes.is_some()
        || recipe.total_minutes.is_some()
    {
        metadata["time"] = json!({
            "prep_minutes": recipe.prep_minutes,
            "cook_minutes": recipe.cook_minutes,
            "total_minutes": recipe.total_minutes,
        });
    }
    if recipe.times_made > 0 || recipe.last_made_at.is_some() {
        metadata["history"] = json!({
            "times_made": recipe.times_made,
            "last_made_at": recipe.last_made_at,
        });
    }
    Ok(format!(
        "---\n{}\n---\n\n",
        serde_json::to_string_pretty(&metadata)?
    ))
}

fn recipe_source_pages(recipe: &Recipe) -> Vec<u32> {
    match (recipe.page_start, recipe.page_end) {
        (Some(start), Some(end)) if start <= end => (start..=end).collect(),
        (Some(page), _) | (_, Some(page)) => vec![page],
        _ => Vec::new(),
    }
}

fn insert_optional_string(metadata: &mut Value, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        metadata[key] = json!(value);
    }
}

fn cook_ingredient_line(ingredient: &Ingredient) -> String {
    let name = if ingredient.item.trim().is_empty() {
        ingredient.display_name.trim()
    } else {
        ingredient.item.trim()
    };
    let mut line = format!(
        "@{}{{{}}}",
        cook_escape_name(name),
        cook_ingredient_quantity(ingredient)
    );
    if let Some(preparation) = &ingredient.preparation {
        if !preparation.trim().is_empty() {
            line.push_str(&format!("({})", cook_escape_annotation(preparation.trim())));
        }
    }
    if ingredient.optional {
        line.push_str(" -- optional");
    }
    line
}

fn cook_ingredient_quantity(ingredient: &Ingredient) -> String {
    if let Some(quantity_text) = &ingredient.quantity_text {
        if !quantity_text.trim().is_empty() {
            let mut value = cook_escape_quantity(quantity_text.trim());
            if let Some(unit) = &ingredient.unit {
                if !unit.trim().is_empty() {
                    value.push('%');
                    value.push_str(&cook_escape_quantity(unit.trim()));
                }
            }
            return value;
        }
    }
    let Some(quantity) = ingredient.quantity else {
        return String::new();
    };
    let mut value = human_number(quantity);
    if let Some(unit) = &ingredient.unit {
        if !unit.trim().is_empty() {
            value.push('%');
            value.push_str(&cook_escape_quantity(unit.trim()));
        }
    }
    value
}

fn cook_escape_name(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(['{', '}'], "")
        .trim()
        .to_owned()
}

fn cook_escape_quantity(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(['{', '}'], "")
        .trim()
        .to_owned()
}

fn cook_escape_annotation(value: &str) -> String {
    value
        .replace(['\n', '\r'], " ")
        .replace(['(', ')'], "")
        .trim()
        .to_owned()
}

fn write_json_document<T>(
    path: PathBuf,
    _kind: &str,
    _title: &str,
    value: &T,
) -> Result<(), Box<dyn Error>>
where
    T: Serialize,
{
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

fn write_cookbooks_root(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("index", "cookbooks"));
    text.push_str("# Cookbooks\n\n");
    for cookbook in &catalogue.cookbooks {
        text.push_str(&format!(
            "- [{}]({}/README.md) - {} recipes\n",
            cookbook.title,
            slug_file(&cookbook.title, &cookbook.id),
            count_recipes(catalogue, &cookbook.id)
        ));
    }
    fs::write(workspace.join("cookbooks").join("README.md"), text)?;
    Ok(())
}

fn write_cookbook(
    workspace: &Path,
    catalogue: &Catalogue,
    cookbook: &Cookbook,
) -> Result<(), Box<dyn Error>> {
    let cookbook_dir = workspace
        .join("cookbooks")
        .join(slug_file(&cookbook.title, &cookbook.id));
    fs::create_dir_all(&cookbook_dir)?;

    let recipes = recipes_for_cookbook(catalogue, &cookbook.id);
    let mut text = String::new();
    text.push_str(&metadata_header("cookbook", &cookbook.id));
    text.push_str(&format!("# {}\n\n", cookbook.title));
    if let Some(year) = cookbook.published_year {
        text.push_str(&format!("- Published: {year}\n"));
    }
    if let Some(publisher) = &cookbook.publisher {
        text.push_str(&format!("- Publisher: {publisher}\n"));
    }
    text.push_str(&format!("- ID: `{}`\n", cookbook.id));
    text.push_str(&format!("- Recipes: {}\n\n", recipes.len()));
    text.push_str("## Source\n\n");
    text.push_str(&format!(
        "- [Source pages](../../source/{}/README.md)\n",
        slug_file(&cookbook.title, &cookbook.id)
    ));
    text.push_str("- [Interpreted content](content/README.md)\n\n");
    text.push_str("## Recipes\n\n");
    for recipe in recipes {
        let file_name = recipe_file_name(recipe);
        link_recipe(workspace, &cookbook_dir.join(&file_name), recipe)?;
        text.push_str(&format!(
            "- [{}]({})",
            recipe.title,
            markdown_link_target(&file_name)
        ));
        if let Some(total) = recipe.total_minutes {
            text.push_str(&format!(" - {total}m"));
        }
        if !recipe.tags.is_empty() {
            text.push_str(&format!(" - {}", recipe.tags.join(", ")));
        }
        text.push('\n');
    }
    fs::write(cookbook_dir.join("README.md"), text)?;
    Ok(())
}

fn write_source_pages(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("source", "source"));
    text.push_str("# Source Pages\n\n");
    for cookbook in &catalogue.cookbooks {
        let pages = source_pages_for_cookbook(catalogue, &cookbook.id);
        if pages.is_empty() {
            continue;
        }
        let slug = slug_file(&cookbook.title, &cookbook.id);
        text.push_str(&format!(
            "- [{}]({}/README.md) - {} pages\n",
            cookbook.title,
            slug,
            pages.len()
        ));

        let cookbook_dir = workspace.join("source").join(&slug);
        let pages_dir = cookbook_dir.join("pages");
        fs::create_dir_all(&pages_dir)?;
        write_source_cookbook_index(&cookbook_dir, cookbook, &pages)?;
        for page in pages {
            write_source_page(&pages_dir, page)?;
        }
    }
    fs::write(workspace.join("source").join("README.md"), text)?;
    Ok(())
}

fn write_source_cookbook_index(
    cookbook_dir: &Path,
    cookbook: &Cookbook,
    pages: &[&CookbookPage],
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("source_cookbook", &cookbook.id));
    text.push_str(&format!("# {} Source Pages\n\n", cookbook.title));
    for page in pages {
        text.push_str(&format!(
            "- [{}](pages/{})",
            source_page_title(page),
            source_page_file_name(page)
        ));
        if let Some(confidence) = page.average_confidence {
            text.push_str(&format!(" - confidence {:.0}%", confidence * 100.0));
        }
        text.push('\n');
    }
    fs::write(cookbook_dir.join("README.md"), text)?;
    Ok(())
}

fn write_source_page(pages_dir: &Path, page: &CookbookPage) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&markdown_frontmatter(json!({
        "kind": "source_page",
        "id": &page.id,
        "cookbook_id": &page.cookbook_id,
        "import_id": &page.import_id,
        "image_index": page.image_index,
        "printed_page_label": &page.printed_page_label,
        "printed_page_number": page.printed_page_number,
        "image_path": &page.image_path,
        "image_hash": &page.image_hash,
        "page_kind": page.page_kind,
        "review_status": page.review_status,
        "average_confidence": page.average_confidence,
        "minimum_confidence": page.minimum_confidence,
    }))?);
    text.push_str(&format!("# {}\n\n", source_page_title(page)));
    if page.ocr_text.trim().is_empty() {
        text.push_str("_No OCR text recorded._\n");
    } else {
        text.push_str(page.ocr_text.trim());
        text.push('\n');
    }
    fs::write(pages_dir.join(source_page_file_name(page)), text)?;
    Ok(())
}

fn write_content_blocks(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    for cookbook in &catalogue.cookbooks {
        let slug = slug_file(&cookbook.title, &cookbook.id);
        let content_dir = workspace.join("cookbooks").join(slug).join("content");
        fs::create_dir_all(&content_dir)?;
        let blocks = content_blocks_for_cookbook(catalogue, &cookbook.id);
        write_content_index(&content_dir, cookbook, &blocks)?;
        for block in blocks {
            if block.kind == CookbookContentBlockKind::Recipe {
                continue;
            }
            write_content_block(&content_dir, block)?;
        }
    }
    Ok(())
}

fn write_content_index(
    content_dir: &Path,
    cookbook: &Cookbook,
    blocks: &[&CookbookContentBlock],
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("content_index", &cookbook.id));
    text.push_str(&format!("# {} Content\n\n", cookbook.title));
    for block in blocks
        .iter()
        .copied()
        .filter(|block| block.kind != CookbookContentBlockKind::Recipe)
    {
        text.push_str(&format!(
            "- [{}]({}) - {:?}",
            content_block_title(block),
            content_block_file_name(block),
            block.kind
        ));
        if let Some(page) = block.page_start {
            text.push_str(&format!(" - p. {page}"));
        }
        text.push('\n');
    }
    fs::write(content_dir.join("README.md"), text)?;
    Ok(())
}

fn write_content_block(
    content_dir: &Path,
    block: &CookbookContentBlock,
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&markdown_frontmatter(json!({
        "kind": "content_block",
        "id": &block.id,
        "cookbook_id": &block.cookbook_id,
        "section_id": &block.section_id,
        "block_kind": block.kind,
        "position": block.position,
        "page_start": block.page_start,
        "page_end": block.page_end,
        "confidence": block.confidence,
        "has_text": block.has_text,
    }))?);
    text.push_str(&format!("# {}\n\n", content_block_title(block)));
    if block.text.trim().is_empty() {
        text.push_str("_No interpreted text recorded._\n");
    } else {
        text.push_str(block.text.trim());
        text.push('\n');
    }
    let source_json = block.source_json.trim();
    if !source_json.is_empty() && source_json != "{}" {
        text.push_str("\n## Source JSON\n\n```json\n");
        text.push_str(source_json);
        text.push_str("\n```\n");
    }
    fs::write(content_dir.join(content_block_file_name(block)), text)?;
    Ok(())
}

fn write_all_recipes(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    let mut recipes = catalogue.recipes.iter().collect::<Vec<_>>();
    recipes.sort_by_key(|recipe| recipe.title.to_ascii_lowercase());

    let mut text = String::new();
    text.push_str(&metadata_header("index", "all-recipes"));
    text.push_str("# All Recipes\n\n");
    text.push_str("Canonical recipe files live in `recipes/`.\n\n");
    for recipe in recipes {
        text.push_str(&format!(
            "- [{}](recipes/{})",
            recipe.title,
            markdown_link_target(&recipe_file_name(recipe))
        ));
        if let Some(total) = recipe.total_minutes {
            text.push_str(&format!(" - {total}m"));
        }
        if let Some(category) = &recipe.category {
            text.push_str(&format!(" - {category}"));
        }
        if !recipe.tags.is_empty() {
            text.push_str(&format!(" - {}", recipe.tags.join(", ")));
        }
        text.push('\n');
    }
    fs::write(workspace.join("all-recipes.md"), text)?;
    Ok(())
}

fn write_recipe_collections(workspace: &Path, catalogue: &Catalogue) -> Result<(), Box<dyn Error>> {
    let mut by_tag: BTreeMap<String, Vec<&Recipe>> = BTreeMap::new();
    let mut by_category: BTreeMap<String, Vec<&Recipe>> = BTreeMap::new();
    let mut by_ingredient: BTreeMap<String, Vec<&Recipe>> = BTreeMap::new();
    let mut by_time: BTreeMap<String, Vec<&Recipe>> = BTreeMap::new();

    for recipe in &catalogue.recipes {
        for tag in &recipe.tags {
            by_tag.entry(tag.to_owned()).or_default().push(recipe);
        }
        if let Some(category) = &recipe.category {
            by_category
                .entry(category.to_owned())
                .or_default()
                .push(recipe);
        }
        for ingredient in &recipe.ingredients {
            let name = if ingredient.item.trim().is_empty() {
                ingredient.display_name.trim()
            } else {
                ingredient.item.trim()
            };
            if name.is_empty() {
                continue;
            }
            let recipes = by_ingredient.entry(name.to_owned()).or_default();
            if !recipes.iter().any(|candidate| candidate.id == recipe.id) {
                recipes.push(recipe);
            }
        }
        let time_bucket = match recipe.total_minutes {
            Some(minutes) if minutes <= 20 => "20-minutes-or-less",
            Some(minutes) if minutes <= 45 => "45-minutes-or-less",
            Some(minutes) if minutes <= 90 => "90-minutes-or-less",
            Some(_) => "slow-cooking",
            None => "time-unknown",
        };
        by_time
            .entry(time_bucket.to_owned())
            .or_default()
            .push(recipe);
    }

    write_group_root(workspace, "tags", "Tags", by_tag.keys())?;
    for (tag, recipes) in by_tag {
        write_recipe_group(workspace, "tags", &tag, "tag", recipes)?;
    }

    write_group_root(workspace, "categories", "Categories", by_category.keys())?;
    for (category, recipes) in by_category {
        write_recipe_group(workspace, "categories", &category, "category", recipes)?;
    }

    write_group_root(
        workspace,
        "ingredients",
        "Ingredients",
        by_ingredient.keys(),
    )?;
    for (ingredient, recipes) in by_ingredient {
        write_recipe_group(workspace, "ingredients", &ingredient, "ingredient", recipes)?;
    }

    write_group_root(workspace, "time", "Time", by_time.keys())?;
    for (bucket, recipes) in by_time {
        write_recipe_group(workspace, "time", &bucket, "time", recipes)?;
    }

    Ok(())
}

fn write_group_root<'a>(
    workspace: &Path,
    directory: &str,
    title: &str,
    names: impl Iterator<Item = &'a String>,
) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("index", directory));
    text.push_str(&format!("# {title}\n\n"));
    for name in names {
        text.push_str(&format!(
            "- [{}]({}/README.md)\n",
            name,
            slug_file(name, name)
        ));
    }
    fs::write(workspace.join(directory).join("README.md"), text)?;
    Ok(())
}

fn write_recipe_group(
    workspace: &Path,
    directory: &str,
    name: &str,
    kind: &str,
    mut recipes: Vec<&Recipe>,
) -> Result<(), Box<dyn Error>> {
    recipes.sort_by_key(|recipe| recipe.title.to_ascii_lowercase());
    let group_dir = workspace.join(directory).join(slug_file(name, name));
    fs::create_dir_all(&group_dir)?;

    let mut text = String::new();
    text.push_str(&metadata_header(kind, &slug_file(name, name)));
    text.push_str(&format!("# {name}\n\n"));
    text.push_str(&format!("{} recipes.\n\n", recipes.len()));
    for recipe in recipes {
        let file_name = recipe_file_name(recipe);
        link_recipe(workspace, &group_dir.join(&file_name), recipe)?;
        text.push_str(&format!(
            "- [{}]({})",
            recipe.title,
            markdown_link_target(&file_name)
        ));
        if let Some(total) = recipe.total_minutes {
            text.push_str(&format!(" - {total}m"));
        }
        text.push('\n');
    }
    fs::write(group_dir.join("README.md"), text)?;
    Ok(())
}

fn write_pantry(workspace: &Path, pantry: &[PantryItem]) -> Result<(), Box<dyn Error>> {
    let mut text = String::new();
    text.push_str(&metadata_header("pantry", "pantry"));
    text.push_str("# Pantry\n\n");
    if pantry.is_empty() {
        text.push_str("_No pantry items recorded._\n");
    } else {
        for item in pantry {
            text.push_str(&format!("- {}", item.display_name));
            if let Some(quantity) = item.quantity {
                text.push_str(&format!(" - {}", human_number(quantity)));
                if let Some(unit) = &item.unit {
                    text.push_str(&format!(" {unit}"));
                }
            }
            text.push_str(&format!(" - {:?}\n", item.category));
        }
    }
    fs::write(workspace.join("pantry.md"), text)?;
    Ok(())
}

fn write_meal_plan(
    workspace: &Path,
    meal_plan: &[MealPlanEntry],
    catalogue: &Catalogue,
) -> Result<(), Box<dyn Error>> {
    let recipe_by_id = recipe_lookup(catalogue);
    let mut text = String::new();
    text.push_str(&metadata_header("meal_plan", "meal-plan"));
    text.push_str("# Meal Plan\n\n");
    if meal_plan.is_empty() {
        text.push_str("_No meals planned._\n");
    } else {
        for entry in meal_plan {
            let title = recipe_by_id
                .get(entry.recipe_id.as_str())
                .map_or(entry.recipe_id.as_str(), |recipe| recipe.title.as_str());
            text.push_str(&format!(
                "- {} {:?}: `{}` - {}\n",
                entry.date, entry.meal_type, entry.recipe_id, title
            ));
        }
    }
    fs::write(workspace.join("meal-plan.md"), text)?;
    Ok(())
}

fn write_history(
    workspace: &Path,
    cook_log: &[CookLogEntry],
    catalogue: &Catalogue,
) -> Result<(), Box<dyn Error>> {
    let recipe_by_id = recipe_lookup(catalogue);
    let mut text = String::new();
    text.push_str(&metadata_header("cook_log", "history"));
    text.push_str("# Cook History\n\n");
    if cook_log.is_empty() {
        text.push_str("_No cook history recorded._\n");
    } else {
        for entry in cook_log {
            let title = recipe_by_id
                .get(entry.recipe_id.as_str())
                .map_or(entry.recipe_id.as_str(), |recipe| recipe.title.as_str());
            text.push_str(&format!(
                "- {}: `{}` - {}",
                entry.made_at, entry.recipe_id, title
            ));
            if let Some(notes) = &entry.notes {
                text.push_str(&format!(" - {notes}"));
            }
            text.push('\n');
        }
    }
    fs::write(workspace.join("history.md"), text)?;
    Ok(())
}

fn count_recipes(catalogue: &Catalogue, cookbook_id: &str) -> usize {
    catalogue
        .recipes
        .iter()
        .filter(|recipe| recipe.cookbook_id == cookbook_id)
        .count()
}

fn recipes_for_cookbook<'a>(catalogue: &'a Catalogue, cookbook_id: &str) -> Vec<&'a Recipe> {
    let mut recipes = catalogue
        .recipes
        .iter()
        .filter(|recipe| recipe.cookbook_id == cookbook_id)
        .collect::<Vec<_>>();
    recipes.sort_by_key(|recipe| recipe.title.to_ascii_lowercase());
    recipes
}

fn source_pages_for_cookbook<'a>(
    catalogue: &'a Catalogue,
    cookbook_id: &str,
) -> Vec<&'a CookbookPage> {
    let mut pages = catalogue
        .cookbook_pages
        .iter()
        .filter(|page| page.cookbook_id == cookbook_id)
        .collect::<Vec<_>>();
    pages.sort_by_key(|page| (page.image_index, page.printed_page_number));
    pages
}

fn content_blocks_for_cookbook<'a>(
    catalogue: &'a Catalogue,
    cookbook_id: &str,
) -> Vec<&'a CookbookContentBlock> {
    let mut blocks = catalogue
        .cookbook_content_blocks
        .iter()
        .filter(|block| block.cookbook_id == cookbook_id)
        .collect::<Vec<_>>();
    blocks.sort_by_key(|block| (block.page_start.unwrap_or(0), block.position));
    blocks
}

fn recipe_lookup(catalogue: &Catalogue) -> HashMap<&str, &Recipe> {
    catalogue
        .recipes
        .iter()
        .map(|recipe| (recipe.id.as_str(), recipe))
        .collect()
}

fn source_page_title(page: &CookbookPage) -> String {
    page.printed_page_label.as_ref().map_or_else(
        || format!("Source page {}", page.image_index),
        |label| format!("Source page {label}"),
    )
}

fn source_page_file_name(page: &CookbookPage) -> String {
    let title = page.printed_page_label.as_ref().map_or_else(
        || format!("page-{}", page.image_index),
        |label| format!("page-{label}"),
    );
    format!(
        "{:04}-{}.{}",
        page.image_index,
        slug_file(&title, &page.id),
        MARKDOWN_EXTENSION
    )
}

fn content_block_title(block: &CookbookContentBlock) -> String {
    block.title.as_ref().map_or_else(
        || format!("{:?} {}", block.kind, block.position),
        ToOwned::to_owned,
    )
}

fn content_block_file_name(block: &CookbookContentBlock) -> String {
    format!(
        "{:04}-{}.{}",
        block.position,
        slug_file(&content_block_title(block), &block.id),
        MARKDOWN_EXTENSION
    )
}

fn recipe_file_name(recipe: &Recipe) -> String {
    format!(
        "{}.{}",
        slug_file(&recipe.title, &recipe.id),
        COOK_EXTENSION
    )
}

fn markdown_link_target(path: &str) -> String {
    path.replace(' ', "%20")
}

fn link_recipe(workspace: &Path, link_path: &Path, recipe: &Recipe) -> Result<(), Box<dyn Error>> {
    let target = workspace.join("recipes").join(recipe_file_name(recipe));
    if fs::symlink_metadata(link_path).is_ok() {
        fs::remove_file(link_path)?;
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&target, link_path)
            .or_else(|_| fs::copy(&target, link_path).map(|_| ()))?;
    }

    #[cfg(not(unix))]
    {
        fs::copy(&target, link_path)?;
    }

    Ok(())
}

fn human_number(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        let mut formatted = format!("{value:.2}");
        while formatted.ends_with('0') {
            formatted.pop();
        }
        if formatted.ends_with('.') {
            formatted.pop();
        }
        formatted
    }
}

fn metadata_header(kind: &str, id: &str) -> String {
    format!(
        "---\n{{\n  \"schema\": {},\n  \"kind\": {},\n  \"id\": {}\n}}\n---\n\n",
        json_string(MARKDOWN_SCHEMA),
        json_string(kind),
        json_string(id)
    )
}

fn markdown_frontmatter(mut value: Value) -> Result<String, Box<dyn Error>> {
    if let Some(object) = value.as_object_mut() {
        object.insert("schema".to_owned(), json!(MARKDOWN_SCHEMA));
    }
    Ok(format!(
        "---\n{}\n---\n\n",
        serde_json::to_string_pretty(&value)?
    ))
}

fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn slug_file(title: &str, fallback: &str) -> String {
    let slug = slug_part(title);
    let fallback_slug = slug_part(fallback);
    if slug.len() >= 2 && (fallback_slug.len() < 2 || slug == fallback_slug) {
        slug
    } else if slug.len() >= 2 && fallback_slug.len() >= 2 {
        format!("{slug}-{fallback_slug}")
    } else if fallback_slug.len() >= 2 {
        fallback_slug
    } else {
        "item".to_owned()
    }
}

fn slug_part(value: &str) -> String {
    let mut slug = String::new();
    for character in value.chars().flat_map(char::to_lowercase) {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
        } else if matches!(character, ' ' | '-' | '_' | ':' | '/') && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_owned()
}

fn toml_escape(value: impl AsRef<OsStr>) -> String {
    value
        .as_ref()
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

#[derive(Debug)]
struct JsonBlock {
    kind: String,
    json: String,
    start_line: usize,
    end_line: usize,
}

fn find_json_block(text: &str) -> Option<JsonBlock> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim();
        if let Some(kind) = trimmed.strip_prefix("```json recitopia:") {
            let kind = kind.trim().to_owned();
            let start_line = index + 1;
            let mut end_line = start_line;
            while end_line < lines.len() && lines[end_line].trim() != "```" {
                end_line += 1;
            }
            let json = lines[start_line..end_line].join("\n");
            return Some(JsonBlock {
                kind,
                json,
                start_line,
                end_line,
            });
        }
        index += 1;
    }
    None
}

#[derive(Debug)]
struct FrontMatter {
    value: Value,
    start_line: usize,
    end_line: usize,
}

fn parse_frontmatter(text: &str) -> Option<Result<FrontMatter, String>> {
    let lines = text.lines().collect::<Vec<_>>();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return None;
    }
    for (index, line) in lines.iter().enumerate().skip(1) {
        if line.trim() == "---" {
            let raw = lines[1..index].join("\n");
            return Some(
                serde_json::from_str::<Value>(&raw)
                    .map(|value| FrontMatter {
                        value,
                        start_line: 1,
                        end_line: index,
                    })
                    .map_err(|error| format!("invalid Recitopia front matter JSON: {error}")),
            );
        }
    }
    Some(Err("missing closing front matter `---`".to_owned()))
}

fn frontmatter_schema(frontmatter: &FrontMatter) -> Option<&str> {
    frontmatter.value.get("schema")?.as_str()
}

fn frontmatter_identity(frontmatter: &FrontMatter) -> Option<(String, String)> {
    let kind = frontmatter.value.get("kind")?.as_str()?.to_owned();
    let id = frontmatter.value.get("id")?.as_str()?.to_owned();
    Some((kind, id))
}

fn find_recitopia_json_comment(text: &str) -> Option<JsonBlock> {
    let lines = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while index < lines.len() {
        if lines[index].trim() == "[- recitopia:json" {
            let start_line = index + 1;
            let mut end_line = start_line;
            while end_line < lines.len() && lines[end_line].trim() != "-]" {
                end_line += 1;
            }
            return Some(JsonBlock {
                kind: "recipe".to_owned(),
                json: lines[start_line..end_line].join("\n"),
                start_line,
                end_line,
            });
        }
        index += 1;
    }
    None
}

fn recipe_from_cook_document(text: &str) -> Result<Option<Recipe>, String> {
    let Some(frontmatter) = parse_frontmatter(text) else {
        return Ok(None);
    };
    let frontmatter = frontmatter?;
    if frontmatter_schema(&frontmatter) != Some(COOK_SCHEMA) {
        return Ok(None);
    }

    let Some(block) = find_recitopia_json_comment(text) else {
        return Err("missing `[- recitopia:json` block comment".to_owned());
    };
    let mut recipe = serde_json::from_str::<Recipe>(&block.json)
        .map_err(|error| format!("invalid embedded recipe JSON: {error}"))?;
    apply_cook_frontmatter(&mut recipe, &frontmatter.value);

    let ingredients = parse_cook_ingredients(text, &recipe.ingredients);
    if !ingredients.is_empty() {
        recipe.ingredients = ingredients;
    }
    let steps = parse_cook_steps(text, &recipe.steps);
    if !steps.is_empty() {
        recipe.steps = steps;
    }

    Ok(Some(recipe))
}

fn apply_cook_frontmatter(recipe: &mut Recipe, metadata: &Value) {
    if let Some(id) = metadata.get("id").and_then(Value::as_str) {
        recipe.id = id.to_owned();
    }
    if let Some(title) = metadata.get("title").and_then(Value::as_str) {
        recipe.title = title.to_owned();
    }
    recipe.subtitle = optional_string_field(metadata, "subtitle");
    if let Some(cookbook_id) = metadata
        .get("cookbook")
        .and_then(|value| value.get("id").or(Some(value)))
        .and_then(Value::as_str)
    {
        recipe.cookbook_id = cookbook_id.to_owned();
    }
    if let Some(source) = metadata.get("source") {
        if let Some(label) = source
            .get("label")
            .or_else(|| source.get("name"))
            .and_then(Value::as_str)
        {
            recipe.source_label = label.to_owned();
        }
        if let Some(pages) = source.get("pages").and_then(Value::as_array) {
            let page_numbers = pages
                .iter()
                .filter_map(|page| page.as_u64().and_then(|page| u32::try_from(page).ok()))
                .collect::<Vec<_>>();
            recipe.page_start = page_numbers.first().copied();
            recipe.page_end = page_numbers.last().copied();
        }
    }
    recipe.category = optional_string_field(metadata, "category");
    recipe.cuisine = optional_string_field(metadata, "cuisine");
    if let Some(tags) = metadata.get("tags").and_then(Value::as_array) {
        recipe.tags = tags
            .iter()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect();
    }
    if let Some(yield_value) = metadata.get("yield") {
        recipe.yield_quantity = yield_value.get("quantity").and_then(Value::as_f64);
        recipe.yield_unit = optional_string_field(yield_value, "unit");
    }
    if let Some(time) = metadata.get("time") {
        recipe.prep_minutes = optional_u32_field(time, "prep_minutes");
        recipe.cook_minutes = optional_u32_field(time, "cook_minutes");
        recipe.total_minutes = optional_u32_field(time, "total_minutes");
    }
}

fn optional_string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn optional_u32_field(value: &Value, key: &str) -> Option<u32> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CookSection {
    Other,
    Ingredients,
    Method,
}

fn parse_cook_ingredients(text: &str, base: &[Ingredient]) -> Vec<Ingredient> {
    let mut section = CookSection::Other;
    let mut pending_id = None;
    let mut ingredients = Vec::new();
    for line in text.lines() {
        if let Some(next_section) = cook_section(line) {
            section = next_section;
            continue;
        }
        if section != CookSection::Ingredients {
            continue;
        }
        let trimmed = line.trim();
        if let Some(id) = recitopia_directive_id(trimmed, "ingredient") {
            pending_id = Some(id);
            continue;
        }
        if !trimmed.starts_with('@') {
            continue;
        }
        let position = u32::try_from(ingredients.len() + 1).unwrap_or(u32::MAX);
        let mut ingredient = pending_id
            .as_deref()
            .and_then(|id| base.iter().find(|ingredient| ingredient.id == id))
            .cloned()
            .or_else(|| base.get(ingredients.len()).cloned())
            .unwrap_or_else(|| default_ingredient(position));
        let id = pending_id.take().unwrap_or_else(|| ingredient.id.clone());
        if trimmed == cook_ingredient_line(&ingredient) {
            ingredient.id = id;
            ingredient.position = Some(position);
            ingredients.push(ingredient);
            continue;
        }
        let Some(parsed) = parse_cook_ingredient(trimmed) else {
            continue;
        };
        ingredient.id = id;
        ingredient.position = Some(position);
        ingredient.item = parsed.name.clone();
        ingredient.display_name = parsed.display_name;
        ingredient.quantity = parsed.quantity;
        ingredient.quantity_text = parsed.quantity_text;
        ingredient.unit = parsed.unit;
        ingredient.preparation = parsed.preparation;
        ingredient.optional = parsed.optional || ingredient.optional;
        ingredients.push(ingredient);
    }
    ingredients
}

fn parse_cook_steps(text: &str, base: &[InstructionStep]) -> Vec<InstructionStep> {
    let mut section = CookSection::Other;
    let mut pending_id = None;
    let mut steps = Vec::new();
    let mut paragraph = Vec::new();

    for line in text.lines() {
        if let Some(next_section) = cook_section(line) {
            flush_cook_step(&mut steps, &mut paragraph, &mut pending_id, base);
            section = next_section;
            continue;
        }
        if section != CookSection::Method {
            continue;
        }
        let trimmed = line.trim();
        if let Some(id) = recitopia_directive_id(trimmed, "step") {
            flush_cook_step(&mut steps, &mut paragraph, &mut pending_id, base);
            pending_id = Some(id);
            continue;
        }
        if trimmed.is_empty() {
            flush_cook_step(&mut steps, &mut paragraph, &mut pending_id, base);
            continue;
        }
        if trimmed.starts_with("--") {
            continue;
        }
        paragraph.push(trimmed.to_owned());
    }
    flush_cook_step(&mut steps, &mut paragraph, &mut pending_id, base);
    steps
}

fn flush_cook_step(
    steps: &mut Vec<InstructionStep>,
    paragraph: &mut Vec<String>,
    pending_id: &mut Option<String>,
    base: &[InstructionStep],
) {
    if paragraph.is_empty() {
        return;
    }
    let position = u32::try_from(steps.len() + 1).unwrap_or(u32::MAX);
    let mut step = pending_id
        .as_deref()
        .and_then(|id| base.iter().find(|step| step.id == id))
        .cloned()
        .or_else(|| base.get(steps.len()).cloned())
        .unwrap_or_else(|| default_step(position));
    step.id = pending_id.take().unwrap_or_else(|| step.id.clone());
    step.position = position;
    step.text = paragraph.join(" ");
    paragraph.clear();
    steps.push(step);
}

fn cook_section(line: &str) -> Option<CookSection> {
    let trimmed = line.trim();
    let title = trimmed.strip_prefix('=')?.trim();
    if title.eq_ignore_ascii_case("ingredients") {
        Some(CookSection::Ingredients)
    } else if title.eq_ignore_ascii_case("method") || title.eq_ignore_ascii_case("instructions") {
        Some(CookSection::Method)
    } else {
        Some(CookSection::Other)
    }
}

fn recitopia_directive_id(line: &str, kind: &str) -> Option<String> {
    let rest = line.strip_prefix("-- recitopia:")?;
    let rest = rest.strip_prefix(kind)?.trim();
    rest.split_whitespace()
        .find_map(|token| token.strip_prefix("id=").map(ToOwned::to_owned))
}

#[derive(Debug)]
struct ParsedCookIngredient {
    name: String,
    display_name: String,
    quantity: Option<f64>,
    quantity_text: Option<String>,
    unit: Option<String>,
    preparation: Option<String>,
    optional: bool,
}

fn parse_cook_ingredient(line: &str) -> Option<ParsedCookIngredient> {
    let (line, optional) = strip_optional_comment(line);
    let body = line.strip_prefix('@')?;
    let quantity_start = body.find('{')?;
    let quantity_end = body[quantity_start + 1..].find('}')? + quantity_start + 1;
    let name = body[..quantity_start].trim().to_owned();
    if name.is_empty() {
        return None;
    }
    let quantity_raw = body[quantity_start + 1..quantity_end].trim();
    let rest = body[quantity_end + 1..].trim();
    let preparation = rest
        .strip_prefix('(')
        .and_then(|value| {
            value
                .split_once(')')
                .map(|(value, _)| value.trim().to_owned())
        })
        .filter(|value| !value.is_empty());
    let (quantity, quantity_text, unit) = parse_cook_quantity(quantity_raw);
    let display_name =
        ingredient_display_name(&name, quantity, quantity_text.as_deref(), unit.as_deref());
    Some(ParsedCookIngredient {
        name,
        display_name,
        quantity,
        quantity_text,
        unit,
        preparation,
        optional,
    })
}

fn strip_optional_comment(line: &str) -> (&str, bool) {
    if let Some((before, after)) = line.split_once("--") {
        (
            before.trim(),
            after.to_ascii_lowercase().contains("optional"),
        )
    } else {
        (line, false)
    }
}

fn parse_cook_quantity(raw: &str) -> (Option<f64>, Option<String>, Option<String>) {
    if raw.is_empty() {
        return (None, None, None);
    }
    let (amount, unit) = raw.split_once('%').map_or((raw, None), |(amount, unit)| {
        (amount.trim(), Some(unit.trim().to_owned()))
    });
    let parsed = amount.parse::<f64>().ok();
    let quantity_text = parsed.is_none().then(|| amount.to_owned());
    (parsed, quantity_text, unit.filter(|unit| !unit.is_empty()))
}

fn ingredient_display_name(
    name: &str,
    quantity: Option<f64>,
    quantity_text: Option<&str>,
    unit: Option<&str>,
) -> String {
    let amount = quantity
        .map(human_number)
        .or_else(|| quantity_text.map(ToOwned::to_owned));
    amount.map_or_else(
        || name.to_owned(),
        |amount| {
            unit.map_or_else(
                || format!("{amount} {name}"),
                |unit| format!("{amount} {unit} {name}"),
            )
        },
    )
}

fn default_ingredient(position: u32) -> Ingredient {
    Ingredient {
        id: format!("ingredient-{position}"),
        position: Some(position),
        display_name: String::new(),
        item: String::new(),
        quantity: None,
        quantity_text: None,
        quantity_min: None,
        quantity_max: None,
        quantity_kind: IngredientQuantityKind::Exact,
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
        estimated_cost_cents: None,
    }
}

fn default_step(position: u32) -> InstructionStep {
    InstructionStep {
        id: format!("step-{position}"),
        position,
        section: None,
        text: String::new(),
        source_page_id: None,
        source_line_start: None,
        source_line_end: None,
    }
}

fn validate_document(text: &str) -> (Vec<Diagnostic>, Option<Recipe>) {
    if let Some(frontmatter) = parse_frontmatter(text) {
        match frontmatter {
            Ok(frontmatter) if frontmatter_schema(&frontmatter) == Some(COOK_SCHEMA) => {
                return match recipe_from_cook_document(text) {
                    Ok(recipe) => (Vec::new(), recipe),
                    Err(error) => (
                        vec![Diagnostic::new(frontmatter_range(&frontmatter), 1, error)],
                        None,
                    ),
                };
            }
            Ok(_) => return (Vec::new(), None),
            Err(error) => {
                return (vec![Diagnostic::error(0, 0, 0, 1, error)], None);
            }
        }
    }

    let Some(block) = find_json_block(text) else {
        if parse_metadata(text).is_some_and(|(kind, _)| prose_kind_allows_no_json(&kind)) {
            return (Vec::new(), None);
        }
        return (Vec::new(), None);
    };

    let range = block_range(&block);
    match block.kind.as_str() {
        "recipe" => match serde_json::from_str::<Recipe>(&block.json) {
            Ok(recipe) => (Vec::new(), Some(recipe)),
            Err(error) => (
                vec![Diagnostic::new(
                    range,
                    1,
                    format!("invalid recipe JSON: {error}"),
                )],
                None,
            ),
        },
        _ => match serde_json::from_str::<Value>(&block.json) {
            Ok(_) => (Vec::new(), None),
            Err(error) => (
                vec![Diagnostic::new(range, 1, format!("invalid JSON: {error}"))],
                None,
            ),
        },
    }
}

fn frontmatter_range(frontmatter: &FrontMatter) -> Range {
    Range {
        start: Position {
            line: u32::try_from(frontmatter.start_line).unwrap_or(u32::MAX),
            character: 0,
        },
        end: Position {
            line: u32::try_from(frontmatter.end_line).unwrap_or(u32::MAX),
            character: 0,
        },
    }
}

fn prose_kind_allows_no_json(kind: &str) -> bool {
    matches!(
        kind,
        "catalogue"
            | "category"
            | "cook_log"
            | "cookbook"
            | "index"
            | "ingredient"
            | "meal_plan"
            | "pantry"
            | "recipe"
            | "tag"
            | "time"
    )
}

fn block_range(block: &JsonBlock) -> Range {
    Range {
        start: Position {
            line: u32::try_from(block.start_line).unwrap_or(u32::MAX),
            character: 0,
        },
        end: Position {
            line: u32::try_from(block.end_line.max(block.start_line)).unwrap_or(u32::MAX),
            character: 0,
        },
    }
}

struct LspServer {
    api: ApiClient,
    documents: HashMap<String, String>,
    id_to_uri: HashMap<String, String>,
    root: Option<PathBuf>,
    runtime: tokio::runtime::Runtime,
    shutdown_requested: bool,
}

impl LspServer {
    fn new(args: LspArgs) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            api: ApiClient::new(&args.api_url),
            documents: HashMap::new(),
            id_to_uri: HashMap::new(),
            root: None,
            runtime: tokio::runtime::Runtime::new()?,
            shutdown_requested: false,
        })
    }

    fn run(&mut self) -> Result<(), Box<dyn Error>> {
        let stdin = io::stdin();
        let mut reader = io::BufReader::new(stdin.lock());
        let stdout = io::stdout();
        let mut writer = stdout.lock();

        while let Some(message) = read_lsp_message(&mut reader)? {
            if let Some(response) = self.handle_message(message)? {
                write_lsp_message(&mut writer, &response)?;
            }
            if self.shutdown_requested {
                break;
            }
        }
        Ok(())
    }

    fn handle_message(&mut self, message: Value) -> Result<Option<Value>, Box<dyn Error>> {
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = message.get("id").cloned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);

        match method {
            "initialize" => {
                self.root = root_path_from_initialize(&params);
                self.scan_workspace();
                Ok(id.map(|id| response(id, initialize_result())))
            }
            "initialized" => Ok(None),
            "shutdown" => Ok(id.map(|id| response(id, Value::Null))),
            "exit" => {
                self.shutdown_requested = true;
                Ok(None)
            }
            "textDocument/didOpen" => {
                self.did_open(&params)?;
                Ok(None)
            }
            "textDocument/didChange" => {
                self.did_change(&params)?;
                Ok(None)
            }
            "textDocument/didSave" => {
                self.did_save(&params)?;
                Ok(None)
            }
            "textDocument/documentSymbol" => {
                Ok(id.map(|id| response(id, self.document_symbols(&params))))
            }
            "textDocument/hover" => Ok(id.map(|id| response(id, self.hover(&params)))),
            "textDocument/definition" => Ok(id.map(|id| response(id, self.definition(&params)))),
            _ => Ok(id.map(|id| response(id, Value::Null))),
        }
    }

    fn did_open(&mut self, params: &Value) -> Result<(), Box<dyn Error>> {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let text = params["textDocument"]["text"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        self.documents.insert(uri.to_owned(), text.clone());
        self.index_document(uri, &text);
        self.publish_diagnostics(uri, validate_document(&text).0)
    }

    fn did_change(&mut self, params: &Value) -> Result<(), Box<dyn Error>> {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let text = params["contentChanges"]
            .as_array()
            .and_then(|changes| changes.last())
            .and_then(|change| change["text"].as_str())
            .unwrap_or_default()
            .to_owned();
        self.documents.insert(uri.to_owned(), text.clone());
        self.index_document(uri, &text);
        self.publish_diagnostics(uri, validate_document(&text).0)
    }

    fn did_save(&mut self, params: &Value) -> Result<(), Box<dyn Error>> {
        let uri = params["textDocument"]["uri"].as_str().unwrap_or_default();
        let text = params.get("text").and_then(Value::as_str).map_or_else(
            || self.documents.get(uri).cloned().unwrap_or_default(),
            ToOwned::to_owned,
        );
        let (mut diagnostics, recipe) = validate_document(&text);
        if diagnostics.is_empty() {
            if let Some(recipe) = recipe {
                match self.runtime.block_on(self.api.put_recipe(&recipe)) {
                    Ok(updated) => {
                        self.show_message(&format!("Saved recipe: {}", updated.title))?;
                    }
                    Err(error) => diagnostics.push(Diagnostic::error(
                        0,
                        0,
                        0,
                        1,
                        format!("recipe save failed: {error}"),
                    )),
                }
            }
        }
        self.publish_diagnostics(uri, diagnostics)
    }

    fn document_symbols(&self, params: &Value) -> Value {
        let Some(uri) = document_uri(params) else {
            return json!([]);
        };
        let Some(text) = self.documents.get(uri) else {
            return json!([]);
        };
        let mut symbols = Vec::new();
        for (line_index, line) in text.lines().enumerate() {
            let (_level, name) = document_symbol_heading(line);
            if name.is_empty() {
                continue;
            }
            let range = Range::line(line_index);
            symbols.push(json!({
                "name": name,
                "kind": 13,
                "range": range,
                "selectionRange": range,
            }));
        }
        json!(symbols)
    }

    fn hover(&self, params: &Value) -> Value {
        let Some((uri, position)) = text_position(params) else {
            return Value::Null;
        };
        let Some(word) = self.word_at(uri, position) else {
            return Value::Null;
        };
        if let Some(target) = self.id_to_uri.get(&word) {
            return json!({
                "contents": {
                    "kind": "markdown",
                    "value": format!("Recitopia ID `{word}`\n\nDefined in `{}`.", display_uri(target)),
                }
            });
        }
        Value::Null
    }

    fn definition(&self, params: &Value) -> Value {
        let Some((uri, position)) = text_position(params) else {
            return Value::Null;
        };
        let Some(word) = self.word_at(uri, position) else {
            return Value::Null;
        };
        let Some(target_uri) = self.id_to_uri.get(&word) else {
            return Value::Null;
        };
        json!([{
            "uri": target_uri,
            "range": Range::line(0),
        }])
    }

    fn publish_diagnostics(
        &self,
        uri: &str,
        diagnostics: Vec<Diagnostic>,
    ) -> Result<(), Box<dyn Error>> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": uri,
                "diagnostics": diagnostics,
            }
        });
        write_lsp_message(&mut io::stdout().lock(), &message)
    }

    fn show_message(&self, text: &str) -> Result<(), Box<dyn Error>> {
        let message = json!({
            "jsonrpc": "2.0",
            "method": "window/showMessage",
            "params": {
                "type": 3,
                "message": text,
            }
        });
        write_lsp_message(&mut io::stdout().lock(), &message)
    }

    fn scan_workspace(&mut self) {
        let Some(root) = self.root.clone() else {
            return;
        };
        self.id_to_uri.clear();
        self.scan_directory(&root.join("recipes"));
        self.scan_directory(&root.join("cookbooks"));
        self.scan_directory(&root);
    }

    fn scan_directory(&mut self, directory: &Path) {
        let Ok(entries) = fs::read_dir(directory) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                self.scan_directory(&path);
                continue;
            }
            if fs::symlink_metadata(&path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
                continue;
            }
            if !is_recitopia_file(&path) {
                continue;
            }
            if let Ok(text) = fs::read_to_string(&path) {
                let uri = file_uri(&path);
                self.index_document(&uri, &text);
            }
        }
    }

    fn index_document(&mut self, uri: &str, text: &str) {
        if let Some((_, id)) = parse_metadata(text) {
            self.id_to_uri.entry(id).or_insert_with(|| uri.to_owned());
        }
        if let (_, Some(recipe)) = validate_document(text) {
            self.id_to_uri
                .entry(recipe.id.clone())
                .or_insert_with(|| uri.to_owned());
        }
    }

    fn word_at(&self, uri: &str, position: Position) -> Option<String> {
        let text = self.documents.get(uri)?;
        let line = text.lines().nth(usize::try_from(position.line).ok()?)?;
        let character = usize::try_from(position.character).ok()?;
        let mut start = character.min(line.len());
        while start > 0 && is_id_byte(line.as_bytes()[start - 1]) {
            start -= 1;
        }
        let mut end = character.min(line.len());
        while end < line.len() && is_id_byte(line.as_bytes()[end]) {
            end += 1;
        }
        (start < end).then(|| line[start..end].to_owned())
    }
}

fn initialize_result() -> Value {
    json!({
        "capabilities": {
            "textDocumentSync": {
                "openClose": true,
                "change": 1,
                "save": { "includeText": true }
            },
            "documentSymbolProvider": true,
            "hoverProvider": true,
            "definitionProvider": true
        },
        "serverInfo": {
            "name": "recitopia-hx",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

fn response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn read_lsp_message(reader: &mut impl BufRead) -> Result<Option<Value>, Box<dyn Error>> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            content_length = Some(value.trim().parse::<usize>()?);
        }
    }

    let Some(length) = content_length else {
        return Ok(None);
    };
    let mut body = vec![0_u8; length];
    reader.read_exact(&mut body)?;
    Ok(Some(serde_json::from_slice(&body)?))
}

fn write_lsp_message(writer: &mut impl Write, message: &Value) -> Result<(), Box<dyn Error>> {
    let body = serde_json::to_vec(message)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct Position {
    line: u32,
    character: u32,
}

#[derive(Clone, Copy, Debug, Serialize)]
struct Range {
    start: Position,
    end: Position,
}

impl Range {
    fn line(line: usize) -> Self {
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        Self {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: u32::MAX,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct Diagnostic {
    range: Range,
    severity: u8,
    source: &'static str,
    message: String,
}

impl Diagnostic {
    fn new(range: Range, severity: u8, message: impl Into<String>) -> Self {
        Self {
            range,
            severity,
            source: "recitopia",
            message: message.into(),
        }
    }

    fn error(
        start_line: usize,
        start_character: usize,
        end_line: usize,
        end_character: usize,
        message: impl Into<String>,
    ) -> Self {
        Self::new(
            Range {
                start: Position {
                    line: u32::try_from(start_line).unwrap_or(u32::MAX),
                    character: u32::try_from(start_character).unwrap_or(u32::MAX),
                },
                end: Position {
                    line: u32::try_from(end_line).unwrap_or(u32::MAX),
                    character: u32::try_from(end_character).unwrap_or(u32::MAX),
                },
            },
            1,
            message,
        )
    }
}

fn root_path_from_initialize(params: &Value) -> Option<PathBuf> {
    params
        .get("rootUri")
        .and_then(Value::as_str)
        .and_then(path_from_file_uri)
        .or_else(|| {
            params
                .get("rootPath")
                .and_then(Value::as_str)
                .map(PathBuf::from)
        })
}

fn document_uri(params: &Value) -> Option<&str> {
    params.get("textDocument")?.get("uri")?.as_str()
}

fn text_position(params: &Value) -> Option<(&str, Position)> {
    let uri = document_uri(params)?;
    let position = serde_json::from_value::<Position>(params.get("position")?.clone()).ok()?;
    Some((uri, position))
}

fn file_uri(path: &Path) -> String {
    format!("file://{}", percent_encode(&path.to_string_lossy()))
}

fn path_from_file_uri(uri: &str) -> Option<PathBuf> {
    let path = uri.strip_prefix("file://")?;
    Some(PathBuf::from(percent_decode(path)))
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn percent_decode(value: &str) -> String {
    let mut decoded = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[index + 1..index + 3]) {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    decoded.push(byte);
                    index += 3;
                    continue;
                }
            }
        }
        decoded.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn display_uri(uri: &str) -> String {
    path_from_file_uri(uri).map_or_else(|| uri.to_owned(), |path| path.display().to_string())
}

fn document_symbol_heading(line: &str) -> (usize, &str) {
    let markdown_level = line
        .chars()
        .take_while(|character| *character == '#')
        .count();
    if markdown_level > 0 {
        return (markdown_level, line[markdown_level..].trim());
    }
    let cook_level = line
        .chars()
        .take_while(|character| *character == '=')
        .count();
    if cook_level > 0 {
        return (cook_level, line[cook_level..].trim());
    }
    (0, "")
}

fn is_id_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

fn is_recitopia_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| matches!(extension, COOK_EXTENSION | MARKDOWN_EXTENSION))
}

fn parse_metadata(text: &str) -> Option<(String, String)> {
    if let Some(Ok(frontmatter)) = parse_frontmatter(text) {
        if let Some(identity) = frontmatter_identity(&frontmatter) {
            return Some(identity);
        }
    }
    let first_line = text.lines().next()?.trim();
    let inner = first_line
        .strip_prefix("<!-- recitopia:")?
        .strip_suffix("-->")?
        .trim();
    let mut kind = None;
    let mut id = None;
    for token in inner.split_whitespace() {
        if let Some(value) = token.strip_prefix("kind=") {
            kind = Some(value.to_owned());
        } else if let Some(value) = token.strip_prefix("id=") {
            id = Some(value.to_owned());
        }
    }
    Some((kind?, id?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use recitopia_api_rs::model::{RecipeExtractionStatus, ShareScope};

    #[test]
    fn extracts_recitopia_json_block() {
        let text = "# Title\n\n```json recitopia:recipe\n{\"id\":\"recipe-1\"}\n```\n";
        let block = find_json_block(text).expect("json block");
        assert_eq!(block.kind, "recipe");
        assert_eq!(block.json, "{\"id\":\"recipe-1\"}");
        assert_eq!(block.start_line, 3);
    }

    #[test]
    fn parses_metadata_header() {
        assert_eq!(
            parse_metadata(&metadata_header("recipe", "recipe-1")),
            Some(("recipe".to_owned(), "recipe-1".to_owned()))
        );
    }

    #[test]
    fn index_document_does_not_require_json_block() {
        let text = format!(
            "{}# Recitopia\n\n- `recipe-1`\n",
            metadata_header("index", "recitopia")
        );
        let (diagnostics, recipe) = validate_document(&text);
        assert!(diagnostics.is_empty());
        assert!(recipe.is_none());
    }

    #[test]
    fn prose_recipe_does_not_require_json_block() {
        let text = format!(
            "{}# Recipe\n\n## Ingredients\n",
            metadata_header("recipe", "recipe-1")
        );
        let (diagnostics, recipe) = validate_document(&text);
        assert!(diagnostics.is_empty());
        assert!(recipe.is_none());
    }

    #[test]
    fn file_uri_round_trips_spaces() {
        let path = Path::new("/tmp/recitopia hx/file.cook");
        let uri = file_uri(path);
        assert_eq!(path_from_file_uri(&uri).as_deref(), Some(path));
    }

    #[test]
    fn slug_file_avoids_duplicate_suffixes_and_raw_spaces() {
        assert_eq!(slug_file("East", "east"), "east");
        assert_eq!(
            slug_file(
                "Persian (pickling) cucumbers",
                "Persian (pickling) cucumbers"
            ),
            "persian-pickling-cucumbers"
        );
        assert_eq!(
            slug_file("Warming Chicken & Potato Stew", "import-id-recipe-1"),
            "warming-chicken-potato-stew-import-id-recipe-1"
        );
    }

    #[test]
    fn cook_document_derives_recipe_edits() {
        let recipe = sample_recipe();
        let catalogue = Catalogue {
            cookbooks: vec![sample_cookbook()],
            recipes: vec![recipe.clone()],
            ..Catalogue::default()
        };
        let directory = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(directory.path().join("recipes")).expect("recipes dir");
        write_recipe(directory.path(), &catalogue, &recipe).expect("write recipe");

        let path = directory
            .path()
            .join("recipes")
            .join(recipe_file_name(&recipe));
        let text = fs::read_to_string(path).expect("read recipe");
        let edited = text
            .replacen(
                "\"title\": \"Test Soup\"",
                "\"title\": \"Better Test Soup\"",
                1,
            )
            .replace("@salt{1%tsp}", "@salt{2%tsp}")
            .replace("Stir the soup.", "Stir the soup gently.");

        let parsed = recipe_from_cook_document(&edited)
            .expect("parse cook")
            .expect("recipe");
        assert_eq!(parsed.title, "Better Test Soup");
        assert_eq!(parsed.ingredients[0].id, "ingredient-1");
        assert_eq!(parsed.ingredients[0].quantity, Some(2.0));
        assert_eq!(parsed.ingredients[0].unit.as_deref(), Some("tsp"));
        assert_eq!(parsed.steps[0].id, "step-1");
        assert_eq!(parsed.steps[0].text, "Stir the soup gently.");
    }

    #[test]
    fn unchanged_cook_document_preserves_embedded_ingredient_projection() {
        let mut recipe = sample_recipe();
        recipe.ingredients[0].quantity = Some(3.0);
        recipe.ingredients[0].quantity_text = Some("3 or 1".to_owned());
        recipe.ingredients[0].display_name = "3 cucumbers or 1 regular cucumber".to_owned();
        recipe.ingredients[0].item = "cucumber".to_owned();
        recipe.ingredients[0].unit = None;
        let catalogue = Catalogue {
            cookbooks: vec![sample_cookbook()],
            recipes: vec![recipe.clone()],
            ..Catalogue::default()
        };
        let directory = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(directory.path().join("recipes")).expect("recipes dir");
        write_recipe(directory.path(), &catalogue, &recipe).expect("write recipe");

        let path = directory
            .path()
            .join("recipes")
            .join(recipe_file_name(&recipe));
        let text = fs::read_to_string(path).expect("read recipe");
        let parsed = recipe_from_cook_document(&text)
            .expect("parse cook")
            .expect("recipe");
        assert_eq!(parsed.ingredients[0], recipe.ingredients[0]);
    }

    #[test]
    fn cook_ingredient_line_preserves_quantity_text_units() {
        let mut recipe = sample_recipe();
        let mut ingredient = recipe.ingredients.remove(0);
        ingredient.quantity = Some(1.0);
        ingredient.quantity_text = Some("1".to_owned());
        ingredient.unit = Some("tsp".to_owned());
        assert_eq!(cook_ingredient_line(&ingredient), "@salt{1%tsp}");
    }

    fn sample_cookbook() -> Cookbook {
        Cookbook {
            id: "test-book".to_owned(),
            title: "Test Book".to_owned(),
            author_ids: Vec::new(),
            isbn: None,
            publisher: None,
            published_year: None,
            cover_image_url: None,
            owner_user_id: None,
            family_id: None,
            share_scope: ShareScope::default(),
            shared_with_user_ids: Vec::new(),
        }
    }

    fn sample_recipe() -> Recipe {
        Recipe {
            id: "test-soup".to_owned(),
            title: "Test Soup".to_owned(),
            subtitle: None,
            alternate_names: Vec::new(),
            cookbook_id: "test-book".to_owned(),
            author_ids: Vec::new(),
            page_start: Some(10),
            page_end: Some(11),
            source_label: "Test Book, pp. 10-11".to_owned(),
            headnote: Some("A tiny soup.".to_owned()),
            serving_context: None,
            yield_quantity: Some(2.0),
            yield_unit: Some("servings".to_owned()),
            prep_minutes: Some(5),
            cook_minutes: Some(10),
            total_minutes: Some(15),
            cuisine: None,
            category: Some("Soup".to_owned()),
            tags: vec!["quick".to_owned()],
            searchable_text: String::new(),
            source_block_id: None,
            source_page_spans: Vec::new(),
            component_recipe_ids: Vec::new(),
            pictured_page_number: None,
            extraction_status: RecipeExtractionStatus::default(),
            images: Vec::new(),
            ingredients: vec![Ingredient {
                id: "ingredient-1".to_owned(),
                position: Some(1),
                display_name: "1 tsp salt".to_owned(),
                item: "salt".to_owned(),
                quantity: Some(1.0),
                quantity_text: None,
                quantity_min: None,
                quantity_max: None,
                quantity_kind: IngredientQuantityKind::Exact,
                quantity_review_status: IngredientQuantityReviewStatus::Parsed,
                quantity_review_reason: None,
                unit: Some("tsp".to_owned()),
                preparation: None,
                section: None,
                optional: false,
                alternative_text: None,
                source_line: None,
                source_page_id: None,
                unit_cost_cents: None,
                estimated_cost_cents: None,
            }],
            steps: vec![InstructionStep {
                id: "step-1".to_owned(),
                position: 1,
                section: None,
                text: "Stir the soup.".to_owned(),
                source_page_id: None,
                source_line_start: None,
                source_line_end: None,
            }],
            notes: Vec::new(),
            last_made_at: None,
            times_made: 0,
            cost_cents: None,
            cost_per_serving_cents: None,
            cache_key: String::new(),
            cache_updated_at: None,
        }
    }
}
