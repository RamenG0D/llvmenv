use llvmenv::error::FileIoConvert;
use llvmenv::*;
use llvmenv::{config::cache_dir, error::CommandExt};

use log::info;
use simplelog::*;
use std::{
    env,
    path::PathBuf,
    process::{exit, Command},
};
use structopt::StructOpt;

#[derive(StructOpt, Debug)]
#[structopt(
    name = "llvmenv",
    about = "Manage multiple LLVM/Clang builds",
    setting = structopt::clap::AppSettings::ColoredHelp
)]
enum LLVMEnv {
    #[structopt(name = "init", about = "Initialize llvmenv")]
    Init {},

    #[structopt(name = "builds", about = "List usable build")]
    Builds {},

    #[structopt(
        name = "clean",
        about = "Cleans all whole build cache or specific build version cache"
    )]
    Clean {
        #[structopt(short = "a", long = "all", help = "clean all build cache")]
        all: bool,
        #[structopt(short = "n", long = "name", help = "clean specific build cache")]
        name: Option<String>,
    },

    #[structopt(name = "entries", about = "List entries to be built")]
    Entries {},
    #[structopt(name = "build-entry", about = "Build LLVM/Clang")]
    BuildEntry {
        name: String,
        #[structopt(short = "u", long = "update")]
        update: bool,
        // not needed anymore (just using the discard flag and auto cleaning the build dir if it exists)
        // #[structopt(short = "c", long = "clean", help = "clean build directory")]
        // clean: bool,
        #[structopt(
            short = "G",
            long = "builder",
            help = "Overwrite cmake generator setting"
        )]
        builder: Option<String>,
        #[structopt(
            short = "d",
            long = "discard",
            help = "discard source directory for remote resources"
        )]
        discard: bool,
        #[structopt(short = "j", long = "nproc")]
        nproc: Option<usize>,
        #[structopt(
            short = "t",
            long = "build-type",
            help = "Overwrite cmake build type (Debug, Release, RelWithDebInfo, or MinSizeRel)"
        )]
        build_type: Option<entry::BuildType>,
    },

    #[structopt(name = "current", about = "Show the name of current build")]
    Current {
        #[structopt(short = "v", long = "verbose")]
        verbose: bool,
    },
    #[structopt(name = "prefix", about = "Show the prefix of the current build")]
    Prefix {
        #[structopt(short = "v", long = "verbose")]
        verbose: bool,
    },
    #[structopt(name = "version", about = "Show the base version of the current build")]
    Version {
        #[structopt(short = "n", long = "name")]
        name: Option<String>,
        #[structopt(long = "major")]
        major: bool,
        #[structopt(long = "minor")]
        minor: bool,
        #[structopt(long = "patch")]
        patch: bool,
    },

    #[structopt(name = "global", about = "Set the build to use (global)")]
    Global { name: String },
    #[structopt(name = "local", about = "Set the build to use (local)")]
    Local {
        name: String,
        #[structopt(short = "p", long = "path", parse(from_os_str))]
        path: Option<PathBuf>,
    },

    #[structopt(name = "archive", about = "archive build into *.tar.xz (require pixz)")]
    Archive {
        name: String,
        #[structopt(short = "v", long = "verbose")]
        verbose: bool,
    },
    #[structopt(name = "expand", about = "expand archive")]
    Expand {
        #[structopt(parse(from_os_str))]
        path: PathBuf,
        #[structopt(short = "v", long = "verbose")]
        verbose: bool,
    },

    #[structopt(name = "edit", about = "Edit llvmenv configure in your editor")]
    Edit {},

    #[structopt(name = "zsh", about = "Setup Zsh integration")]
    Zsh {},
}

