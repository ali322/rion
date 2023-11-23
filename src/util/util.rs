use chrono::{Duration, Local, NaiveDateTime};
use uuid::Uuid;

pub fn default_expire() -> NaiveDateTime {
    Local::now().naive_local() + Duration::days(30)
}

pub fn uuid_v4() -> String {
    Uuid::new_v4().to_string()
}
