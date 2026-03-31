use std::env;
use std::fs;
use std::path::PathBuf;

use super::super::NetworkError;
use crate::core::route_plan::RouteApplyReport;

const LAST_APPLY_REPORT_FILE: &str = "last-apply-report.json";

pub(crate) fn write_persisted_apply_report(report: &RouteApplyReport) -> Result<(), NetworkError> {
    let path = last_apply_report_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(report).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    fs::write(path, json)?;
    Ok(())
}

pub(crate) fn load_persisted_apply_report() -> Result<Option<RouteApplyReport>, NetworkError> {
    let path = last_apply_report_path();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(NetworkError::Io(err)),
    };
    let report = serde_json::from_str(&text).map_err(|err| {
        NetworkError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    })?;
    Ok(Some(report))
}

pub(crate) fn clear_persisted_apply_report() -> Result<(), NetworkError> {
    let path = last_apply_report_path();
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(NetworkError::Io(err)),
    }
}

fn last_apply_report_path() -> PathBuf {
    if let Some(dir) = env::var_os("STATE_DIRECTORY") {
        return PathBuf::from(dir).join(LAST_APPLY_REPORT_FILE);
    }
    PathBuf::from("/var/lib/r-wg").join(LAST_APPLY_REPORT_FILE)
}
