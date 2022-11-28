use chrono::DateTime;
use chrono::Datelike;
use chrono::TimeZone;
use chrono::Timelike;
use chrono::Utc;
use chrono::Weekday;
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::fmt;
use std::fs::{self, File};
use std::io::prelude::*;
use std::io::{self, ErrorKind};
use std::net::TcpStream;
use std::path::Path;
use tracing::{self, debug, error, info};

#[derive(Serialize, Deserialize)]
struct IQFeedTickMetaData {
    #[serde(with = "crate::iqfeed_date_time")]
    min_date_time: DateTime<Tz>,

    #[serde(with = "crate::iqfeed_date_time")]
    max_date_time: DateTime<Tz>,
}

#[derive(Debug, Clone)]
struct IQFeedNoDataError;

impl fmt::Display for IQFeedNoDataError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "no data from iqfeed")
    }
}

impl Error for IQFeedNoDataError {}

// call iqfeed for ticks
pub fn iqfeed_ticks(symbol: &str, out_dir: &str, no_mkt_hours: bool) -> Result<(), Box<dyn Error>> {
    let out_dir_path = Path::new(out_dir).join(symbol);
    fs::create_dir_all(out_dir_path.to_str().unwrap())?;
    let now_dt = Utc::now().with_timezone(&New_York);
    let file_name = format!("{}.csv", now_dt.format("%Y-%m-%d-%H-%M-%S"));
    let out_path = out_dir_path.join(file_name);
    let out_file = File::create(&out_path)?;
    let meta_out_path = out_dir_path.join("meta.toml");
    let meta_content = fs::read_to_string(&meta_out_path).unwrap_or_else(|err| {
        if err.kind() == ErrorKind::NotFound {
            return String::new();
        }
        panic!("{}", err)
    });

    // _date_time strings for logging
    let mut min_date_time = String::new();
    let mut max_date_time = String::new();

    let mut meta_cfg: IQFeedTickMetaData;
    if !meta_content.is_empty() {
        meta_cfg = toml::from_str(&meta_content)?;
        min_date_time = format!("{}", meta_cfg.min_date_time.format("%Y%m%d %H%M%S"));
        max_date_time = format!("{}", meta_cfg.max_date_time.format("%Y%m%d %H%M%S"));
    } else {
        let naive_dt = Utc::now().naive_utc();
        let ny_dt = New_York.from_utc_datetime(&naive_dt);
        if !no_mkt_hours && ny_dt.weekday() != Weekday::Sat
            && ny_dt.weekday() != Weekday::Sun
            && ny_dt.hour() > 9 // todo: technically 9:30, but whatever
            && ny_dt.hour() < 16
        {
            return Err("Due to limited history, ticks should not be gathered \
                        for new symbols during NYC market hours."
                .into());
        }
        meta_cfg = IQFeedTickMetaData {
            min_date_time: now_dt,
            max_date_time: now_dt,
        }
    }

    info!(
        min_date_time = min_date_time.as_str(),
        max_date_time = max_date_time.as_str(),
        out_file = out_path.to_str().unwrap(),
        symbol = ?symbol,
        "Downloading iqfeed ticks"
    );

    let mut stream = TcpStream::connect("127.0.0.1:9100")?;
    stream.write_all("S,SET PROTOCOL,5.1\r\n".as_bytes())?;

    debug!(
        request = format!("HTT,{},{},{},,,,1,{}\r\n", symbol, max_date_time, "", 1).as_str(),
        "Issuing request",
    );

    // historical tick request
    // HTT,[Symbol],[BeginDate BeginTime],[EndDate EndTime],[MaxDatapoints],[BeginFilterTime],[EndFilterTime],[DataDirection],[RequestID],[DatapointsPerSend]<CR><LF>
    stream
        .write_all(format!("HTT,{},{},{},,,,1,{}\r\n", symbol, max_date_time, "", 1).as_bytes())?;
    let mut lines = io::BufReader::new(stream).lines();
    let mut out_file_buf = io::BufWriter::new(out_file);

    // First line is S,CURRENT_PROTOCOL,5.1
    // Discard
    let _current_proto_header = lines.next();
    writeln!(out_file_buf, "request_id,date_time,last,last_size,total_volume,bid,ask,tick_id,basis_for_last,trade_market_center,trade_conditions,trade_aggressor")?;

    let mut n_ticks = 0;

    for line_res in lines {
        let line = line_res?;
        let v: Vec<&str> = line.split(',').collect();
        if &v[1].to_owned() == "E" {
            error!(error = v[2], "IQFeed sent back an error");
            drop(out_file_buf);
            if n_ticks == 0 {
                fs::remove_file(out_path)?;
            }
            return Err(Box::new(IQFeedNoDataError));
        }
        if &v[1].to_owned() == "!ENDMSG!" {
            break;
        }
        let tick_date_time = crate::iqfeed_date_time::parse(&v[1].to_owned())?;
        if tick_date_time > meta_cfg.max_date_time {
            meta_cfg.max_date_time = tick_date_time;
            out_file_buf.write_all(line.as_bytes())?;
            out_file_buf.write_all(b"\n")?;
            n_ticks += 1;
        }
        if tick_date_time < meta_cfg.min_date_time {
            meta_cfg.min_date_time = tick_date_time;
        }
    }

    out_file_buf.flush()?;
    fs::write(&meta_out_path, toml::to_string(&meta_cfg)?)?;

    // maybe we didn't get any new ticks after all,
    // if so, clean up the file
    if n_ticks == 0 {
        fs::remove_file(out_path)?;
    }

    info!(n_ticks = n_ticks, "Finished writing ticks");

    Ok(())
}
