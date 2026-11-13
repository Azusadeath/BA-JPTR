mod build;
mod files;
mod licenses;
mod native;
mod verify;

use clap::{Args, Parser, Subcommand};
use std::path::{Path, PathBuf};

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Parser)]
#[command(about = "BA-JPTR Windows development and release tools")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Subcommand)]
enum Task {
    /// Build applications and publish hash-addressed local engines.
    Build {
        #[command(flatten)]
        options: CargoOptions,
        #[arg(long)]
        no_publish: bool,
    },
    Check(CargoOptions),
    Test {
        #[command(flatten)]
        options: CargoOptions,
        #[arg(long)]
        test: Option<String>,
        #[arg(long)]
        filter: Option<String>,
    },
    Fmt {
        #[arg(long)]
        check: bool,
    },
    Lint {
        #[arg(long)]
        fix: bool,
    },
    Metadata {
        #[arg(long)]
        output: PathBuf,
    },
    Package {
        #[arg(long)]
        destination: Option<PathBuf>,
    },
    PackageSource {
        #[arg(long)]
        destination: PathBuf,
    },
    Licenses {
        #[arg(long)]
        metadata: Option<PathBuf>,
        #[arg(long)]
        destination: Option<PathBuf>,
    },
    VerifyPortable(VerifyOptions),
    Smoke(VerifyOptions),
}

#[derive(Args)]
struct CargoOptions {
    #[arg(long)]
    debug: bool,
    #[arg(long, short = 'p')]
    package: Option<String>,
    #[arg(long)]
    update_lock: bool,
}

#[derive(Args)]
struct VerifyOptions {
    #[arg(long)]
    portable: PathBuf,
    #[arg(long)]
    work_dir: PathBuf,
}

fn require(condition: bool, message: impl Into<String>) -> Result {
    if condition {
        Ok(())
    } else {
        Err(message.into().into())
    }
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn run() -> Result {
    let root = root();
    match Cli::parse().command {
        Task::Build {
            options,
            no_publish,
        } => {
            require(
                no_publish || options.package.is_none(),
                "Use --no-publish for a single-package build.",
            )?;
            build::cargo_task(root, "build", &options, None, None)?;
            if !no_publish {
                build::publish(root, options.debug)?;
            }
        }
        Task::Check(options) => build::cargo_task(root, "check", &options, None, None)?,
        Task::Test {
            options,
            test,
            filter,
        } => build::cargo_task(root, "test", &options, test.as_deref(), filter.as_deref())?,
        Task::Fmt { check } => build::fmt(root, check)?,
        Task::Lint { fix } => build::lint(root, fix)?,
        Task::Metadata { output } => files::write(&output, &build::metadata(root)?)?,
        Task::Package { destination } => files::package(
            root,
            &destination.unwrap_or_else(|| root.join("dist/BA-JPTR")),
        )?,
        Task::PackageSource { destination } => files::source_package(root, &destination)?,
        Task::Licenses {
            metadata,
            destination,
        } => {
            let bytes = match metadata {
                Some(path) => std::fs::read(path)?,
                None => build::metadata(root)?,
            };
            licenses::collect(
                &serde_json::from_slice(&bytes)?,
                &destination.unwrap_or_else(|| root.join("licenses/rust-dependencies")),
            )?;
        }
        Task::VerifyPortable(options) => verify::portable(&options)?,
        Task::Smoke(options) => verify::smoke(root, &options)?,
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("xtask: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
