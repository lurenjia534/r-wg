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

    pub fn plan_save_target<'a>(
        &self,
        request: SaveTargetRequest<'a>,
    ) -> Result<SaveTargetPlan, SaveConfigError> {
        let existing_names = request
            .existing_configs
            .iter()
            .map(|config| ExistingConfigName {
                id: config.id,
                name: config.name,
            })
            .collect::<Vec<_>>();
        let validated = self.validate_save_request(SaveConfigRequest {
            requested_name: request.requested_name,
            text: request.text,
            source_id: if request.force_new {
                None
            } else {
                request.source_id
            },
            existing_configs: &existing_names,
        })?;

        let source_id = if request.force_new {
            None
        } else {
            request.source_id
        };

        if let Some(existing) = source_id.and_then(|id| {
            request
                .existing_configs
                .iter()
                .find(|config| config.id == id)
        }) {
            return Ok(SaveTargetPlan {
                id: existing.id,
                name: validated.name,
                storage_path: existing.storage_path.to_path_buf(),
                source: existing.source,
                is_new: false,
            });
        }

        Ok(SaveTargetPlan {
            id: request.next_id,
            name: validated.name,
            storage_path: request.next_storage_path,
            source: ConfigSourceKind::Paste,
            is_new: true,
        })
    }

    pub fn plan_rename<'a>(
        &self,
        request: RenameConfigRequest<'a>,
    ) -> Result<RenameConfigDecision, RenameConfigError> {
        let name = request.requested_name.trim();
        if name.is_empty() {
            return Err(RenameConfigError::MissingName);
        }

        let target_id = request
            .source_id
            .or(request.selected_id)
            .ok_or(RenameConfigError::MissingSelection)?;
        let target = request
            .existing_configs
            .iter()
            .find(|config| config.id == target_id)
            .ok_or(RenameConfigError::MissingConfig)?;

        if target.name == name {
            return Ok(RenameConfigDecision::Unchanged);
        }

        if request
            .existing_configs
            .iter()
            .any(|config| config.id != target_id && config.name == name)
        {
            return Err(RenameConfigError::DuplicateName);
        }

        Ok(RenameConfigDecision::Rename {
            config_id: target_id,
            previous_name: target.name.to_string(),
            name: name.to_string(),
        })
    }

    pub fn plan_delete<'a>(&self, request: DeleteConfigsRequest<'a>) -> DeleteConfigsDecision {
        if request.requested_ids.is_empty() {
            return DeleteConfigsDecision::NoSelection;
        }

        let requested_ids: HashSet<u64> = request.requested_ids.iter().copied().collect();
        let mut deleted_ids = Vec::new();
        let mut deleted_names = Vec::new();
        let mut deleted_paths = Vec::new();
        let mut skipped_running = Vec::new();

        for config in request.existing_configs {
            if !requested_ids.contains(&config.id) {
                continue;
            }

            let is_running =
                request.running_id == Some(config.id) || request.running_name == Some(config.name);
            if is_running {
                match request.policy {
                    DeletePolicy::BlockRunning => return DeleteConfigsDecision::BlockedRunning,
                    DeletePolicy::SkipRunning => {
                        skipped_running.push(config.name.to_string());
                        continue;
                    }
                }
            }

            deleted_ids.push(config.id);
            deleted_names.push(config.name.to_string());
            deleted_paths.push(config.storage_path.to_path_buf());
        }

        if deleted_ids.is_empty() {
            if skipped_running.is_empty() {
                DeleteConfigsDecision::NoSelection
            } else {
                DeleteConfigsDecision::OnlySkippedRunning { skipped_running }
            }
        } else {
            DeleteConfigsDecision::Delete(DeleteConfigsPlan {
                deleted_ids,
                deleted_names,
                deleted_paths,
                skipped_running,
            })
        }
    }

    pub fn begin_import_batch(
        &self,
        names_in_use: HashSet<String>,
        total: usize,
    ) -> ImportBatchState {
        ImportBatchState {
            names_in_use,
            processed: 0,
            total,
            imported: 0,
            failed: 0,
            last_error: None,
            last_imported_id: None,
        }
    }

    pub fn record_import_success(
        &self,
        batch: &mut ImportBatchState,
        imported: ImportedConfigRecord,
    ) -> RecordedImportSuccess {
        batch.imported += 1;
        batch.processed += 1;
        let name = self.reserve_unique_name(&mut batch.names_in_use, &imported.name);
        batch.last_imported_id = Some(imported.id);
        RecordedImportSuccess {
            config: ImportedConfigRecord {
                id: imported.id,
                name,
                origin_path: imported.origin_path,
                storage_path: imported.storage_path,
                source: imported.source,
            },
            progress: ImportProgress {
                processed: batch.processed,
                total: batch.total,
                status_message: format!("Importing {}/{}...", batch.processed, batch.total),
            },
        }
    }

    pub fn record_import_failure(
        &self,
        batch: &mut ImportBatchState,
        path: &Path,
        message: &str,
    ) -> ImportProgress {
        batch.failed += 1;
        batch.processed += 1;
        batch.last_error = Some(format!("{message} ({})", path.display()));
        ImportProgress {
            processed: batch.processed,
            total: batch.total,
            status_message: format!("Importing {}/{}...", batch.processed, batch.total),
        }
    }

    pub fn finish_import_batch(&self, batch: ImportBatchState) -> FinalizedImportBatch {
        let status_message = if batch.imported == 0 && batch.failed > 0 {
            None
        } else if batch.failed > 0 {
            Some(format!(
                "Imported {} configs, {} failed",
                batch.imported, batch.failed
            ))
        } else {
            Some(format!("Imported {} configs", batch.imported))
        };
        let error_message = if batch.imported == 0 && batch.failed > 0 {
            Some(
                batch
                    .last_error
                    .unwrap_or_else(|| "Import failed".to_string()),
            )
        } else {
            None
        };

        FinalizedImportBatch {
            imported: batch.imported,
            failed: batch.failed,
            should_persist: batch.imported > 0,
            selected_import_id: batch.last_imported_id,
            status_message,
            error_message,
        }
    }

    pub fn delete_status_message(
        &self,
        deleted_names: &[String],
        skipped_running: usize,
    ) -> String {
        let deleted_count = deleted_names.len();
        if deleted_count == 0 && skipped_running > 0 {
            if skipped_running == 1 {
                return "Skipped 1 running config".to_string();
            }
            return format!("Skipped {skipped_running} running configs");
        }
        if deleted_count == 1 && skipped_running == 0 {
            return format!("Deleted {}", deleted_names[0]);
        }
        let config_word = if deleted_count == 1 {
            "config"
        } else {
            "configs"
        };
        if skipped_running > 0 {
            return format!(
                "Deleted {deleted_count} {config_word}, skipped {skipped_running} running"
            );
        }
        format!("Deleted {deleted_count} {config_word}")
    }

    pub fn plan_post_delete_selection(
        &self,
        request: PostDeleteSelectionRequest<'_>,
    ) -> PostDeleteSelection {
        if request.remaining_ids.is_empty() {
            return PostDeleteSelection::Clear;
        }
        if let Some(selected_id) = request.previous_selected_id {
            if request.remaining_ids.contains(&selected_id) {
                return PostDeleteSelection::Keep(selected_id);
            }
        }
        if let Some(previous_index) = request.previous_selected_index {
            let index = previous_index.min(request.remaining_ids.len() - 1);
            return PostDeleteSelection::SelectFallback(request.remaining_ids[index]);
        }
        PostDeleteSelection::Clear
    }

    pub fn read_import_source(&self, path: &Path) -> Result<ImportSource, String> {
        let text = std::fs::read_to_string(path).map_err(|err| format!("Read failed: {err}"))?;
        config::parse_config(&text).map_err(|err| format!("Invalid config: {err}"))?;
        Ok(ImportSource {
            suggested_name: suggested_name_from_path(path),
            text,
        })
    }

    pub fn import_config_job(
        &self,
        job: ImportConfigJob,
    ) -> Result<ImportedConfigArtifact, String> {
        let import_source = self.read_import_source(&job.origin_path)?;
        self.write_config_text(&job.storage_path, &import_source.text)?;
        Ok(ImportedConfigArtifact {
            id: job.id,
            suggested_name: import_source.suggested_name,
            origin_path: job.origin_path,
            storage_path: job.storage_path,
            text: import_source.text,
        })
    }

    pub fn read_config_text(&self, path: &Path) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|err| format!("Read failed: {err}"))
    }

    pub fn resolve_export_text(
        &self,
        initial_text: Option<String>,
        storage_path: &Path,
    ) -> Result<String, String> {
        match initial_text {
            Some(text) => Ok(text),
            None => self.read_config_text(storage_path),
        }
    }

    pub fn write_config_text(&self, path: &Path, text: &str) -> Result<(), String> {
        write_atomic(path, text.as_bytes())
    }

    pub fn plan_export_path(&self, directory: &Path, config_name: &str) -> PathBuf {
        directory.join(format!("{}.conf", sanitize_export_stem(config_name)))
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigSourceKind {
    File,
    Paste,
}

#[derive(Clone, Copy)]
pub struct ExistingStoredConfig<'a> {
    pub id: u64,
    pub name: &'a str,
    pub storage_path: &'a Path,
    pub source: ConfigSourceKind,
}

