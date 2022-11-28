use chrono::prelude::Local;
use chrono::{DateTime, Duration, DurationRound, Timelike, Utc};
use chrono_tz::America::New_York;
use chrono_tz::Tz;
use serde::Deserialize;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
struct IQFeedTick {
    #[serde(with = "crate::iqfeed_date_time")]
    date_time: DateTime<Tz>,
    request_id: u32,
    last: f64,
    last_size: f64,
    total_volume: f64,
    bid: f64,
    ask: f64,
    tick_id: u64,
    basis_for_last: String,
    trade_market_center: u32,
    trade_conditions: String,
    trade_aggressor: String,
}

#[derive(Copy, Clone)]
pub enum Timestamp {
    IQFeed,
    Unix,
}

pub struct BarOptions<'o> {
    pub delimiter: String,
    pub multiply: f64,
    pub symbol: &'o String,
    pub timestamp_index: usize,
    pub last_index: usize,
    pub volume_index: usize,
    pub timestamp_type: Timestamp,
    pub dollar_threshold: f64,
}

fn list_tick_files(in_dir_path: PathBuf) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut tick_files = fs::read_dir(in_dir_path)?
        .filter_map(|d| {
            d.ok().and_then(|f| {
                if f.path().to_str().unwrap().ends_with(".csv") {
                    Some(f.path())
                } else {
                    None
                }
            })
        })
        .collect::<Vec<PathBuf>>();
    tick_files.sort_by(|a, b| {
        let a_meta = fs::metadata(a).unwrap();
        let b_meta = fs::metadata(b).unwrap();
        a_meta.created().unwrap().cmp(&b_meta.created().unwrap())
    });
    Ok(tick_files)
}

pub fn time_bars(symbol: &str, interval: &str) -> Result<(), Box<dyn Error>> {
    let (mut open, mut high, mut low, mut cumulative_dollar, mut cumulative_volume) =
        (0.0, 0.0, 0.0, 0.0, 0.0);

    // TODO: These defaults are bugs waiting to happen. (e.g., what if price == 0)
    // Need to figure out a better approach for this, and write some tests.
    let mut bartime: DateTime<Tz> = chrono::MIN_DATETIME.with_timezone(&New_York);
    let parsed_interval = interval.parse::<u32>().unwrap();
    let mut last_printed_minute = 0;

    let out_dir_path = Path::new("bars").join(symbol);
    let in_dir_path = Path::new("ticks").join(symbol);

    fs::create_dir_all(out_dir_path.to_str().unwrap())?;
    let now_dt = Utc::now().with_timezone(&New_York);
    let file_name = format!("{}.csv", now_dt.format("time-%Y-%m-%d-%H-%M-%S"));
    let out_path = out_dir_path.join(file_name);
    let mut out_file = File::create(&out_path)?;
    info!(
        out_file = out_path.to_str().unwrap(),
        interval = interval,
        "Sampling time bars"
    );
    writeln!(out_file, "date_time,open,high,low,close,volume,cum_dollars")?;
    let tick_files = list_tick_files(in_dir_path)?;
    for csv_file in tick_files {
        let file = File::open(csv_file)?;
        let mut rdr = csv::Reader::from_reader(file);
        let mut tick = csv::ByteRecord::new();
        while rdr.read_byte_record(&mut tick)? {
            let date_time_str = String::from_utf8_lossy(&tick[1]).as_ref().to_owned();
            let date_time = crate::iqfeed_date_time::parse(&date_time_str)?;
            let minute = date_time.minute();
            let last = String::from_utf8_lossy(&tick[2]).parse::<f64>()?;
            if open == 0.0 {
                open = last;
                high = last;
                low = last;
                bartime = date_time;
            }
            let volume = String::from_utf8_lossy(&tick[3]).parse::<f64>()?;
            cumulative_volume += volume;
            cumulative_dollar += last * volume;
            if last < low {
                low = last;
            }
            if last > high {
                high = last;
            }
            let close = last;
            if minute % parsed_interval == 0 && minute != last_printed_minute {
                writeln!(
                    // TODO: fix timestamp, it should be open TS not close
                    out_file,
                    "{},{},{},{},{},{},{}",
                    bartime
                        .duration_round(Duration::minutes(15))?
                        .format("%Y-%m-%d %H:%M:%S"),
                    open,
                    high,
                    low,
                    close,
                    cumulative_volume,
                    cumulative_dollar
                )?;
                open = 0.0;
                high = 0.0;
                low = 0.0;
                cumulative_dollar = 0.0;
                cumulative_volume = 0.0;
                last_printed_minute = minute;
            }
        }
    }
    Ok(())
}

