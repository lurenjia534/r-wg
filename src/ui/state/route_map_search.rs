use gpui::SharedString;

pub(crate) struct RouteMapSearchState {
    pub(crate) raw_query: SharedString,
    pub(crate) debounced_query: SharedString,
    next_revision: u64,
    queued_revision: Option<u64>,
    pub(crate) worker_active: bool,
}

impl RouteMapSearchState {
    pub(super) fn new() -> Self {
        Self {
            raw_query: SharedString::default(),
            debounced_query: SharedString::default(),
            next_revision: 0,
            queued_revision: None,
            worker_active: false,
        }
    }

    pub(crate) fn enqueue(&mut self) -> u64 {
        self.next_revision = self.next_revision.saturating_add(1);
        self.queued_revision = Some(self.next_revision);
        self.next_revision
    }

    pub(crate) fn take_queued_revision(&mut self) -> Option<u64> {
        self.queued_revision.take()
    }
}
