use std::io::{self, ErrorKind};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::Serialize;

use super::atomic;

#[derive(Debug)]
pub enum StateRepositoryError {
    Read(io::Error),
    Parse(serde_json::Error),
    Serialize(serde_json::Error),
    Write(io::Error),
}

#[derive(Clone, Default)]
pub struct StateRepository;

impl StateRepository {
    pub fn new() -> Self {
        Self
    }

    pub fn load_json<T>(&self, path: &Path) -> Result<Option<T>, StateRepositoryError>
    where
        T: DeserializeOwned,
    {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(StateRepositoryError::Read(err)),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(StateRepositoryError::Parse)
    }

    pub fn save_json<T>(&self, path: &Path, state: &T) -> Result<(), StateRepositoryError>
    where
        T: Serialize,
    {
        let data = serde_json::to_vec_pretty(state).map_err(StateRepositoryError::Serialize)?;
        atomic::write_atomic(path, &data).map_err(StateRepositoryError::Write)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
    struct SampleState {
        version: u32,
        name: String,
    }

    fn temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("r-wg-state-repo-{label}-{unique}"));
        fs::create_dir_all(&dir).expect("temp dir should be created");
        dir
    }

    #[test]
    fn load_json_returns_none_for_missing_state() {
        let dir = temp_dir("missing");
        let repository = StateRepository::new();

        let loaded = repository
            .load_json::<SampleState>(&dir.join("state.json"))
            .expect("missing state should not fail");

        assert_eq!(loaded, None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_and_load_json_round_trips() {
        let dir = temp_dir("round-trip");
        let path = dir.join("state.json");
        let repository = StateRepository::new();
        let state = SampleState {
            version: 4,
            name: "alpha".to_string(),
        };

        repository
            .save_json(&path, &state)
            .expect("state should save");
        let loaded: SampleState = repository
            .load_json(&path)
            .expect("state should load")
            .expect("state should exist");

        assert_eq!(loaded, state);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_json_reports_parse_errors() {
        let dir = temp_dir("parse");
        let path = dir.join("state.json");
        fs::write(&path, "{").expect("fixture should write");
        let repository = StateRepository::new();

        let error = repository
            .load_json::<SampleState>(&path)
            .expect_err("invalid json should fail");

        assert!(matches!(error, StateRepositoryError::Parse(_)));
        let _ = fs::remove_dir_all(dir);
    }
}
