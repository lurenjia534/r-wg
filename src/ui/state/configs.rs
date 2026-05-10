use std::ops::{Deref, DerefMut};

use crate::ui::persistence::{self, StoragePaths};

use super::TunnelConfig;

pub(crate) struct ConfigsState {
    /// 全部隧道配置。
    pub(crate) configs: Vec<TunnelConfig>,
    /// 配置持久化目录与 state.json 路径。
    pub(crate) storage: Option<StoragePaths>,
    /// 下一个配置 ID（用于内部文件名）。
    pub(crate) next_config_id: u64,
}

impl ConfigsState {
    pub(super) fn new() -> Self {
        Self {
            configs: Vec::new(),
            storage: None,
            next_config_id: 1,
        }
    }

    pub(crate) fn ensure_storage(&mut self) -> Result<StoragePaths, String> {
        if let Some(storage) = &self.storage {
            return Ok(storage.clone());
        }
        let storage = persistence::ensure_storage_dirs()?;
        self.storage = Some(storage.clone());
        Ok(storage)
    }

    pub(crate) fn alloc_config_id(&mut self) -> u64 {
        let id = self.next_config_id.max(1);
        self.next_config_id = id.saturating_add(1);
        id
    }

    pub(crate) fn next_config_id(&self) -> u64 {
        self.next_config_id.max(1)
    }

    pub(crate) fn find_by_id(&self, config_id: u64) -> Option<TunnelConfig> {
        self.get_by_id(config_id).cloned()
    }

    pub(crate) fn find_index_by_id(&self, config_id: u64) -> Option<usize> {
        self.iter().position(|config| config.id == config_id)
    }

    pub(crate) fn get_by_id(&self, config_id: u64) -> Option<&TunnelConfig> {
        self.iter().find(|config| config.id == config_id)
    }

    pub(crate) fn get_mut_by_id(&mut self, config_id: u64) -> Option<&mut TunnelConfig> {
        self.iter_mut().find(|config| config.id == config_id)
    }
}

impl Deref for ConfigsState {
    type Target = Vec<TunnelConfig>;

    fn deref(&self) -> &Self::Target {
        &self.configs
    }
}

impl DerefMut for ConfigsState {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.configs
    }
}

impl<'a> IntoIterator for &'a ConfigsState {
    type Item = &'a TunnelConfig;
    type IntoIter = std::slice::Iter<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter()
    }
}

impl<'a> IntoIterator for &'a mut ConfigsState {
    type Item = &'a mut TunnelConfig;
    type IntoIter = std::slice::IterMut<'a, TunnelConfig>;

    fn into_iter(self) -> Self::IntoIter {
        self.configs.iter_mut()
    }
}
