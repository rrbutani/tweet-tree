use std::convert::Infallible;
use std::env;
use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;

use color_eyre::{eyre::WrapErr, Help, Result};
use egg_mode::tweet;
use once_cell::unsync::OnceCell;
use structopt::StructOpt;

trait EnvVarOrArg {
    const NAME: &'static str;
    const VAR_NAME: &'static str;
    const ARG_NAME: &'static str;
}

macro_rules! env_var_arg {
    ($(
        $nom:ident: ($name:literal, $var:literal, $arg:literal $(,)?)
    ),* $(,)?) => {$(
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        struct $nom;
        impl EnvVarOrArg for $nom {
            const NAME: &'static str = $name;
            const VAR_NAME: &'static str = $var;
            const ARG_NAME: &'static str = $arg;
        }
    )*};
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ArgWithEnvVarDefault<Arg: EnvVarOrArg>(
    OnceCell<String>,
    PhantomData<Arg>,
);

impl<A: EnvVarOrArg> Default for ArgWithEnvVarDefault<A> {
    fn default() -> Self {
        Self(OnceCell::new(), PhantomData)
    }
}

impl<A: EnvVarOrArg> Display for ArgWithEnvVarDefault<A> {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(inner) = self.0.get() { write!(fmt, "{}", inner)?; }

        Ok(())
    }
}

impl<A: EnvVarOrArg> FromStr for ArgWithEnvVarDefault<A> {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Infallible> {
        let inner = if s.is_empty() {
            OnceCell::new()
        } else {
            s.to_string().into()
        };

        Ok(Self(inner, PhantomData))
    }
}

impl<A: EnvVarOrArg> Deref for ArgWithEnvVarDefault<A> {
    type Target = String;

    fn deref(&self) -> &String {
        self.0.get_or_try_init(|| {
            env::var(A::VAR_NAME)
                .wrap_err_with(||
                    format!("Unable to get the {} from `${}`.", A::NAME, A::VAR_NAME)
                )
        })
        .with_suggestion(|| format!("pass in `--{}` or set `${}`", A::ARG_NAME, A::VAR_NAME))
        .unwrap()
    }
}

env_var_arg! {
    ConsumerKey: (
        "Twitter API Consumer Key",
        "TWITTER_CONSUMER_KEY",
        "consumer-key",
    ),
    ConsumerSecret: (
        "Twitter API Consumer Secret",
        "TWITTER_CONSUMER_SECRET",
        "consumer-secret",
    ),
}

// impl<A: EnvVarOrArg> Deref for ArgWithEnvVarDefault<>

// #[derive(Clone, Debug, Display, FromStr, PartialEq, Eq)]
// struct ConsumerKey(String);

// impl Default for ConsumerKey {
//     fn default() -> Self {
//         Self(env::var("TWITTER_CONSUMER_KEY")
//             .wrap_err("Unable to get the Twitter API Consumer Key.")
//             .suggestion("pass in `--consumer_key` or set `$TWITTER_CONSUMER_KEY`")
//             .unwrap()
//         )
//     }
// }

// #[derive(Clone, Debug, Display, FromStr, PartialEq, Eq)]
// struct ConsumerSecret(String);

// impl Default for ConsumerSecret {
//     fn default() -> Self {
//         Self(env::var("TWITTER_CONSUMER_SECRET")
//             .wrap_err("Unable to get the Twitter API Consumer Secret.")
//             .suggestion("pass in `--consumer_secret` or set `$TWITTER_CONSUMER_SECRET`")
//             .unwrap()
//         )
//     }
// }

#[derive(Debug, StructOpt)]
struct Args {
    /// The root of the twitter thread to crawl.
    root_tweet_id: u64,

    /// Output file for the graph (graphviz dot); stdout if not given.
    #[structopt(short = "o", long = "output", parse(from_os_str))]
    output: Option<PathBuf>,

    #[structopt(default_value)]
    /// Twitter API consumer key. Must be authorized to use the V2 API.
    ///
    /// If not specified this is grabbed from `$TWITTER_CONSUMER_KEY`.
    consumer_key: ArgWithEnvVarDefault<ConsumerKey>,

    /// Twitter API consumer secret. Must be authorized to use the V2 API.
    ///
    /// If not specified this is grabbed from `$TWITTER_CONSUMER_SECRET`.
    #[structopt(default_value)]
    consumer_secret: ArgWithEnvVarDefault<ConsumerSecret>,
}


#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::from_args();

    println!("{}", *args.consumer_key);
    println!("{}", *args.consumer_secret);

    Ok(())
}
