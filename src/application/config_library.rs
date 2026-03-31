use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::{fs, io::ErrorKind};

use crate::core::config;

#[derive(Clone, Default)]
pub struct ConfigLibraryService;

impl ConfigLibraryService {
    pub fn new() -> Self {
        Self
    }

    pub fn next_available_name<'a>(
        &self,
        names: impl IntoIterator<Item = &'a str>,
        base: &str,
    ) -> String {
        let names: HashSet<&str> = names.into_iter().collect();

        if !names.contains(base) {
            return base.to_string();
        }
        for idx in 2..1000 {
            let candidate = format!("{base}-{idx}");
            if !names.contains(candidate.as_str()) {
                return candidate;
            }
        }
        format!("{base}-{}", names.len() + 1)
    }

    pub fn reserve_unique_name(&self, names_in_use: &mut HashSet<String>, base: &str) -> String {
        let candidate = self.next_available_name(names_in_use.iter().map(String::as_str), base);
        names_in_use.insert(candidate.clone());
        candidate
    }

    pub fn validate_save_request<'a>(
        &self,
        request: SaveConfigRequest<'a>,
    ) -> Result<ValidatedSaveRequest, SaveConfigError> {
        let name = request.requested_name.trim();
        if name.is_empty() {
            return Err(SaveConfigError::MissingName);
        }

        if request.text.trim().is_empty() {
            return Err(SaveConfigError::MissingText);
        }

        if request
            .existing_configs
            .iter()
            .any(|entry| entry.name == name && Some(entry.id) != request.source_id)
        {
            return Err(SaveConfigError::DuplicateName);
        }

        Ok(ValidatedSaveRequest {
            name: name.to_string(),
        })
    }

    pub fn read_import_source(&self, path: &Path) -> Result<ImportSource, String> {
        let text = std::fs::read_to_string(path).map_err(|err| format!("Read failed: {err}"))?;
        config::parse_config(&text).map_err(|err| format!("Invalid config: {err}"))?;
        Ok(ImportSource {
            suggested_name: suggested_name_from_path(path),
            text,
        })
    }

    pub fn read_config_text(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|err| format!("Read failed: {err}"))
    }

    pub fn write_config_text(&self, path: &Path, text: &str) -> Result<(), String> {
        write_atomic(path, text.as_bytes())
    }

    pub fn export_config(&self, path: &Path, text: &str) -> Result<PathBuf, String> {
        self.write_config_text(path, text)?;
        Ok(path.to_path_buf())
    }

    pub fn delete_config_files(&self, paths: &[PathBuf]) -> Result<(), String> {
        for path in paths {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(err) if err.kind() == ErrorKind::NotFound => {}
                Err(err) => return Err(format!("Remove file failed: {err}")),
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct ExistingConfigName<'a> {
    pub id: u64,
    pub name: &'a str,
}

pub struct SaveConfigRequest<'a> {
    pub requested_name: &'a str,
    pub text: &'a str,
    pub source_id: Option<u64>,
    pub existing_configs: &'a [ExistingConfigName<'a>],
}

pub struct ValidatedSaveRequest {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveConfigError {
    MissingName,
    MissingText,
    DuplicateName,
}

impl SaveConfigError {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingName => "Tunnel name is required",
            Self::MissingText => "Config text is required",
            Self::DuplicateName => "Tunnel name already exists",
        }
    }
}

pub struct ImportSource {
    pub suggested_name: String,
    pub text: String,
}

fn suggested_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("Tunnel")
        .to_string()
}

fn write_atomic(path: &Path, contents: &[u8]) -> Result<(), String> {
    let tmp_path = path.with_extension("tmp");
    std::fs::write(&tmp_path, contents).map_err(|err| format!("Write temp file failed: {err}"))?;
    if let Err(err) = std::fs::rename(&tmp_path, path) {
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|remove_err| format!("Remove old file failed: {remove_err}"))?;
            std::fs::rename(&tmp_path, path)
                .map_err(|rename_err| format!("Replace file failed: {rename_err}"))?;
            return Ok(());
        }
        return Err(format!("Commit file failed: {err}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_available_name_preserves_base_when_unused() {
        let service = ConfigLibraryService::new();
        let names = ["alpha", "beta"];
        assert_eq!(service.next_available_name(names, "gamma"), "gamma");
    }

    #[test]
    fn reserve_unique_name_updates_set() {
        let service = ConfigLibraryService::new();
        let mut names = HashSet::from(["alpha".to_string()]);
        let reserved = service.reserve_unique_name(&mut names, "alpha");
        assert_eq!(reserved, "alpha-2");
        assert!(names.contains("alpha-2"));
    }

    #[test]
    fn validate_save_request_rejects_duplicate_names() {
        let service = ConfigLibraryService::new();
        let existing = [ExistingConfigName { id: 7, name: "alpha" }];
        let result = service.validate_save_request(SaveConfigRequest {
            requested_name: "alpha",
            text: "[Interface]\nPrivateKey = test",
            source_id: None,
            existing_configs: &existing,
        });

        assert_eq!(result.err(), Some(SaveConfigError::DuplicateName));
    }
}