pub struct SaveTargetRequest<'a> {
    pub requested_name: &'a str,
    pub text: &'a str,
    pub source_id: Option<u64>,
    pub force_new: bool,
    pub existing_configs: &'a [ExistingStoredConfig<'a>],
    pub next_id: u64,
    pub next_storage_path: PathBuf,
}

pub struct SaveTargetPlan {
    pub id: u64,
    pub name: String,
    pub storage_path: PathBuf,
    pub source: ConfigSourceKind,
    pub is_new: bool,
}

pub struct RenameConfigRequest<'a> {
    pub requested_name: &'a str,
    pub source_id: Option<u64>,
    pub selected_id: Option<u64>,
    pub existing_configs: &'a [ExistingStoredConfig<'a>],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenameConfigDecision {
    Unchanged,
    Rename {
        config_id: u64,
        previous_name: String,
        name: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenameConfigError {
    MissingName,
    MissingSelection,
    MissingConfig,
    DuplicateName,
}

impl RenameConfigError {
    pub fn message(self) -> &'static str {
        match self {
            Self::MissingName => "Tunnel name is required",
            Self::MissingSelection => "Select a tunnel first",
            Self::MissingConfig => "Selected tunnel no longer exists",
            Self::DuplicateName => "Tunnel name already exists",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeletePolicy {
    BlockRunning,
    SkipRunning,
}

pub struct DeleteConfigsRequest<'a> {
    pub requested_ids: &'a [u64],
    pub existing_configs: &'a [ExistingStoredConfig<'a>],
    pub running_id: Option<u64>,
    pub running_name: Option<&'a str>,
    pub policy: DeletePolicy,
}

pub struct DeleteConfigsPlan {
    pub deleted_ids: Vec<u64>,
    pub deleted_names: Vec<String>,
    pub deleted_paths: Vec<PathBuf>,
    pub skipped_running: Vec<String>,
}

pub enum DeleteConfigsDecision {
    NoSelection,
    BlockedRunning,
    OnlySkippedRunning { skipped_running: Vec<String> },
    Delete(DeleteConfigsPlan),
}

pub struct ImportBatchState {
    names_in_use: HashSet<String>,
    processed: usize,
    total: usize,
    imported: usize,
    failed: usize,
    last_error: Option<String>,
    last_imported_id: Option<u64>,
}

pub struct ImportProgress {
    pub processed: usize,
    pub total: usize,
    pub status_message: String,
}

pub struct RecordedImportSuccess {
    pub config: ImportedConfigRecord,
    pub progress: ImportProgress,
}

pub struct FinalizedImportBatch {
    pub imported: usize,
    pub failed: usize,
    pub should_persist: bool,
    pub selected_import_id: Option<u64>,
    pub status_message: Option<String>,
    pub error_message: Option<String>,
}

pub struct ImportedConfigRecord {
    pub id: u64,
    pub name: String,
    pub origin_path: PathBuf,
    pub storage_path: PathBuf,
    pub source: ConfigSourceKind,
}

pub struct PostDeleteSelectionRequest<'a> {
    pub remaining_ids: &'a [u64],
    pub previous_selected_id: Option<u64>,
    pub previous_selected_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostDeleteSelection {
    Clear,
    Keep(u64),
    SelectFallback(u64),
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

pub struct ImportConfigJob {
    pub id: u64,
    pub origin_path: PathBuf,
    pub storage_path: PathBuf,
}

pub struct ImportedConfigArtifact {
    pub id: u64,
    pub suggested_name: String,
    pub origin_path: PathBuf,
    pub storage_path: PathBuf,
    pub text: String,
}

fn suggested_name_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or("Tunnel")
        .to_string()
}

fn sanitize_export_stem(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => out.push('_'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        "tunnel".to_string()
    } else {
        trimmed.to_string()
    }
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
        let existing = [ExistingConfigName {
            id: 7,
            name: "alpha",
        }];
        let result = service.validate_save_request(SaveConfigRequest {
            requested_name: "alpha",
            text: "[Interface]\nPrivateKey = test",
            source_id: None,
            existing_configs: &existing,
        });

        assert_eq!(result.err(), Some(SaveConfigError::DuplicateName));
    }

    #[test]
    fn plan_rename_rejects_duplicate_names() {
        let service = ConfigLibraryService::new();
        let alpha_path = Path::new("/tmp/alpha.conf");
        let beta_path = Path::new("/tmp/beta.conf");
        let existing = [
            ExistingStoredConfig {
                id: 1,
                name: "alpha",
                storage_path: alpha_path,
                source: ConfigSourceKind::Paste,
            },
            ExistingStoredConfig {
                id: 2,
                name: "beta",
                storage_path: beta_path,
                source: ConfigSourceKind::Paste,
            },
        ];

        let result = service.plan_rename(RenameConfigRequest {
            requested_name: "beta",
            source_id: Some(1),
            selected_id: None,
            existing_configs: &existing,
        });

        assert_eq!(result.err(), Some(RenameConfigError::DuplicateName));
    }

    #[test]
    fn plan_delete_skips_running_when_requested() {
        let service = ConfigLibraryService::new();
        let alpha_path = Path::new("/tmp/alpha.conf");
        let existing = [ExistingStoredConfig {
            id: 1,
            name: "alpha",
            storage_path: alpha_path,
            source: ConfigSourceKind::Paste,
        }];

        let decision = service.plan_delete(DeleteConfigsRequest {
            requested_ids: &[1],
            existing_configs: &existing,
            running_id: Some(1),
            running_name: Some("alpha"),
            policy: DeletePolicy::SkipRunning,
        });

        assert!(matches!(
            decision,
            DeleteConfigsDecision::OnlySkippedRunning { .. }
        ));
    }

    #[test]
    fn plan_export_path_sanitizes_config_name() {
        let service = ConfigLibraryService::new();
        let path = service.plan_export_path(Path::new("/tmp"), "a/b:c");
        assert_eq!(path, PathBuf::from("/tmp/a_b_c.conf"));
    }

    #[test]
    fn finish_import_batch_keeps_last_imported_id() {
        let service = ConfigLibraryService::new();
        let mut batch = service.begin_import_batch(HashSet::new(), 2);
        let _ = service.record_import_success(
            &mut batch,
            ImportedConfigRecord {
                id: 7,
                name: "alpha".to_string(),
                origin_path: PathBuf::from("/tmp/alpha.conf"),
                storage_path: PathBuf::from("/tmp/store-alpha.conf"),
                source: ConfigSourceKind::File,
            },
        );

        let summary = service.finish_import_batch(batch);

        assert_eq!(summary.selected_import_id, Some(7));
        assert!(summary.should_persist);
    }

    #[test]
    fn post_delete_selection_falls_back_to_previous_index() {
        let service = ConfigLibraryService::new();
        let selection = service.plan_post_delete_selection(PostDeleteSelectionRequest {
            remaining_ids: &[3, 4],
            previous_selected_id: Some(2),
            previous_selected_index: Some(1),
        });

        assert_eq!(selection, PostDeleteSelection::SelectFallback(4));
    }
}