fn main() -> error::Result<()> {
    TermLogger::init(
        LevelFilter::Info,
        ConfigBuilder::new().set_time_offset_to_local().expect("time offset").build(),
        TerminalMode::Mixed,
		ColorChoice::Auto,
    )
    .or(SimpleLogger::init(
        LevelFilter::Info,
        ConfigBuilder::new().set_time_offset_to_local().expect("time offset").build(),
    ))
    .unwrap();

    let opt = LLVMEnv::from_args();
    match opt {
        LLVMEnv::Init {} => config::init_config()?,

        LLVMEnv::Clean { all, name } => {
            if all {
                let builds = entry::load_entries()?;
                for build in builds {
                    let vp = cache_dir()?.join(build.name());
                    if vp.exists() {
                        std::fs::remove_dir_all(vp)?;
                    }
                }
            } else if let Some(name) = name {
                let build = entry::load_entry(&name)?;
                build.clean_cache_dir()?;
            } else {
                log::error!("Either --all or --name is required");
            }
        }

        LLVMEnv::Builds {} => {
            let builds = build::builds()?;
            let max = builds.iter().map(|b| b.name().len()).max().unwrap();
            for b in &builds {
                println!(
                    "{name:<width$}: {prefix}",
                    name = b.name(),
                    prefix = b.prefix().display(),
                    width = max
                );
            }
        }

        LLVMEnv::Entries {} => {
            if let Ok(entries) = entry::load_entries() {
                for entry in &entries {
                    println!("{}", entry.name());
                }
            } else {
                panic!("No entries. Please define entries in $XDG_CONFIG_HOME/llvmenv/entry.toml");
            }
        }
        LLVMEnv::BuildEntry {
            name,
            update,
            discard,
            builder,
            nproc,
            build_type,
        } => {
            let mut entry = entry::load_entry(&name)?;
            let nproc = nproc.unwrap_or_else(num_cpus::get);
            if let Some(builder) = builder {
                entry.set_builder(&builder)?;
            }
            if let Some(build_type) = build_type {
                entry.set_build_type(build_type)?;
            }

            let bdir = match entry {
                llvmenv::entry::Entry::Remote { ref name, .. } => {
                    dirs::cache_dir().unwrap().join(format!("llvmenv/{}", name))
                }
                llvmenv::entry::Entry::Local { ref path, .. } => path.into(),
            };
            if discard {
                // dir may or may not exist yet we dont want to error if it does not
                if bdir.exists() {
                    // remove the directory and all its contents
                    std::fs::remove_dir_all(&bdir).with(&bdir).unwrap();
                }
            } else {
                info!("source directory: {}", bdir.display());
            }
            let bdir = bdir.as_path();

            if bdir.exists() {
                info!("source directory already exists, so skiping checkout");
            } else {
                entry.checkout().unwrap();
            }
            if update {
                info!("updating source, by checking for required resources!");
                entry.update().unwrap();
            }

            entry.build(nproc).unwrap();

            // discarding the initial source directory should be default behavior (unless otherwise specified by the user)
            // TODO: Add a flag to keep the source directory here
            if discard && bdir.exists() {
                std::fs::remove_dir_all(bdir).with(bdir).unwrap();
            }
        }

        LLVMEnv::Current { verbose } => {
            let build = build::seek_build()?;
            println!("{}", build.name());
            if verbose {
                if let Some(env) = build.env_path() {
                    eprintln!("set by {}", env.display());
                }
            }
        }
        LLVMEnv::Prefix { verbose } => {
            let build = build::seek_build()?;
            println!("{}", build.prefix().display());
            if verbose {
                if let Some(env) = build.env_path() {
                    eprintln!("set by {}", env.display());
                }
            }
        }
        LLVMEnv::Version {
            name,
            major,
            minor,
            patch,
        } => {
            let build = if let Some(name) = name {
                get_existing_build(&name)
            } else {
                build::seek_build()?
            };
            let version = build.version()?;
            if !(major || minor || patch) {
                println!("{}.{}.{}", version.major, version.minor, version.patch);
            } else {
                if major {
                    print!("{}", version.major);
                }
                if minor {
                    print!("{}", version.minor);
                }
                if patch {
                    print!("{}", version.patch);
                }
                println!();
            }
        }

        LLVMEnv::Global { name } => {
            let build = get_existing_build(&name);
            build.set_global()?;
        }
        LLVMEnv::Local { name, path } => {
            let build = get_existing_build(&name);
            let path = path.unwrap_or_else(|| env::current_dir().unwrap());
            build.set_local(&path)?;
        }

        LLVMEnv::Archive { name, verbose } => {
            let build = get_existing_build(&name);
            build.archive(verbose)?;
        }
        LLVMEnv::Expand { path, verbose } => {
            build::expand(&path, verbose)?;
        }

        LLVMEnv::Edit {} => {
            let editor = env::var("EDITOR").expect("EDITOR environmental value is not set");
            Command::new(editor)
                .arg(config::config_dir()?.join(config::ENTRY_TOML))
                .check_run()?;
        }

        LLVMEnv::Zsh {} => {
            let src = include_str!("../../llvmenv.zsh");
            println!("{}", src);
        }
    }
    Ok(())
}

fn get_existing_build(name: &str) -> build::Build {
    let build = build::Build::from_name(name).unwrap();
    if build.exists() {
        build
    } else {
        eprintln!("Build '{}' does not exists", name);
        exit(1)
    }
}
