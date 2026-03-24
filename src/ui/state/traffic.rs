/// 按天统计的 RX/TX bucket，`day_key` 为自 Unix epoch 起的天数。
#[derive(Clone)]
pub(crate) struct TrafficDayBucket {
    pub(crate) day_key: i32,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

/// 按小时统计的 RX/TX bucket，`hour_key` 为自 Unix epoch 起的小时数。
#[derive(Clone)]
pub(crate) struct TrafficHourBucket {
    pub(crate) hour_key: i64,
    pub(crate) rx_bytes: u64,
    pub(crate) tx_bytes: u64,
}

pub(crate) struct TrafficStore {
    pub(crate) global_days: Vec<TrafficDayBucket>,
    pub(crate) global_hours: Vec<TrafficHourBucket>,
    pub(crate) config_days: HashMap<u64, Vec<TrafficDayBucket>>,
    pub(crate) config_hours: HashMap<u64, Vec<TrafficHourBucket>>,
    pub(crate) dirty: bool,
    pub(crate) last_persist_at: Option<Instant>,
    pub(crate) rev: u64,
}

impl TrafficStore {
    pub(crate) fn new() -> Self {
        Self {
            global_days: Vec::new(),
            global_hours: Vec::new(),
            config_days: HashMap::new(),
            config_hours: HashMap::new(),
            dirty: false,
            last_persist_at: None,
            rev: 0,
        }
    }

    pub(crate) fn record(
        &mut self,
        config_id: Option<u64>,
        day_key: i32,
        hour_key: i64,
        rx_bytes: u64,
        tx_bytes: u64,
    ) -> bool {
        if rx_bytes.saturating_add(tx_bytes) == 0 {
            return false;
        }

        let mut created = false;
        if update_traffic_day_buckets(&mut self.global_days, day_key, rx_bytes, tx_bytes) {
            created = true;
        }
        if update_traffic_hour_buckets(&mut self.global_hours, hour_key, rx_bytes, tx_bytes) {
            created = true;
        }

        if let Some(config_id) = config_id {
            let days = self.config_days.entry(config_id).or_default();
            if update_traffic_day_buckets(days, day_key, rx_bytes, tx_bytes) {
                created = true;
            }

            let hours = self.config_hours.entry(config_id).or_default();
            if update_traffic_hour_buckets(hours, hour_key, rx_bytes, tx_bytes) {
                created = true;
            }
        }

        self.dirty = true;
        self.rev = self.rev.wrapping_add(1);
        created
    }

    pub(crate) fn remove_config(&mut self, config_id: u64) -> bool {
        let removed_days = self.config_days.remove(&config_id).is_some();
        let removed_hours = self.config_hours.remove(&config_id).is_some();
        let removed = removed_days || removed_hours;
        if removed {
            self.rev = self.rev.wrapping_add(1);
        }
        removed
    }

    pub(crate) fn mark_persisted(&mut self, at: Instant) {
        self.last_persist_at = Some(at);
        self.dirty = false;
    }

    pub(crate) fn reset_persist_state(&mut self) {
        self.dirty = false;
        self.last_persist_at = None;
    }
}

pub(crate) fn day_key_from_date(date: NaiveDate) -> i32 {
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).expect("epoch date should be valid");
    date.signed_duration_since(epoch).num_days() as i32
}

fn update_traffic_day_buckets(
    days: &mut Vec<TrafficDayBucket>,
    day_key: i32,
    rx_bytes: u64,
    tx_bytes: u64,
) -> bool {
    if let Some(day) = days.iter_mut().find(|day| day.day_key == day_key) {
        day.rx_bytes = day.rx_bytes.saturating_add(rx_bytes);
        day.tx_bytes = day.tx_bytes.saturating_add(tx_bytes);
        return false;
    }

    days.push(TrafficDayBucket {
        day_key,
        rx_bytes,
        tx_bytes,
    });
    prune_traffic_day_buckets(days);
    true
}

fn update_traffic_hour_buckets(
    hours: &mut Vec<TrafficHourBucket>,
    hour_key: i64,
    rx_bytes: u64,
    tx_bytes: u64,
) -> bool {
    if let Some(hour) = hours.iter_mut().find(|hour| hour.hour_key == hour_key) {
        hour.rx_bytes = hour.rx_bytes.saturating_add(rx_bytes);
        hour.tx_bytes = hour.tx_bytes.saturating_add(tx_bytes);
        return false;
    }

    hours.push(TrafficHourBucket {
        hour_key,
        rx_bytes,
        tx_bytes,
    });
    prune_traffic_hour_buckets(hours);
    true
}

fn prune_traffic_day_buckets(days: &mut Vec<TrafficDayBucket>) {
    days.sort_by(|a, b| a.day_key.cmp(&b.day_key));
    if days.len() > TRAFFIC_ROLLING_DAYS {
        let remove_count = days.len() - TRAFFIC_ROLLING_DAYS;
        days.drain(0..remove_count);
    }
}

fn prune_traffic_hour_buckets(hours: &mut Vec<TrafficHourBucket>) {
    hours.sort_by(|a, b| a.hour_key.cmp(&b.hour_key));
    if hours.len() > TRAFFIC_HOURLY_HISTORY {
        let remove_count = hours.len() - TRAFFIC_HOURLY_HISTORY;
        hours.drain(0..remove_count);
    }
}