pub fn dollar_bars(opts: &BarOptions) -> Result<(), Box<dyn Error>> {
    let (mut open, mut high, mut low, mut close, mut cumulative_dollar, mut cumulative_volume) =
        (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    let mut bar_open_time = String::from("");
    let mut prev_tick_timestamp = Vec::new();
    let out_dir_path = Path::new("bars").join(opts.symbol);
    let in_dir_path = Path::new("ticks").join(opts.symbol);

    info!(
        out_dir_path = out_dir_path.to_str().unwrap(),
        in_dir_path = in_dir_path.to_str().unwrap(),
        symbol = opts.symbol.as_str(),
        "Processing ticks into bars"
    );
    fs::create_dir_all(out_dir_path.to_str().unwrap())?;
    let now_dt = Utc::now().with_timezone(&New_York);
    let file_name = format!("{}.csv", now_dt.format("dollar-%Y-%m-%d-%H-%M-%S"));
    let out_path = out_dir_path.join(file_name);
    let mut out_file = File::create(&out_path)?;
    writeln!(out_file, "date_time,open,high,low,close,volume,cum_dollars")?;
    info!(
        out_file = out_path.to_str().unwrap(),
        "Sampling dollar bars"
    );
    let tick_files = list_tick_files(in_dir_path)?;
    for csv_file in tick_files {
        let file = File::open(&csv_file)?;
        let mut rdr = csv::ReaderBuilder::new()
            .delimiter(opts.delimiter.as_bytes()[0])
            .from_reader(file);
        let mut tick = csv::ByteRecord::new();
        let mut new_bar = true;

        while rdr.read_byte_record(&mut tick)? {
            let last = String::from_utf8_lossy(&tick[opts.last_index]).parse::<f64>()?;
            if new_bar {
                bar_open_time = String::from_utf8_lossy(&tick[opts.timestamp_index]).to_string();
                open = last;
                high = last;
                low = last;
                cumulative_dollar = 0.0;
                cumulative_volume = 0.0;
                new_bar = false;
            }
            let volume = String::from_utf8_lossy(&tick[opts.volume_index]).parse::<f64>()?;
            cumulative_volume += volume;
            cumulative_dollar += last * volume * opts.multiply;
            if last < low {
                low = last;
            }
            if last > high {
                high = last;
            }
            close = last;

            // Need to check that open time is not the exact same as this tick's
            // time, since sometimes orders of huge size come in at pretty much
            // exactly the same time.
            if cumulative_dollar >= opts.dollar_threshold
                && prev_tick_timestamp != tick[opts.timestamp_index]
            {
                writeln!(
                    out_file,
                    "{},{},{},{},{},{},{}",
                    bar_open_time, open, high, low, close, cumulative_volume, cumulative_dollar
                )?;
                new_bar = true;
            }
            prev_tick_timestamp = tick[opts.timestamp_index].to_vec();
        }
    }

    writeln!(
        out_file,
        "{},{},{},{},{},{},{}",
        bar_open_time, open, high, low, close, cumulative_volume, cumulative_dollar
    )?;

    // clean up old bar files
    for d in fs::read_dir(out_dir_path)?.flatten() {
        if let Ok(metadata) = fs::metadata(d.path()) {
            let modtime = metadata.modified().expect("Modtime supported");
            let timedelta = Local::now() - DateTime::from(modtime);
            if d.path() != out_path && timedelta > Duration::minutes(5) {
                fs::remove_file(d.path())?;
            }
        } else {
            error!("Couldn't delete bars, metadata not supported");
        }
    }

    Ok(())
}
