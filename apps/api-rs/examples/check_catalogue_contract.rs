use std::{collections::BTreeSet, env, error::Error, fs, io};

use recitopia_api_rs::model::Catalogue;

fn main() -> Result<(), Box<dyn Error>> {
    let fixture_path = env::args_os().nth(1).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: cargo run --example check_catalogue_contract -- <catalogue.json>",
        )
    })?;
    let fixture_bytes = fs::read(&fixture_path)?;
    let expected = serde_json::from_slice::<serde_json::Value>(&fixture_bytes)?;
    let catalogue = serde_json::from_value::<Catalogue>(expected.clone())?;
    let actual = serde_json::to_value(&catalogue)?;

    for recipe in &catalogue.recipes {
        let recomputed = recipe.clone().recompute("contract-check")?;
        if recomputed.cache_key != recipe.cache_key {
            return Err(io::Error::other(format!(
                "cache-key mismatch for {}: Zig {}, Rust {}",
                recipe.id, recipe.cache_key, recomputed.cache_key
            ))
            .into());
        }
    }

    if actual != expected {
        let mut differences = Vec::new();
        collect_difference_paths(&expected, &actual, "$", &mut differences);
        for difference in &differences {
            eprintln!("contract mismatch: {difference}");
        }
        return Err(io::Error::other(
            "Rust catalogue serialization does not match the input contract",
        )
        .into());
    }

    println!(
        "catalogue contract valid: {} cookbooks, {} recipes with matching cache keys, {} pages, {} blocks",
        catalogue.cookbooks.len(),
        catalogue.recipes.len(),
        catalogue.cookbook_pages.len(),
        catalogue.cookbook_content_blocks.len()
    );
    Ok(())
}

fn collect_difference_paths(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: &str,
    differences: &mut Vec<String>,
) {
    const MAX_DIFFERENCES: usize = 20;
    if differences.len() >= MAX_DIFFERENCES {
        return;
    }

    match (expected, actual) {
        (serde_json::Value::Object(expected), serde_json::Value::Object(actual)) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (expected.get(key), actual.get(key)) {
                    (Some(expected), Some(actual)) => {
                        collect_difference_paths(expected, actual, &child_path, differences);
                    }
                    (Some(_), None) => differences.push(format!("{child_path} is missing")),
                    (None, Some(_)) => differences.push(format!("{child_path} is unexpected")),
                    (None, None) => {}
                }
                if differences.len() >= MAX_DIFFERENCES {
                    break;
                }
            }
        }
        (serde_json::Value::Array(expected), serde_json::Value::Array(actual)) => {
            if expected.len() != actual.len() {
                differences.push(format!(
                    "{path} length differs: expected {}, actual {}",
                    expected.len(),
                    actual.len()
                ));
            }
            for (index, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                collect_difference_paths(
                    expected,
                    actual,
                    &format!("{path}[{index}]"),
                    differences,
                );
                if differences.len() >= MAX_DIFFERENCES {
                    break;
                }
            }
        }
        _ if expected != actual => differences.push(format!(
            "{path} differs: expected {expected}, actual {actual}"
        )),
        _ => {}
    }
}
