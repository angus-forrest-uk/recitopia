use std::{
    env,
    net::{AddrParseError, IpAddr, SocketAddr},
    num::ParseIntError,
    path::{Path, PathBuf},
};

use thiserror::Error;

pub const DEFAULT_RUST_API_PORT: u16 = 8079;
const DEFAULT_API_HOST: &str = "127.0.0.1";
const DEFAULT_DATABASE_PATH: &str = "../../data/recitopia.duckdb";
const DEFAULT_IMPORT_DIRECTORY: &str = "../../data/imports";
const DEFAULT_OCR_SERVER_URL: &str = "http://127.0.0.1:8078";
const DEFAULT_OCR_SCRIPT: &str = "../../tools/ocr/paddle_ocr.py";
const DEFAULT_RECITOPIA_LLM_COOKBOOK_SCRIPT: &str = "../../tools/ml/llm_cookbook_mapper.py";
const DEFAULT_LLM_RECIPE_SCRIPT: &str = "../../tools/ml/llm_mapper.py";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreMode {
    ReadOnly,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DatabaseConfig {
    pub path: PathBuf,
    pub mode: StoreMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetConfig {
    pub import_dir: PathBuf,
    pub image_convert_bin: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PipelineConfig {
    pub ocr_server_url: String,
    pub ocr_python: PathBuf,
    pub ocr_script: PathBuf,
    pub llm_python: PathBuf,
    pub llm_cookbook_script: PathBuf,
    pub llm_recipe_script: PathBuf,
    pub ocr_batch_page_limit: usize,
    pub concurrency: usize,
}

impl DatabaseConfig {
    #[must_use]
    pub fn is_memory(&self) -> bool {
        self.path == Path::new(":memory:")
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub host: IpAddr,
    pub port: u16,
    pub database: DatabaseConfig,
    pub assets: AssetConfig,
    pub pipeline: PipelineConfig,
}

impl Config {
    /// Loads configuration from the process environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a configured host, port, or store mode is invalid.
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    /// Loads configuration through a caller-provided environment lookup.
    ///
    /// Rust-specific variables take precedence over their Zig-compatible counterparts.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when a configured host, port, or store mode is invalid.
    pub fn from_lookup<F>(mut lookup: F) -> Result<Self, ConfigError>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let host_value = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_API_HOST", "RECITOPIA_API_HOST"],
        )
        .unwrap_or_else(|| DEFAULT_API_HOST.to_owned());
        let host = host_value
            .parse()
            .map_err(|source| ConfigError::InvalidHost {
                value: host_value,
                source,
            })?;

        let port_value = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_API_PORT", "RECITOPIA_API_PORT"],
        );
        let port = match port_value {
            Some(value) => value
                .parse()
                .map_err(|source| ConfigError::InvalidPort { value, source })?,
            None => DEFAULT_RUST_API_PORT,
        };

        let database_path = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_DB_PATH", "RECITOPIA_DB_PATH"],
        )
        .unwrap_or_else(|| DEFAULT_DATABASE_PATH.to_owned());
        let mode_value = first_value(&mut lookup, &["RECITOPIA_RUST_STORE_MODE"])
            .unwrap_or_else(|| "read-only".to_owned());
        let mode = parse_store_mode(&mode_value)?;
        let import_dir = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_IMPORT_DIR", "RECITOPIA_IMPORT_DIR"],
        )
        .unwrap_or_else(|| DEFAULT_IMPORT_DIRECTORY.to_owned());
        let image_convert_bin = first_value(
            &mut lookup,
            &[
                "RECITOPIA_RUST_IMAGE_CONVERT_BIN",
                "RECITOPIA_IMAGE_CONVERT_BIN",
            ],
        )
        .map(PathBuf::from);
        let ocr_server_url = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_OCR_SERVER_URL", "RECITOPIA_OCR_SERVER_URL"],
        )
        .unwrap_or_else(|| DEFAULT_OCR_SERVER_URL.to_owned());
        let ocr_python = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_OCR_PYTHON", "RECITOPIA_OCR_PYTHON"],
        )
        .unwrap_or_else(|| "python3".to_owned());
        let ocr_script = first_value(
            &mut lookup,
            &["RECITOPIA_RUST_OCR_SCRIPT", "RECITOPIA_OCR_SCRIPT"],
        )
        .unwrap_or_else(|| DEFAULT_OCR_SCRIPT.to_owned());
        let llm_python = first_value(
            &mut lookup,
            &[
                "RECITOPIA_RUST_LLM_PYTHON",
                "RECITOPIA_LLM_PYTHON",
            ],
        )
        .unwrap_or_else(|| "python3".to_owned());
        let llm_cookbook_script = first_value(
            &mut lookup,
            &[
                "RECITOPIA_RUST_LLM_COOKBOOK_SCRIPT",
                "RECITOPIA_LLM_COOKBOOK_SCRIPT",
            ],
        )
        .unwrap_or_else(|| DEFAULT_RECITOPIA_LLM_COOKBOOK_SCRIPT.to_owned());
        let llm_recipe_script = first_value(
            &mut lookup,
            &[
                "RECITOPIA_RUST_LLM_RECIPE_SCRIPT",
                "RECITOPIA_LLM_SCRIPT",
            ],
        )
        .unwrap_or_else(|| DEFAULT_LLM_RECIPE_SCRIPT.to_owned());
        let ocr_batch_page_limit = positive_usize(
            "RECITOPIA_OCR_BATCH_PAGE_LIMIT",
            first_value(
                &mut lookup,
                &[
                    "RECITOPIA_RUST_OCR_BATCH_PAGE_LIMIT",
                    "RECITOPIA_OCR_BATCH_PAGE_LIMIT",
                ],
            ),
            8,
            32,
        )?;
        let concurrency = positive_usize(
            "RECITOPIA_RUST_PIPELINE_CONCURRENCY",
            first_value(&mut lookup, &["RECITOPIA_RUST_PIPELINE_CONCURRENCY"]),
            2,
            16,
        )?;

        Ok(Self {
            host,
            port,
            database: DatabaseConfig {
                path: PathBuf::from(database_path),
                mode,
            },
            assets: AssetConfig {
                import_dir: PathBuf::from(import_dir),
                image_convert_bin,
            },
            pipeline: PipelineConfig {
                ocr_server_url,
                ocr_python: PathBuf::from(ocr_python),
                ocr_script: PathBuf::from(ocr_script),
                llm_python: PathBuf::from(llm_python),
                llm_cookbook_script: PathBuf::from(llm_cookbook_script),
                llm_recipe_script: PathBuf::from(llm_recipe_script),
                ocr_batch_page_limit,
                concurrency,
            },
        })
    }

    #[must_use]
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port)
    }
}

