use chrono::offset::TimeZone;
use chrono::DateTime;
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use serde::de::{self};
use serde::{Deserialize, Deserializer, Serializer};
use std::error::Error;

pub const FORMAT: &str = "%Y-%m-%d %H:%M:%S.%f";

pub fn parse(s: &str) -> Result<DateTime<Tz>, Box<dyn Error>> {
    let year = s[..4].parse::<i32>()?;
    let month = s[5..7].parse::<u32>()?;
    let day = s[8..10].parse::<u32>()?;
    let hour = s[11..13].parse::<u32>()?;
    let minute = s[14..16].parse::<u32>()?;
    let second = s[17..19].parse::<u32>()?;
    let milli = s[20..23].parse::<u32>()?;
    Ok(New_York
        .ymd(year, month, day)
        .and_hms_milli(hour, minute, second, milli))
}

pub fn serialize<S>(date: &DateTime<Tz>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let s = format!("{}", date.format(FORMAT));
    serializer.serialize_str(&s)
}

pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Tz>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse(&s).map_err(de::Error::custom)
}
