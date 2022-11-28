mod bars;
mod iqfeed_date_time;
mod ticks;

use chrono::{DateTime, Duration};
use chrono_tz::Tz;
use clap::{App, Arg};
use serde::Deserialize;
use std::env;
use std::error::Error;
use std::fmt;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::ErrorKind;
use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;
use tracing::Level;
use tracing::{self, debug, error, info};

#[derive(Debug, Deserialize)]
struct Bar {
    #[serde(with = "iqfeed_date_time")]
    date_time: DateTime<Tz>,
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}
#[derive(Debug)]
struct ProcessingError {
    errs: Vec<Box<dyn Error>>,
} // aggregates multiple errors in processing into one

impl fmt::Display for ProcessingError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self.errs)
    }
}

fn nan_or_val(x: Option<f64>) -> String {
    match x {
        Some(x) => format!("{}", x),
        None => String::from("NaN"),
    }
}

fn daily_vol(csv_path: &str) -> Result<(), Box<dyn Error>> {
    let file = File::open(csv_path)?;
    let lookback = 20;
    let lookback_f64 = lookback as f64;
    let smoothing = 2.;
    let mut n_days = 1;
    let mut ewma = None;
    let mut rdr = csv::Reader::from_reader(file);
    let mut bars = rdr.deserialize();
    let first_bar: Bar = bars.next().unwrap()?;
    let mut day_cur: DateTime<Tz> = first_bar.date_time;
    let mut price_cur = first_bar.close;
    let mut sma_sum = 0.;
    let mut ewma_daily_vols: Vec<Option<f64>> = Vec::new();
    println!("start_date_time,end_date_time,return,ewma");

    for result in bars {
        let bar: Bar = result?;
        if bar.date_time.signed_duration_since(day_cur) > Duration::days(1) {
            n_days += 1;
            let ret = (bar.close / price_cur) - 1.;
            if n_days > lookback {
                if n_days == lookback + 1 {
                    // use sma to "bootstrap" ewma
                    ewma = Some(sma_sum / n_days as f64);
                }
                // TODO: add ewma() and std() helper functions, they won't
                // affect time complexity which is mainly what we're concerned about,
                // and will be useful in other places
                ewma = Some(
                    (ret * (smoothing / (1. + lookback_f64)))
                        + (ewma.unwrap() * (1. - (smoothing / (1. + lookback_f64)))),
                );
                ewma_daily_vols.push(ewma);
            } else {
                sma_sum += ret;
            }
            println!("{},{},{},{}", day_cur, bar.date_time, ret, nan_or_val(ewma),);
            day_cur = bar.date_time;
            price_cur = bar.close;
        }
    }

    Ok(())
}

fn symbol_lookup(query: &str) -> Result<(), Box<dyn Error>> {
    let mut stream = match TcpStream::connect("127.0.0.1:9100") {
        Ok(s) => s,
        Err(e) => return Err(e.into()),
    };
    let filter_type = "";
    let filter_value = "";
    match stream.write(
        format!(
            // http://www.iqfeed.net/dev/api/docs/SymbolLookupviaTCPIP.cfm
            "S,SET PROTOCOL,6.2\r
SBF,d,{},{},{},1\r\n",
            query, filter_type, filter_value
        )
        .as_bytes(),
    ) {
        Ok(_) => {}
        Err(e) => return Err(e.into()),
    };
    let mut lines = BufReader::new(stream).lines();

    // skip first proto 'header' response
    lines.next();

    for line_res in lines {
        let line = line_res?;
        if line.contains("!ENDMSG!") {
            break;
        }
        println!("{}", str::replace(&line, ",", "\t"));
    }

    Ok(())
}

