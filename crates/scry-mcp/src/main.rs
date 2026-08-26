//! MCP stdio server for the scry corpus library.

mod protocol;
mod server;
mod wire;

use std::env;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;

use scry::{Embed, Slice, Store};

struct Config {
    store: PathBuf,
    cache: PathBuf,
}

impl Config {
    fn from_args(mut arguments: impl Iterator<Item = OsString>) -> io::Result<Self> {
        let _program = arguments.next();
        let mut store = None;
        let mut cache = None;
        while let Some(argument) = arguments.next() {
            match argument.to_str() {
                Some("--store") => store = Some(path_argument(&mut arguments, "--store")?),
                Some("--cache") => cache = Some(path_argument(&mut arguments, "--cache")?),
                Some(unknown) => {
                    return Err(invalid_argument(format!("unknown argument: {unknown}")));
                }
                None => return Err(invalid_argument("an argument was not valid UTF-8")),
            }
        }
        let store = store.ok_or_else(|| invalid_argument("--store is required"))?;
        let cache = cache.ok_or_else(|| invalid_argument("--cache is required"))?;
        Ok(Self { store, cache })
    }
}

fn path_argument(
    arguments: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> io::Result<PathBuf> {
    arguments
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| invalid_argument(format!("{flag} needs a path")))
}

fn invalid_argument(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn run() -> io::Result<()> {
    let config = Config::from_args(env::args_os())?;
    let embed = Embed::load(&config.cache)?;
    let slice = Slice::new(&embed)?;
    let store = match Store::open(&config.store, embed.model())? {
        Ok(store) => store,
        Err(end) => {
            return Err(io::Error::new(io::ErrorKind::InvalidData, end.to_string()));
        }
    };
    server::Server::new(store, embed, slice).run(io::stdin().lock(), io::stdout().lock())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("scry-mcp: {error}");
        std::process::exit(1);
    }
}
