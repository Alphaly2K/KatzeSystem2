use chrono::{Datelike, Timelike};

pub(super) fn local_time() -> [u16; 7] {
    let now = chrono::Local::now();
    [
        now.year() as u16,
        now.month() as u16,
        now.weekday().num_days_from_sunday() as u16,
        now.day() as u16,
        now.hour() as u16,
        now.minute() as u16,
        now.second() as u16,
    ]
}

pub(super) fn day_of_week(year: i32, month: i32, day: i32) -> i32 {
    let (year, month) = if month > 2 {
        (year, month.wrapping_add(1))
    } else {
        (year.wrapping_sub(1), month.wrapping_add(13))
    };
    let truncate = |value: f64| {
        if (-2147483648.0..2147483648.0).contains(&value) {
            value as i32
        } else {
            i32::MIN
        }
    };
    let days = truncate(f64::from(year) * 365.25)
        .wrapping_sub(truncate(f64::from(month) * -30.6))
        .wrapping_add(year / 400)
        .wrapping_sub(year / 100)
        .wrapping_add(day.wrapping_sub(429));
    days.wrapping_add(1)
        .wrapping_sub((days / 7).wrapping_mul(7))
        % 7
}
