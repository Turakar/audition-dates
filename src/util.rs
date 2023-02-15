use chrono::{DateTime, TimeZone};

pub fn datetime_to_day<TZ: TimeZone>(datetime: DateTime<TZ>) -> DateTime<TZ> {
    let timezone = datetime.timezone();
    let result = datetime
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_local_timezone(timezone);
    match result {
        chrono::LocalResult::None => panic!(),
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(dt, _) => dt,
    }
}
