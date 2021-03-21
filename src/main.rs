use std::convert::{Infallible, TryInto};
use std::collections::HashMap;
use std::env;
use std::fmt::{self, Display};
use std::marker::PhantomData;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;

use chrono::Utc;
use color_eyre::{eyre::WrapErr, Help, owo_colors::OwoColorize, Result};
use egg_mode::{auth, tweet, user, KeyPair, Token};
use futures::StreamExt;
use once_cell::unsync::OnceCell;
use petgraph::graphmap::DiGraphMap;
use rand::{Rng, thread_rng};
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

#[derive(Debug)]
struct User {
    handle: String,
    name: String,
    color: (u8, u8, u8),
}

impl Display for User {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (r, g, b) = self.color;
        write!(fmt, "{} (@{})", self.name, self.handle.truecolor(r, g, b))
    }
}

impl User {
    async fn new(id: u64, token: &Token) -> Result<Self> {
        let user = &user::lookup(Some(user::UserID::ID(id)), token).await?[0];

        let color = if user.profile_background_color != "000000" {
            let c = &user.profile_background_color;
            let r = u8::from_str_radix(&c[0..2], 16)?;
            let g = u8::from_str_radix(&c[2..4], 16)?;
            let b = u8::from_str_radix(&c[4..6], 16)?;
            (r, g, b)
        } else {
            thread_rng().gen()
        };

        let user = User {
            handle: user.screen_name.clone(),
            name: user.name.clone(),
            color,
        };

        eprintln!("{}: {}", "New User".italic().blue(), user);
        Ok(user)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;
    let args = Args::from_args();

    let token = KeyPair::new(
        (*args.consumer_key).clone(),
        (*args.consumer_secret).clone(),
    );
    let token = auth::bearer_token(&token)
        .await
        .wrap_err("Unable to authenticate!")
        .suggestion("check your consumer key/consumer secret?")?;

    let root = tweet::show(args.root_tweet_id, &token)
        .await
        .wrap_err_with(|| format!("Failed to find the specified root tweet (`{}`)", args.root_tweet_id))?;

    if Utc::now().signed_duration_since(root.created_at).num_days() >= 7 {
        eprintln!(
            "{}: The given root tweet is {}!\n\n\
            The Twitter Recent Search API will not find tweets that are over \
            seven days old.\n\
            The Full-archive Search API will but that API is currently limited \
            to Academic Research users only.\n\n\
            See this page for more details: {}.",
            "WARNING".yellow().bold(),
            "over 7 days old".bold().italic(),
            "https://developer.twitter.com/en/docs/twitter-api/tweets/search/introduction".underline().italic(),
        );
    }

    type TweetId = u64;
    type UserId = u64;

    let mut users = HashMap::<UserId, (User, usize)>::new();
    let root_user_id = root.user.as_ref().unwrap().id;
    users.insert(root_user_id, (User::new(root_user_id, &token).await?, 1));

    let mut tweets = HashMap::<TweetId, UserId>::new();
    tweets.insert(args.root_tweet_id, root_user_id);

    let mut graph = DiGraphMap::<TweetId, UserId>::new();
    graph.add_node(args.root_tweet_id);

    let mut children = tweet::all_children_raw(args.root_tweet_id, &token).await;
    while let Some(t) = children.next().await {
        let t = t?;
        let author_id = t.author_id.unwrap();

        let (_, ref mut count) = if let Some(p) = users.get_mut(&author_id) {
            p
        } else {
            users.insert(author_id, (User::new(author_id, &token).await?, 0));
            users.get_mut(&author_id).unwrap()
        };
        *count += 1;

        let t: tweet::Tweet = (*t).clone().try_into()?;
        tweets.insert(t.id, author_id);

        let prev = t.in_reply_to_status_id.unwrap();
        graph.add_edge(prev, t.id, author_id);
    }

    eprintln!("{} tweets found! ({} unique users)", graph.node_count(), users.len());

    Ok(())
}
