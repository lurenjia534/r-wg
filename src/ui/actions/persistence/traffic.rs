use std::collections::{BTreeMap, HashMap, HashSet};

use crate::ui::persistence::{
    PersistedConfigTrafficDayBucket, PersistedConfigTrafficHourBucket, PersistedTrafficDayBucket,
    PersistedTrafficHourBucket,
};
use crate::ui::state::{
    TrafficDayBucket, TrafficHourBucket, TrafficStore, TRAFFIC_HOURLY_HISTORY, TRAFFIC_ROLLING_DAYS,
};

pub(super) fn restore_traffic_store(
    global_days: Vec<PersistedTrafficDayBucket>,
    global_hours: Vec<PersistedTrafficHourBucket>,
    config_days: Vec<PersistedConfigTrafficDayBucket>,
    config_hours: Vec<PersistedConfigTrafficHourBucket>,
    config_ids: &HashSet<u64>,
) -> TrafficStore {
    TrafficStore {
        global_days: merge_day_buckets(global_days),
        global_hours: merge_hour_buckets(global_hours),
        config_days: merge_config_day_buckets(config_days, config_ids),
        config_hours: merge_config_hour_buckets(config_hours, config_ids),
        dirty: false,
        last_persist_at: None,
        rev: 0,
    }
}

fn merge_day_buckets(items: Vec<PersistedTrafficDayBucket>) -> Vec<TrafficDayBucket> {
    let mut map = BTreeMap::<i32, (u64, u64)>::new();
    for day in items {
        let entry = map.entry(day.day_key).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(day.rx_bytes);
        entry.1 = entry.1.saturating_add(day.tx_bytes);
    }
    let mut days = map
        .into_iter()
        .map(|(day_key, (rx_bytes, tx_bytes))| TrafficDayBucket {
            day_key,
            rx_bytes,
            tx_bytes,
        })
        .collect::<Vec<_>>();
    prune_day_buckets(&mut days);
    days
}

fn merge_hour_buckets(items: Vec<PersistedTrafficHourBucket>) -> Vec<TrafficHourBucket> {
    let mut map = BTreeMap::<i64, (u64, u64)>::new();
    for hour in items {
        let entry = map.entry(hour.hour_key).or_insert((0, 0));
        entry.0 = entry.0.saturating_add(hour.rx_bytes);
        entry.1 = entry.1.saturating_add(hour.tx_bytes);
    }
    let mut hours = map
        .into_iter()
        .map(|(hour_key, (rx_bytes, tx_bytes))| TrafficHourBucket {
            hour_key,
            rx_bytes,
            tx_bytes,
        })
        .collect::<Vec<_>>();
    prune_hour_buckets(&mut hours);
    hours
}

fn merge_config_day_buckets(
    items: Vec<PersistedConfigTrafficDayBucket>,
    config_ids: &HashSet<u64>,
) -> HashMap<u64, Vec<TrafficDayBucket>> {
    let mut map = HashMap::<u64, BTreeMap<i32, (u64, u64)>>::new();
    for day in items {
        if !config_ids.contains(&day.config_id) {
            continue;
        }
        let entry = map
            .entry(day.config_id)
            .or_default()
            .entry(day.day_key)
            .or_insert((0, 0));
        entry.0 = entry.0.saturating_add(day.rx_bytes);
        entry.1 = entry.1.saturating_add(day.tx_bytes);
    }
    let mut out = HashMap::new();
    for (config_id, days) in map {
        let mut days = days
            .into_iter()
            .map(|(day_key, (rx_bytes, tx_bytes))| TrafficDayBucket {
                day_key,
                rx_bytes,
                tx_bytes,
            })
            .collect::<Vec<_>>();
        prune_day_buckets(&mut days);
        out.insert(config_id, days);
    }
    out
}

fn merge_config_hour_buckets(
    items: Vec<PersistedConfigTrafficHourBucket>,
    config_ids: &HashSet<u64>,
) -> HashMap<u64, Vec<TrafficHourBucket>> {
    let mut map = HashMap::<u64, BTreeMap<i64, (u64, u64)>>::new();
    for hour in items {
        if !config_ids.contains(&hour.config_id) {
            continue;
        }
        let entry = map
            .entry(hour.config_id)
            .or_default()
            .entry(hour.hour_key)
            .or_insert((0, 0));
        entry.0 = entry.0.saturating_add(hour.rx_bytes);
        entry.1 = entry.1.saturating_add(hour.tx_bytes);
    }
    let mut out = HashMap::new();
    for (config_id, hours) in map {
        let mut hours = hours
            .into_iter()
            .map(|(hour_key, (rx_bytes, tx_bytes))| TrafficHourBucket {
                hour_key,
                rx_bytes,
                tx_bytes,
            })
            .collect::<Vec<_>>();
        prune_hour_buckets(&mut hours);
        out.insert(config_id, hours);
    }
    out
}

fn prune_day_buckets(days: &mut Vec<TrafficDayBucket>) {
    days.sort_by(|a, b| a.day_key.cmp(&b.day_key));
    if days.len() > TRAFFIC_ROLLING_DAYS {
        let remove_count = days.len() - TRAFFIC_ROLLING_DAYS;
        days.drain(0..remove_count);
    }
}

fn prune_hour_buckets(hours: &mut Vec<TrafficHourBucket>) {
    hours.sort_by(|a, b| a.hour_key.cmp(&b.hour_key));
    if hours.len() > TRAFFIC_HOURLY_HISTORY {
        let remove_count = hours.len() - TRAFFIC_HOURLY_HISTORY;
        hours.drain(0..remove_count);
    }
}