fn check_iqfeed_health() -> i32 {
    let (sender, receiver) = mpsc::channel();
    let timeout_sender = sender.clone();
    let _main = thread::spawn(move || {
        let mut stream = match TcpStream::connect("127.0.0.1:9100") {
            Ok(s) => s,
            Err(e) => {
                sender.send(Err(e)).unwrap();
                return;
            }
        };
        match stream.write("S,SET PROTOCOL,5.1\r\nHTT,NONSENSE_SYMBOL,,,,,,1,\r\n\r\n".as_bytes()) {
            Ok(_) => {}
            Err(e) => {
                sender.send(Err(e)).unwrap();
                return;
            }
        };
        let mut lines = BufReader::new(stream).lines();

        // should get back a response
        // S,CURRENT PROTOCOL,5.1
        if let Some(line) = lines.next() {
            match line {
                Ok(line) => {
                    info!("Got a response: {:?}", line);
                    if line != "S,CURRENT PROTOCOL,5.1" {
                        sender
                            .send(Err(std::io::Error::new(
                                ErrorKind::InvalidData,
                                "Got a different response than expected",
                            )))
                            .unwrap();
                        return;
                    }
                }
                Err(e) => {
                    sender.send(Err(e)).unwrap();
                    return;
                }
            }
        }
        sender.send(Ok(())).unwrap();
    });
    let _timeout = thread::spawn(move || {
        thread::sleep(std::time::Duration::from_millis(5000));
        if let Ok(()) = timeout_sender.send(Err(std::io::Error::new(
            ErrorKind::TimedOut,
            "timeout trying to connect to IQFeed",
        ))) {}
    });
    return match receiver.recv() {
        Ok(msg) => match msg {
            Ok(_) => 0,
            Err(e) => {
                error!("{:?}", e);
                2 // nomad failed check
            }
        },
        Err(e) => {
            error!("{:?}", e);
            2
        }
    };
}

