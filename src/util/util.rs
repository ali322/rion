use block_id::{Alphabet, BlockId};
use chrono::{Duration, Local, NaiveDateTime};
use uuid::Uuid;

pub fn default_expire() -> NaiveDateTime {
    Local::now().naive_local() + Duration::days(30)
}

pub fn uuid_v4() -> String {
    Uuid::new_v4().to_string()
}

pub fn short_id(c: u64) -> String {
    let seed = 9876;
    let length = 6;
    let generator = BlockId::new(Alphabet::alphanumeric(), seed, length);
    let id = generator.encode_string(c).unwrap();
    return id;
}
