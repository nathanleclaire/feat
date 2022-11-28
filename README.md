# feat

![feat logo, a warrior holding a sword and shield](https://featcmd.com/_images/feat.png)

Feat (short for "feature") is a program for ingesting financial time series data
(e.g., trades from an exchange) and using that data to generate samples and
features for research, backtesting, and developing machine learning models.  It
is written in Rust and optimized for performance, enabling researchers to move
faster on experimenting with new ideas, or simply to generate input data for
existing models more quickly.

It is inspired by the work of [Lopez de Prado](https://www.quantresearch.org/)
and [mlfinlab](https://github.com/hudson-and-thames/mlfinlab).

## Intro

Research and prototyping are often performed in scripting languages Python and
R, but these languages are designed for fast iteration, not performance
ingesting and processing data. Processing large quantities of data, such as
those that relate to [market
microstructure](https://en.wikipedia.org/wiki/Market_microstructure), is a
bottleneck that slows down research and production predictions, as generating
the final samples from scratch can take hours to complete. While many vendors
support accessing aggregated price and volume data in the form of traditional
OHLC candlestick bars, these so-called "time bars" are known to have [less
desirable statistical
properties](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=3257419) than
some of the alternatives.

Additionally, many vendors who provide programmatic access to the source data
have limited history available. Feat is designed to always capture the latest
data so that aged out underlying data is still available even when the window
of available history shifts forward. It has support for pulling such data to a
local machine using data providers' APIs as well as further refining it into
samples useful for analysis.

Feat can also be configured to work on data from providers it has not been
precoded to support, such as a historical tick data CSV with arbitrary field
names. In the [Future](/future), we want to expand Feat to rapidly generate
data that is useful for forecasting models but can be slow to generate, such as
[autocorrelation](https://en.wikipedia.org/wiki/Autocorrelation), [bubble
tests](https://en.wikipedia.org/wiki/Augmented_Dickey%E2%80%93Fuller_test),
[triple barrier
labels](https://towardsdatascience.com/the-triple-barrier-method-251268419dcd),
and more.

## Benchmarks

Fast is easy to claim. It's harder to back up with data.

Here is one example of Feat processing dollar bars from a BTCUSDT Binance trade
data CSV from [CryptoArchive](https://www.cryptoarchive.com.au/). This test was
executed on a [t3a.xlarge
instance](https://aws.amazon.com/ec2/instance-types/t3/) (with 4 vCPUs and 16GB
memory) on AWS, with the source file on the standard root block device (EBS).
The bars are sampled at a rate of $7,000,000 per bar.

``` $ feat bars dollar \ BTCUSDT \ --timestamp_index 3 \ --last_index 1 \
--volume_index 2 \ --delimiter "|" INFO feat::bars: Processing ticks into bars
out_dir_path="bars/BTCUSDT" in_dir_path="ticks/BTCUSDT" symbol="BTCUSDT" INFO
feat::bars: Sampling dollar bars
out_file="bars/BTCUSDT/dollar-2021-09-12-17-11-39.csv" INFO feat: Finished all
seconds=399 ```

The source file is about 43GB large and it takes ~6.5 minutes to process the
dollar bars. For comparison, `wc -l` on the tick file takes ~4.5 minutes.

## Data

### Pulling Market Data

Currently, Feat has support for ingesting tick data from [DTN
IQFeed](https://www.iqfeed.net/), an affordable yet performant and reliable
source for traditional finance data.

### DTN IQFeed

Use DTN's [symbol
guide](https://www.iqfeed.net/symbolguide/index.cfm?symbolguide=lookup&displayaction=support&section=guide&web=iqfeed)
to locate the symbol you are interested in. DTN has support for equities,
futures, options, and more but you may need to ensure that the correct
permissions are enabled in your account.

Once you have the symbol(s) you are interested in, you can use the `feat ticks`
command to ingest the data for it. This data will appear in a new
`$WORKDIR/ticks/$SYMBOL` directory. On future runs, Feat will remember the date
of the latest ingested trade and pull only the data since this time. The
filenames are timestamps based on when the pull occurred.

### Single Symbol

```
$ feat ticks @ES#C
```

### Multiple Symbols

If you pass a filename with the suffix `.txt`, with one symbol per line, `feat
ticks` will pull the latest tick data for each of those symbols.

For instance, `syms.txt` might look like:

```
AMD
F
TSLA
GME
```

And the subsequent invocation:

```
$ feat ticks syms.txt
```

Once ticks are ingested, bars can be processed from them.

## Bars

# Processing Bars

Traditional OHLC bars in finance are based on the passage of time, but from
individual tick level data, [other types of
bars](https://quant.stackexchange.com/questions/43534/volume-or-dollar-bars-vs-volatility-normalized-and-demeaned-financial-time-seri)
can be generated as well, such as tick bars that are sampled when a certain
number of trades have completed, or dollar bars that are sampled once a certain
amount of money has changed hands. That way, bars can be sampled as new
information arrives to the market, instead of simply when the passage of time
has occurred.

Feat has support for processing the downloaded ticks into bars to then be fed
into further analysis or financial machine learning.

Feat will look for a directory named `ticks` in the current working directory
to locate the tick files to be processed into bars, for instance, `ticks/TSLA`.

You can also pass a file with suffix `.txt` with a symbol on each line to
process bars for multiple symbols.

## Dollar Bars

To process dollar bars:

```
$ feat bars dollar TSLA
```

The current threshold to sample a bar is $7mm. This can be configured but
support for configuring it hasn't been added yet.

## Time Bars

Feat can also process 15 minute time bars.

```
$ feat bars time @ES#C
```

## Custom Data Formats

Not every downloaded format conforms exactly to the ones generated by Feat when
pulling from IQFeed, for instance, the index of the date/time field might be
different, or the file might even be delimited with a different character than
commas.

Feat has limited support for processing these files too. You can use these
flags for the `bars` command to instruct Feat how to process them:

- `--timestamp_index` - the numeric index of the datetime field
- `--last_index` - the numeric index of the price traded for that tick
- `--volume_index` - the numeric index of how much volume was traded for that tick
- `--delimiter` - the character used for CSV delimiting (default: `,`)

e.g., if the individual lines looked like this:

```
1|65600.0|0.15|2021-11-11 00:00:00.123
```

then the command would be along these lines:

```
$ feat bars dollar \
    BTCUSDT \
    --timestamp_index 3 \
    --last_index 1 \
    --volume_index 2 \
    --delimiter "|"
```

## Future

# Ideas and Future Directions

Feat is currently in alpha status (no pun intended) and what comes next for the
project is still being determined. If there's something you'd like to see, give
us a shout!

Here are some ideas we are evaluating for working on next.

## Realtime/Performance

Generating data for training and experimentation is neccessary but not
sufficient. Models deployed to production must have the most recent data
possible in order to be useful, and being able to generate samples and features
more quickly in production opens up more possibilities for trading.

To that end, one direction we'd like to eventually pursue with Feat is to
enable a streaming mode that will ingest and process data continuously. For
instance, it could use Kafka brokers as intermediaries instead of files,
emitting new samples and features constantly.

Likewise, CSV is a very inefficient format to use, but we started with it
because it is common, easy to inspect, and because many data providers
distribute their historical data in CSV. Feat could be updated to support more
formats, including binary protocols such as msgpack for [additional improved
performance](https://msgpack.org/index.html). This could also improve
performance when loading data into, say,
[Pandas](https://towardsdatascience.com/the-best-format-to-save-pandas-data-414dca023e0d).

## Python Bindings

While a command line is a good first step, we'd like folks to be able to use
Feat as transparently as possible within their existing code and
infrastructure. To that end, we might like to explore adding support for Python
bindings so that the high performance Rust code can be leveraged without
needing to change existing pipelines or shell out to the command line.

## More Sample Types

Currently Feat only supports generating dollar bars from the underlying data.
We reasoned that this was a good first step, since they are straightforward to
produce, while still being more desirable than good old fashioned time bars.

However, dollar bars are only the beginning. de Prado outlines other bar types
such as tick and volume bars, and more excitingly, imbalance bars that try to
detect large sweeps of the order book and other meaningful divergences. Being
able to generate these other types of bars, and maybe novel sampling techniques
too, is a direction we're looking into.

## More Input Data

There are endless data sources available, and smooth integration for ingesting
and processing that data is valuable. We're eyeballing
[tardis.dev](https://tardis.dev/), a crypto data provider, for the next source
after IQFeed.

Feat also focuses primarily on processing filled trades at the moment - but
there's no reason it couldn't generate samples from orders submitted and
cancelled or not hit (L2 style data) as well. Similarly, we have mused about
adding support for "[ETF
trick](https://quant.stackexchange.com/questions/51145/the-etf-trick-e-mini-sp-500-futures)"-ing
options data to make it more approachable to conduct research on common options
strategies such as buying or selling straddles, performing covered calls, etc.

## More Features

We could add support for computing strutural break tests, computationally
expensive features such as autocorrelation, or features based on the market
microstructure that are not otherwise readily available, such as the volume
filled on the bid or ask.
