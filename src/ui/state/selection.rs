use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;

use gpui::SharedString;

use crate::ui::features::configs::state::LoadedConfigState;

use super::{ConfigsState, PendingStart, ProxyRunningFilter, RuntimeState};

pub(crate) struct SelectionState {
    /// 是否已触发持久化加载，避免重复启动加载任务。
    pub(crate) persistence_loaded: bool,
    /// 当前单选中的配置 ID。
    pub(crate) selected_id: Option<u64>,
    /// 正在异步加载的配置 ID（用于防止 UI 写入旧数据）。
    pub(crate) loading_config_id: Option<u64>,
    /// 正在异步加载的配置路径（用于防止索引复用带来的错写）。
    pub(crate) loading_config_path: Option<PathBuf>,
    /// 最近一次写入输入框的配置标记，用于跳过重复 set_value。
    pub(crate) loaded_config: Option<LoadedConfigState>,
    /// 文本缓存：按路径缓存最近读取的配置文本，减少重复 IO。
    pub(crate) config_text_cache: HashMap<PathBuf, SharedString>,
    /// 文本缓存顺序：用于简易 LRU 淘汰。
    pub(crate) config_text_cache_order: VecDeque<PathBuf>,
    /// Endpoint metadata 正在后台计算中的配置 ID。
    pub(crate) endpoint_family_loading: HashSet<u64>,
    /// 代理列表结构化筛选：国家。
    pub(crate) proxy_country_filter: Option<String>,
    /// 代理列表结构化筛选：城市。
    pub(crate) proxy_city_filter: Option<String>,
    /// 代理列表结构化筛选：协议类型。
    pub(crate) proxy_protocol_filter: Option<String>,
    /// 代理列表结构化筛选：运行状态。
    pub(crate) proxy_running_filter: ProxyRunningFilter,
    /// 代理/节点多选模式开关。
    pub(crate) proxy_select_mode: bool,
    /// 代理/节点多选：选中的配置 ID 列表。
    pub(crate) proxy_selected_ids: HashSet<u64>,
    pub(crate) selection_revision: u64,
}

impl SelectionState {
    pub(super) fn new() -> Self {
        Self {
            persistence_loaded: false,
            selected_id: None,
            loading_config_id: None,
            loading_config_path: None,
            loaded_config: None,
            config_text_cache: HashMap::new(),
            config_text_cache_order: VecDeque::new(),
            endpoint_family_loading: HashSet::new(),
            proxy_country_filter: None,
            proxy_city_filter: None,
            proxy_protocol_filter: None,
            proxy_running_filter: ProxyRunningFilter::All,
            proxy_select_mode: false,
            proxy_selected_ids: HashSet::new(),
            selection_revision: 0,
        }
    }

    pub(crate) fn begin_persistence_load(&mut self) -> bool {
        if self.persistence_loaded {
            return false;
        }
        self.persistence_loaded = true;
        true
    }

    pub(crate) fn build_pending_start(
        &self,
        configs: &ConfigsState,
        runtime: &RuntimeState,
    ) -> Option<PendingStart> {
        if let Some(config_id) = self.selected_id {
            return configs.get_by_id(config_id).map(|config| PendingStart {
                config_id: config.id,
                password_authorized: false,
            });
        }
        runtime.running_id.map(|id| PendingStart {
            config_id: id,
            password_authorized: false,
        })
    }

    pub(crate) fn restore_after_persist(
        &mut self,
        selected_id: Option<u64>,
        configs: &ConfigsState,
    ) {
        self.selected_id = selected_id.filter(|id| configs.get_by_id(*id).is_some());
        self.loaded_config = None;
        self.loading_config_id = None;
        self.loading_config_path = None;
        self.selection_revision = self.selection_revision.wrapping_add(1);
    }
}
