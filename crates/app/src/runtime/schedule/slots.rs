use chrono::Duration;

use super::time::UtcInterval;

pub(super) fn merge_intervals(mut values: Vec<UtcInterval>) -> Vec<UtcInterval> {
    values.retain(|value| value.start < value.end);
    values.sort_by_key(|value| value.start);
    let mut output: Vec<UtcInterval> = Vec::new();
    for value in values {
        if let Some(last) = output.last_mut()
            && value.start <= last.end
        {
            last.end = std::cmp::max(last.end, value.end);
            continue;
        }
        output.push(value);
    }
    output
}

pub(super) fn clip_to_working(interval: UtcInterval, working: &[UtcInterval]) -> Vec<UtcInterval> {
    working.iter().filter_map(|work| intersect(interval, *work)).collect()
}

pub(super) fn intersect_all(groups: &[Vec<UtcInterval>]) -> Vec<UtcInterval> {
    let Some(first) = groups.first() else {
        return Vec::new();
    };
    let mut common = first.clone();
    for group in groups.iter().skip(1) {
        common = intersect_groups(&common, group);
        if common.is_empty() {
            break;
        }
    }
    common
}

pub(super) fn fitting(
    values: Vec<UtcInterval>,
    duration_minutes: u16,
    limit: usize,
) -> Vec<UtcInterval> {
    let duration = Duration::minutes(i64::from(duration_minutes));
    values
        .into_iter()
        .filter(|value| value.end.signed_duration_since(value.start) >= duration)
        .take(limit)
        .collect()
}

fn intersect_groups(left: &[UtcInterval], right: &[UtcInterval]) -> Vec<UtcInterval> {
    let mut output = Vec::new();
    for first in left {
        for second in right {
            if let Some(value) = intersect(*first, *second) {
                output.push(value);
            }
        }
    }
    merge_intervals(output)
}

fn intersect(left: UtcInterval, right: UtcInterval) -> Option<UtcInterval> {
    let start = std::cmp::max(left.start, right.start);
    let end = std::cmp::min(left.end, right.end);
    (start < end).then_some(UtcInterval { start, end })
}