fn main() {
    let start = SystemTime::now();
    let sentry_url = env::var("SENTRY_URL").unwrap();
    let _guard = sentry::init((
        sentry_url,
        sentry::ClientOptions {
            release: sentry::release_name!(),
            ..Default::default()
        },
    ));

    let mut app = App::new("Feat CLI")
        .version("0.0.1")
        .author("Nathan LeClaire <nathan.leclaire@gmail.com>")
        .about("Time series data processing tool")
        .arg(Arg::new("debug").long("debug").takes_value(false))
        .subcommand(
            App::new("ticks")
                .about("Gets ticks from data providers")
                .arg(Arg::new("symbol").required(true))
                .arg(Arg::new("output_dir").default_value("ticks"))
                .arg(
                    Arg::new("no_mkt_hours")
                        .long("no_mkt_hours")
                        .takes_value(false),
                ),
        )
        .subcommand(
            App::new("lookup")
                .about("Find a symbol")
                .arg(Arg::new("query").required(true)),
        )
        .subcommand(
            App::new("bars")
                .about("Gets bars from ticks")
                .arg(Arg::new("multiply").long("multiply").default_value("1."))
                .arg(Arg::new("delimiter").long("delimiter").default_value(","))
                .arg(
                    Arg::new("timestamp_index")
                        .long("timestamp_index")
                        .default_value("1"),
                )
                .arg(Arg::new("last_index").long("last_index").default_value("2"))
                .arg(
                    Arg::new("volume_index")
                        .long("volume_index")
                        .default_value("3"),
                )
                .arg(
                    Arg::new("timestamp_type")
                        .long("timestamp_type")
                        .default_value("string"),
                )
                .arg(Arg::new("bar_type").required(true))
                .arg(Arg::new("symbol").required(true)),
        )
        .subcommand(
            App::new("vol")
                .about("Gets daily volatility from bars")
                .arg(Arg::new("input_file").required(true)),
        )
        .subcommand(App::new("check").about("Check iqfeed health"));
    let matches = app.get_matches_mut();
    let debug = matches.is_present("debug");

    let mut subscriber = tracing_subscriber::fmt().with_ansi(env::consts::OS != "windows"); // term lib has issues w/ Windows
    if debug {
        subscriber = subscriber.with_max_level(Level::DEBUG);
    }
    tracing::subscriber::set_global_default(subscriber.finish())
        .map_err(|_err| eprintln!("Unable to set global default subscriber"))
        .unwrap();

    let err = match matches.subcommand_name() {
        Some("bars") => {
            let subcmd_matches = matches.subcommand_matches("bars").unwrap();
            let bar_type = subcmd_matches.value_of("bar_type");
            let symbol = subcmd_matches.value_of("symbol").unwrap();
            let multiply = match subcmd_matches.value_of("multiply") {
                Some(x) => x.to_owned().parse::<f64>().unwrap(),
                None => 1.,
            };
            let timestamp_index = match subcmd_matches.value_of("timestamp_index") {
                Some(x) => x.to_owned().parse::<usize>().unwrap(),
                None => 1,
            };
            let last_index = match subcmd_matches.value_of("last_index") {
                Some(x) => x.to_owned().parse::<usize>().unwrap(),
                None => 2,
            };
            let volume_index = match subcmd_matches.value_of("volume_index") {
                Some(x) => x.to_owned().parse::<usize>().unwrap(),
                None => 3,
            };
            let timestamp_type = match subcmd_matches.value_of("timestamp_type") {
                Some(x) => match x {
                    "unix" => bars::Timestamp::Unix,
                    &_ => bars::Timestamp::IQFeed,
                },
                None => bars::Timestamp::IQFeed,
            };
            let delimiter = subcmd_matches.value_of("delimiter").unwrap_or(",");
            if symbol.ends_with(".txt") {
                let symbol_file = File::open(symbol).unwrap();
                let lines = BufReader::new(symbol_file).lines();
                let errs = lines
                    .map(|line| match bar_type {
                        Some("time") => bars::time_bars(&line.unwrap(), &String::from("15")),
                        Some("dollar") => {
                            let opts = bars::BarOptions {
                                delimiter: String::from(delimiter),
                                symbol: &line.unwrap(),
                                dollar_threshold: 7000000.0,
                                multiply,
                                timestamp_index,
                                last_index,
                                volume_index,
                                timestamp_type,
                            };
                            bars::dollar_bars(&opts)
                        }
                        None => panic!("Must specify bar_type"),
                        _ => panic!("Must specify bar_type"),
                    })
                    .filter(|res| res.is_err())
                    .flat_map(Err)
                    .collect::<Vec<Box<dyn Error>>>();
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(ProcessingError { errs })
                }
            } else {
                let opts = bars::BarOptions {
                    delimiter: String::from(delimiter),
                    symbol: &symbol.to_owned(),
                    dollar_threshold: 7000000.0,
                    multiply,
                    timestamp_index,
                    last_index,
                    volume_index,
                    timestamp_type,
                };
                let res = match bar_type {
                    Some("time") => bars::time_bars(&symbol.to_owned(), &String::from("15")),
                    Some("dollar") => bars::dollar_bars(&opts),
                    None => panic!("Must specify bar_type"),
                    _ => panic!("Must specify bar_type"),
                };
                match res {
                    Ok(_) => Ok(()),
                    Err(e) => Err(ProcessingError { errs: vec![e] }),
                }
            }
        }
        Some("vol") => {
            let input_file = matches.value_of("input_file").unwrap();
            match daily_vol(&input_file.to_owned()) {
                Ok(_) => Ok(()),
                Err(e) => Err(ProcessingError { errs: vec![e] }),
            }
        }
        Some("ticks") => {
            let subcmd_matches = matches.subcommand_matches("ticks").unwrap();
            let symbol = subcmd_matches.value_of("symbol").unwrap();
            let output_dir = subcmd_matches.value_of("output_dir").unwrap();
            let no_mkt_hours = subcmd_matches.is_present("no_mkt_hours");

            if check_iqfeed_health() != 0 {
                panic!("No iqfeed connection")
            }

            if symbol.ends_with(".txt") {
                let symbol_file = File::open(symbol).unwrap();
                let lines = BufReader::new(symbol_file).lines();
                let errs = lines
                    .map(|line| {
                        debug!(line = ?line.as_ref().unwrap().clone(), output_dir = ?output_dir, "calling iqfeed ticks");
                        ticks::iqfeed_ticks(&line.unwrap(), &output_dir.to_owned(), no_mkt_hours)
                    })
                    .filter(|res| res.is_err())
                    .flat_map(Err)
                    .collect::<Vec<Box<dyn Error>>>();
                if errs.is_empty() {
                    Ok(())
                } else {
                    Err(ProcessingError { errs })
                }
            } else {
                match ticks::iqfeed_ticks(&symbol.to_owned(), &output_dir.to_owned(), no_mkt_hours)
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(ProcessingError { errs: vec![e] }),
                }
            }
        }
        Some("check") => {
            std::process::exit(check_iqfeed_health());
        }
        Some("lookup") => {
            let subcmd_matches = matches.subcommand_matches("lookup").unwrap();
            let query = subcmd_matches.value_of("query").unwrap();
            match symbol_lookup(query) {
                Ok(_) => Ok(()),
                Err(e) => Err(ProcessingError { errs: vec![e] }),
            }
        }
        _ => {
            app.print_help().unwrap();
            std::process::exit(1);
        }
    };

    std::process::exit(match err {
        Err(err) => {
            error!(error = format!("{}", err).as_str(), "Something went wrong");
            1
        }
        Ok(_) => {
            info!(seconds = start.elapsed().unwrap().as_secs(), "Finished all");
            0
        }
    });
}