fn positive_usize(
    name: &'static str,
    value: Option<String>,
    default: usize,
    maximum: usize,
) -> Result<usize, ConfigError> {
    let Some(value) = value else {
        return Ok(default);
    };
    value
        .parse::<usize>()
        .ok()
        .filter(|parsed| (1..=maximum).contains(parsed))
        .ok_or(ConfigError::InvalidPositiveInteger {
            name,
            value,
            maximum,
        })
}

fn first_value<F>(lookup: &mut F, keys: &[&str]) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    keys.iter()
        .find_map(|key| lookup(key).filter(|value| !value.is_empty()))
}

fn parse_store_mode(value: &str) -> Result<StoreMode, ConfigError> {
    match value.to_ascii_lowercase().as_str() {
        "read-only" | "readonly" | "ro" => Ok(StoreMode::ReadOnly),
        "read-write" | "readwrite" | "rw" => Ok(StoreMode::ReadWrite),
        _ => Err(ConfigError::InvalidStoreMode(value.to_owned())),
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("invalid API host {value:?}: {source}")]
    InvalidHost {
        value: String,
        #[source]
        source: AddrParseError,
    },
    #[error("invalid API port {value:?}: {source}")]
    InvalidPort {
        value: String,
        #[source]
        source: ParseIntError,
    },
    #[error("invalid RECITOPIA_RUST_STORE_MODE {0:?}; expected read-only or read-write")]
    InvalidStoreMode(String),
    #[error("invalid {name} {value:?}; expected an integer from 1 through {maximum}")]
    InvalidPositiveInteger {
        name: &'static str,
        value: String,
        maximum: usize,
    },
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn config(values: &[(&str, &str)]) -> Result<Config, ConfigError> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect::<HashMap<_, _>>();
        Config::from_lookup(|key| values.get(key).cloned())
    }

    #[test]
    fn defaults_to_parallel_port_and_read_only_store() {
        let result = config(&[]).expect("default configuration");

        assert_eq!(result.host, "127.0.0.1".parse::<IpAddr>().unwrap());
        assert_eq!(result.port, 8079);
        assert_eq!(result.database.mode, StoreMode::ReadOnly);
        assert_eq!(result.database.path, PathBuf::from(DEFAULT_DATABASE_PATH));
        assert_eq!(
            result.assets.import_dir,
            PathBuf::from(DEFAULT_IMPORT_DIRECTORY)
        );
        assert_eq!(result.assets.image_convert_bin, None);
        assert_eq!(result.pipeline.ocr_server_url, DEFAULT_OCR_SERVER_URL);
        assert_eq!(result.pipeline.ocr_batch_page_limit, 8);
        assert_eq!(result.pipeline.concurrency, 2);
    }

    #[test]
    fn rust_specific_values_override_zig_compatible_values() {
        let result = config(&[
            ("RECITOPIA_API_HOST", "127.0.0.2"),
            ("RECITOPIA_RUST_API_HOST", "0.0.0.0"),
            ("RECITOPIA_API_PORT", "8077"),
            ("RECITOPIA_RUST_API_PORT", "18079"),
            ("RECITOPIA_DB_PATH", "zig.duckdb"),
            ("RECITOPIA_RUST_DB_PATH", ":memory:"),
            ("RECITOPIA_RUST_STORE_MODE", "rw"),
            ("RECITOPIA_IMPORT_DIR", "zig-imports"),
            ("RECITOPIA_RUST_IMPORT_DIR", "rust-imports"),
            ("RECITOPIA_IMAGE_CONVERT_BIN", "/bin/magick"),
            ("RECITOPIA_OCR_SERVER_URL", "http://127.0.0.1:18078"),
            ("RECITOPIA_RUST_OCR_BATCH_PAGE_LIMIT", "12"),
            ("RECITOPIA_RUST_PIPELINE_CONCURRENCY", "3"),
        ])
        .expect("override configuration");

        assert_eq!(result.host, "0.0.0.0".parse::<IpAddr>().unwrap());
        assert_eq!(result.port, 18079);
        assert!(result.database.is_memory());
        assert_eq!(result.database.mode, StoreMode::ReadWrite);
        assert_eq!(result.assets.import_dir, PathBuf::from("rust-imports"));
        assert_eq!(
            result.assets.image_convert_bin,
            Some(PathBuf::from("/bin/magick"))
        );
        assert_eq!(result.pipeline.ocr_server_url, "http://127.0.0.1:18078");
        assert_eq!(result.pipeline.ocr_batch_page_limit, 12);
        assert_eq!(result.pipeline.concurrency, 3);
    }

    #[test]
    fn rejects_invalid_values() {
        assert!(matches!(
            config(&[("RECITOPIA_RUST_API_HOST", "example-host")]),
            Err(ConfigError::InvalidHost { .. })
        ));
        assert!(matches!(
            config(&[("RECITOPIA_RUST_API_PORT", "99999")]),
            Err(ConfigError::InvalidPort { .. })
        ));
        assert!(matches!(
            config(&[("RECITOPIA_RUST_STORE_MODE", "sometimes")]),
            Err(ConfigError::InvalidStoreMode(_))
        ));
        assert!(matches!(
            config(&[("RECITOPIA_RUST_PIPELINE_CONCURRENCY", "0")]),
            Err(ConfigError::InvalidPositiveInteger { .. })
        ));
    }
}

#[must_use]
pub fn llm_configured() -> bool {
    let Ok(provider) = std::env::var("RECITOPIA_LLM_PROVIDER") else {
        return false;
    };
    let provider = provider.trim().to_ascii_lowercase();
    if provider.is_empty() {
        return false;
    }
    if std::env::var_os("RECITOPIA_LLM_API_KEY").is_some() {
        return true;
    }
    let keys: &[&str] = match provider.as_str() {
        "anthropic" => &["ANTHROPIC_API_KEY"],
        "google" => &["GOOGLE_API_KEY", "GEMINI_API_KEY"],
        "openai" => &["OPENAI_API_KEY"],
        "openrouter" => &["OPENROUTER_API_KEY"],
        "llm" => &["RECITOPIA_LLM_API_KEY"],
        _ => return false,
    };
    keys.iter().any(|key| std::env::var_os(key).is_some())
}
