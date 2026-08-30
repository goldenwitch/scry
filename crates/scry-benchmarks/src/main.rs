//! Machine-agnostic baseline artifact runner for scry.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use scry::Embed;

mod artifact;
mod bridge;
mod onnx;
mod shape;
mod workload;

const USAGE: &str = "usage:
    scry-benchmarks [--cache PATH] [--model PATH] [--output PATH] [--check PATH]
                    [--add-only]
                    [--prepare-cache]
				   [--batch-size N] [--sequence-length N]

The v1 baseline is fixed to batch size 1 and sequence length 32.
The model path defaults to PATH/<pinned-revision>/model.onnx under the cache.
The cache defaults to SCRY_MODEL_CACHE or the system temporary directory.
The output defaults to benchmarks/baseline-v1.json.
--add-only runs the diagnostic add workload matrix and writes no artifact.";

struct Options {
    cache: PathBuf,
    model: Option<PathBuf>,
    output: PathBuf,
    check: Option<PathBuf>,
    add_only: bool,
    prepare_cache: bool,
    batch_size: u64,
    sequence_length: u64,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("scry-benchmarks: {error}");
        eprintln!("{USAGE}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let Some(options) = parse_options()? else {
        println!("{USAGE}");
        return Ok(());
    };
    if options.prepare_cache {
        Embed::load(&options.cache).map_err(|error| {
            format!(
                "cannot prepare pinned model cache at {}: {error}",
                options.cache.display()
            )
        })?;
        verify_model_cache(&options.cache)?;
        println!("prepared {}", options.cache.display());
        return Ok(());
    }
    if options.add_only {
        verify_model_cache(&options.cache)?;
        workload::run_add_only(&options.cache)?;
        return Ok(());
    }
    let model_path = options.model.clone().unwrap_or_else(|| {
        options
            .cache
            .join(bridge::MODEL_REVISION)
            .join("model.onnx")
    });
    verify_model_cache(&options.cache)?;
    let model_bytes = fs::read(&model_path).map_err(|error| {
        format!(
            "cannot read pinned model at {}: {error}",
            model_path.display()
        )
    })?;
    let config = bridge::BridgeConfig::new(options.batch_size, options.sequence_length)
        .map_err(|error| error.to_string())?;
    if config.batch_size != 1 || config.sequence_length != 32 {
        return Err("v1 baseline requires batch size 1 and sequence length 32".to_owned());
    }
    let static_model =
        bridge::build_static_model(&model_bytes, config).map_err(|error| error.to_string())?;
    let workload = workload::run(&options.cache)?;
    let artifact = artifact::build_artifact(
        &static_model,
        config,
        workload.input_fingerprint,
        &workload.snapshot,
    )
    .map_err(|error| error.to_string())?;
    if let Some(path) = options.check {
        artifact::check_artifact(&artifact, &path).map_err(|error| error.to_string())?;
        println!("baseline matches {}", path.display());
    } else {
        artifact::write_artifact(&artifact, &options.output).map_err(|error| error.to_string())?;
        println!("wrote {}", options.output.display());
    }
    Ok(())
}

fn parse_options() -> Result<Option<Options>, String> {
    let mut arguments = env::args().skip(1);
    let mut cache = None;
    let mut model = None;
    let mut output = PathBuf::from("benchmarks/baseline-v1.json");
    let mut check = None;
    let mut add_only = false;
    let mut prepare_cache = false;
    let mut batch_size = 1;
    let mut sequence_length = 32;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(None),
            "--cache" => cache = Some(PathBuf::from(required(&mut arguments, "cache")?)),
            "--model" => model = Some(PathBuf::from(required(&mut arguments, "model")?)),
            "--output" => output = PathBuf::from(required(&mut arguments, "output")?),
            "--check" => check = Some(PathBuf::from(required(&mut arguments, "check")?)),
            "--add-only" => add_only = true,
            "--prepare-cache" => prepare_cache = true,
            "--batch-size" => {
                batch_size = positive(&required(&mut arguments, "batch-size")?, "batch-size")?;
            }
            "--sequence-length" => {
                sequence_length = positive(
                    &required(&mut arguments, "sequence-length")?,
                    "sequence-length",
                )?;
            }
            unknown => return Err(format!("unknown argument `{unknown}`")),
        }
    }
    let cache = cache
        .or_else(|| env::var_os("SCRY_MODEL_CACHE").map(PathBuf::from))
        .unwrap_or_else(|| env::temp_dir().join("scry-model-cache"));
    Ok(Some(Options {
        cache,
        model,
        output,
        check,
        add_only,
        prepare_cache,
        batch_size,
        sequence_length,
    }))
}

fn required(arguments: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    arguments
        .next()
        .ok_or_else(|| format!("{name} requires a value"))
}

fn positive(value: &str, name: &str) -> Result<u64, String> {
    let value = value
        .parse::<u64>()
        .map_err(|_| format!("{name} is not a positive integer"))?;
    (value > 0)
        .then_some(value)
        .ok_or_else(|| format!("{name} is not a positive integer"))
}

fn verify_model_cache(cache: &Path) -> Result<(), String> {
    let revision = cache.join(bridge::MODEL_REVISION);
    for (relative, expected_size) in bridge::MODEL_FILES {
        let path = revision.join(relative);
        let metadata = fs::metadata(&path).map_err(|error| {
            format!(
                "pinned model asset {} is unavailable: {error}",
                path.display()
            )
        })?;
        if metadata.len() != expected_size {
            return Err(format!(
                "pinned model asset {} has {} bytes, expected {}",
                path.display(),
                metadata.len(),
                expected_size
            ));
        }
    }
    Ok(())
}
