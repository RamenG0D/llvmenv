//! Describes how to compile LLVM/Clang
//!
//! entry.toml
//! -----------
//! **entry** in llvmenv describes how to compile LLVM/Clang, and set by `$XDG_CONFIG_HOME/llvmenv/entry.toml`.
//! `llvmenv init` generates default setting:
//!
//! ```toml
//! [llvm-mirror]
//! url    = "https://github.com/llvm-mirror/llvm"
//! target = ["X86"]
//!
//! [[llvm-mirror.tools]]
//! name = "clang"
//! url = "https://github.com/llvm-mirror/clang"
//!
//! [[llvm-mirror.tools]]
//! name = "clang-extra"
//! url = "https://github.com/llvm-mirror/clang-tools-extra"
//! relative_path = "tools/clang/tools/extra"
//! ```
//!
//! (TOML format has been changed largely at version 0.2.0)
//!
//! **tools** property means LLVM tools, e.g. clang, compiler-rt, lld, and so on.
//! These will be downloaded into `${llvm-top}/tools/${tool-name}` by default,
//! and `relative_path` property change it.
//! This toml will be decoded into [EntrySetting][EntrySetting] and normalized into [Entry][Entry].
//!
//! [Entry]: ./enum.Entry.html
//! [EntrySetting]: ./struct.EntrySetting.html
//!
//! Local entries (since v0.2.0)
//! -------------
//! Different from above *remote* entries, you can build locally cloned LLVM source with *local* entry.
//!
//! ```toml
//! [my-local-llvm]
//! path = "/path/to/your/src"
//! target = ["X86"]
//! ```
//!
//! Entry is regarded as *local* if there is `path` property, and *remote* if there is `url` property.
//! Other options are common to *remote* entries.
//!
//! Pre-defined entries
//! ------------------
//!
//! There is also pre-defined entries corresponding to the LLVM/Clang releases:
//!
//! ```shell
//! $ llvmenv entries
//! llvm-mirror
//! 7.0.0
//! 6.0.1
//! 6.0.0
//! 5.0.2
//! 5.0.1
//! 4.0.1
//! 4.0.0
//! 3.9.1
//! 3.9.0
//! ```
//!
//! These are compiled with the default setting as shown above. You have to create entry manually
//! if you want to use custom settings.

use itertools::*;
use log::{info, warn};
use regex::Regex;
use semver::Version;
use serde_derive::Deserialize;
use std::{collections::HashMap, fs, path::PathBuf, process, str::FromStr};

use crate::{config::*, error::*, resource::*};

/// Option for CMake Generators
///
/// - Official document: [CMake Generators](https://cmake.org/cmake/help/latest/manual/cmake-generators.7.html)
///
/// ```
/// use llvmenv::entry::CMakeGenerator;
/// use std::str::FromStr;
/// assert_eq!(CMakeGenerator::from_str("Makefile").unwrap(), CMakeGenerator::Makefile);
/// assert_eq!(CMakeGenerator::from_str("Ninja").unwrap(), CMakeGenerator::Ninja);
/// assert_eq!(CMakeGenerator::from_str("vs").unwrap(), CMakeGenerator::VisualStudio);
/// assert_eq!(CMakeGenerator::from_str("VisualStudio").unwrap(), CMakeGenerator::VisualStudio);
/// assert!(CMakeGenerator::from_str("MySuperBuilder").is_err());
/// ```
#[derive(Deserialize, PartialEq, Debug, Clone, Default)]
pub enum CMakeGenerator {
    /// Use platform default generator (without -G option)
    #[default]
    Platform,
    /// Unix Makefile
    Makefile,
    /// Ninja generator
    Ninja,
    /// Visual Studio 15 2017
    VisualStudio,
    /// Visual Studio 15 2017 Win64
    VisualStudioWin64,
}

impl FromStr for CMakeGenerator {
    type Err = Error;
    fn from_str(generator: &str) -> Result<Self> {
        Ok(match generator.to_ascii_lowercase().as_str() {
            "makefile" => CMakeGenerator::Makefile,
            "ninja" => CMakeGenerator::Ninja,
            "visualstudio" | "vs" => CMakeGenerator::VisualStudio,
            _ => {
                return Err(Error::UnsupportedGenerator {
                    generator: generator.into(),
                });
            }
        })
    }
}

impl CMakeGenerator {
    /// Option for cmake
    pub fn option(&self) -> Vec<String> {
        match self {
            CMakeGenerator::Platform => Vec::new(),
            CMakeGenerator::Makefile => vec!["-G", "Unix Makefiles"],
            CMakeGenerator::Ninja => vec!["-G", "Ninja"],
            CMakeGenerator::VisualStudio => vec!["-G", "Visual Studio 15 2017"],
            CMakeGenerator::VisualStudioWin64 => {
                vec!["-G", "Visual Studio 17 2022 Win64", "-Thost=x64"]
            }
        }
        .into_iter()
        .map(|s| s.into())
        .collect()
    }

    /// Option for cmake build mode (`cmake --build` command)
    pub fn build_option(&self, nproc: usize, build_type: BuildType) -> Vec<String> {
        match self {
            CMakeGenerator::VisualStudioWin64 | CMakeGenerator::VisualStudio => {
                vec!["--config".into(), format!("{:?}", build_type)]
            }
            CMakeGenerator::Platform => Vec::new(),
            CMakeGenerator::Makefile | CMakeGenerator::Ninja => {
                vec!["--".into(), "-j".into(), format!("{}", nproc)]
            }
        }
    }
}

/// CMake build type
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub enum BuildType {
    Debug,
    #[default]
    Release,
    RelWithDebInfo,
    MinSizeRel,
}

impl FromStr for BuildType {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "debug" => Ok(BuildType::Debug),
            "release" => Ok(BuildType::Release),
            "relwithdebinfo" => Ok(BuildType::RelWithDebInfo),
            "minsizerel" => Ok(BuildType::MinSizeRel),
            _ => Err(Error::UnsupportedBuildType {
                build_type: s.to_string(),
            }),
        }
    }
}

/// LLVM Tools e.g. clang, compiler-rt, and so on.
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Tool {
    /// Name of tool (will be downloaded into `tools/{name}` by default)
    pub name: String,

    /// URL for tool. Git/SVN repository or Tar archive are allowed.
    pub url: String,

    /// Git branch (not for SVN)
    pub branch: Option<String>,

    /// Relative install Path (see the example of clang-extra in [module level doc](index.html))
    pub relative_path: Option<String>,
}

impl Tool {
    fn new(name: &str, url: &str) -> Self {
        Tool {
            name: name.into(),
            url: url.into(),
            branch: None,
            relative_path: None,
        }
    }

    fn rel_path(&self) -> String {
        match self.relative_path {
            Some(ref rel_path) => rel_path.to_string(),
            None => match self.name.as_str() {
                "clang-tools-extra" | "compiler-rt" | "libcxx" | "libcxxabi" | "libunwind"
                | "openmp" | "third-party" | "mlir" | "cmake" | "clang" | "lld" | "lldb"
                | "polly" => {
                    format!("../{}", self.name)
                }
                _ => panic!(
                    "Unknown tool. Please specify its relative path explicitly: {}",
                    self.name
                ),
            },
        }
    }
}

/// Setting for both Remote and Local entries. TOML setting file will be decoded into this struct.
#[derive(Deserialize, Debug, Default, Clone, PartialEq)]
pub struct EntrySetting {
    /// URL of remote LLVM resource, see also [resouce](../resource/index.html) module
    pub url: Option<String>,

    /// Path of local LLVM source dir
    pub path: Option<String>,

    /// Additional LLVM Tools, e.g. clang, openmp, lld, and so on.
    #[serde(default)]
    pub tools: Vec<Tool>,

    /// Target to be build, e.g. "X86". Empty means all backend
    #[serde(default)]
    pub target: Vec<String>,

    /// CMake Generator option (-G option in cmake)
    #[serde(default)]
    pub generator: CMakeGenerator,

    ///  Option for `CMAKE_BUILD_TYPE`
    #[serde(default)]
    pub build_type: BuildType,

    /// Additional LLVM build options
    #[serde(default)]
    pub option: HashMap<String, String>,

    /// Wether or not this is an individual tarball or a whole project
    #[serde(default)]
    pub project: bool,
}

/// Describes how to compile LLVM/Clang
///
/// See also [module level document](index.html).
#[derive(Debug, PartialEq)]
pub enum Entry {
    Remote {
        name: String,
        version: Option<Version>,
        url: String,
        tools: Vec<Tool>,
        setting: EntrySetting,
    },
    Local {
        name: String,
        version: Option<Version>,
        path: PathBuf,
        setting: EntrySetting,
    },
}

fn load_entry_toml(toml_str: &str) -> Result<Vec<Entry>> {
    let entries: HashMap<String, EntrySetting> = toml::from_str(toml_str)?;
    entries
        .into_iter()
        .map(|(name, setting)| Entry::parse_setting(&name, Version::parse(&name).ok(), setting))
        .collect()
}

// Fixed to get the tags automatically from the official github repository
pub fn official_releases() -> Vec<Entry> {
    let mut command = process::Command::new("git");
    let out = command
        .args([
            "ls-remote",
            "--tags",
            "--refs",
            "https://github.com/llvm/llvm-project.git",
        ])
        .output()
        .expect("Please Install `GIT` for your system")
        .stdout;

    let output = String::from_utf8_lossy(&out);
    // strip the stuff we don't need
    // example: `4df9396b4217bb9a0a39ea81f9d977014b64e491	refs/tags/llvmorg-1.0.0`
    let output = output.split("\n").collect::<Vec<&str>>();
    let tags = output[0..output.len() - 1]
        .iter()
        .map(|x| x.split("\t").collect::<Vec<&str>>()[1])
        .map(|x| {
            // remove the refs/tags/llvmorg- from the string
            x.strip_prefix("refs/tags/llvmorg-")
                .expect("Failed to strip prefix from tag")
        })
        .filter(|x| {
            // just discard the tags that don't match the semver pattern
            // like `10-init` and `10.0.0-rc1`
            Regex::new(r"^\d+\.\d+\.\d+$").unwrap().is_match(x)
        })
        // now we need to discard any duplicate tags
        .unique()
        // now just order each version by semver ordering
        .map(|x| Version::parse(x).expect("Failed to parse version"))
        .sorted()
        // just reverse the order for decending order
        .rev()
        .collect::<Vec<_>>();

    tags.iter()
        .map(|x| Entry::official(x.major, x.minor, x.patch))
        .collect()
}

pub fn load_entries() -> Result<Vec<Entry>> {
    let global_toml = config_dir()?.join(ENTRY_TOML);
    let mut entries = load_entry_toml(&fs::read_to_string(&global_toml).with(&global_toml)?)?;
    let mut official = official_releases();
    entries.append(&mut official);
    Ok(entries)
}

pub fn load_entry(name: &str) -> Result<Entry> {
    let entries = load_entries()?;
    for entry in entries {
        if entry.name() == name {
            return Ok(entry);
        }
    }
    Err(Error::InvalidEntry {
        message: "Entry not found".into(),
        name: name.into(),
    })
}

lazy_static::lazy_static! {
    static ref LLVM_9_0_0: Version = Version::new(9, 0, 0);
    static ref LLVM_8_0_1: Version = Version::new(8, 0, 1);
}

impl Entry {
    /// Entry for official LLVM release
    pub fn official(major: u64, minor: u64, patch: u64) -> Self {
        let version = Version::new(major, minor, patch);
        let mut setting = EntrySetting::default();

        let base_url = if version <= *LLVM_9_0_0 && version != *LLVM_8_0_1 {
            format!("http://releases.llvm.org/{}", version)
        } else {
            format!(
                "https://github.com/llvm/llvm-project/releases/download/llvmorg-{}",
                version
            )
        };

        setting.url = Some(format!("{}/llvm-{}.src.tar.xz", base_url, version));
        setting.tools.push(Tool::new(
            "clang",
            &format!(
                "{}/{}-{}.src.tar.xz",
                base_url,
                if version > *LLVM_9_0_0 {
                    "clang"
                } else {
                    "cfe"
                },
                version
            ),
        ));

        // these tools are only available from versions 16.0.0 and above
        if version >= Version::new(16, 0, 0) {
            setting.tools.push(Tool::new(
                "mlir",
                &format!("{}/mlir-{}.src.tar.xz", base_url, version),
            ));
            setting.tools.push(Tool::new(
                "third-party",
                &format!("{}/third-party-{}.src.tar.xz", base_url, version),
            ));
            setting.tools.push(Tool::new(
                "cmake",
                &format!("{}/cmake-{}.src.tar.xz", base_url, version),
            ));
        }

        setting.tools.push(Tool::new(
            "polly",
            &format!("{}/polly-{}.src.tar.xz", base_url, version),
        ));

        #[cfg(not(target_os = "macos"))]
        setting.tools.push(Tool::new(
            "compiler-rt",
            &format!("{}/compiler-rt-{}.src.tar.xz", base_url, version),
        ));

        setting.tools.push(Tool::new(
            "lld",
            &format!("{}/lld-{}.src.tar.xz", base_url, version),
        ));
        setting.tools.push(Tool::new(
            "lldb",
            &format!("{}/lldb-{}.src.tar.xz", base_url, version),
        ));
        setting.tools.push(Tool::new(
            "clang-tools-extra",
            &format!("{}/clang-tools-extra-{}.src.tar.xz", base_url, version),
        ));
        // unfortunately, libcxx and libcxxabi are not available for windows
        // due to current msvc limitations :(
        // dang... 'L' Windows (Heh)
        #[cfg(not(target_os = "windows"))]
        {
            setting.tools.push(Tool::new(
                "libcxx",
                &format!("{}/libcxx-{}.src.tar.xz", base_url, version),
            ));
            setting.tools.push(Tool::new(
                "libcxxabi",
                &format!("{}/libcxxabi-{}.src.tar.xz", base_url, version),
            ));
        }

        // libunwind is not available for macos
        #[cfg(not(target_os = "macos"))]
        setting.tools.push(Tool::new(
            "libunwind",
            &format!("{}/libunwind-{}.src.tar.xz", base_url, version),
        ));

        setting.tools.push(Tool::new(
            "openmp",
            &format!("{}/openmp-{}.src.tar.xz", base_url, version),
        ));

        let name = version.to_string();

        Entry::parse_setting(&name, Some(version), setting).unwrap()
    }

    fn parse_setting(name: &str, version: Option<Version>, setting: EntrySetting) -> Result<Self> {
        if setting.path.is_some() && setting.url.is_some() {
            return Err(Error::InvalidEntry {
                name: name.into(),
                message: "One of Path or URL are allowed".into(),
            });
        }
        if let Some(path) = &setting.path {
            if !setting.tools.is_empty() {
                warn!("'tools' must be used with URL, ignored");
            }
            return Ok(Entry::Local {
                name: name.into(),
                version,
                path: PathBuf::from(shellexpand::full(&path).unwrap().to_string()),
                setting,
            });
        }
        if let Some(url) = &setting.url {
            return Ok(Entry::Remote {
                name: name.into(),
                version,
                url: url.clone(),
                tools: setting.tools.clone(),
                setting,
            });
        }
        Err(Error::InvalidEntry {
            name: name.into(),
            message: "Path nor URL are not found".into(),
        })
    }

    fn setting(&self) -> &EntrySetting {
        match self {
            Entry::Remote { setting, .. } => setting,
            Entry::Local { setting, .. } => setting,
        }
    }

    fn setting_mut(&mut self) -> &mut EntrySetting {
        match self {
            Entry::Remote { setting, .. } => setting,
            Entry::Local { setting, .. } => setting,
        }
    }

    pub fn set_builder(&mut self, generator: &str) -> Result<()> {
        let generator = CMakeGenerator::from_str(generator)?;
        self.setting_mut().generator = generator;
        Ok(())
    }

    pub fn set_build_type(&mut self, build_type: BuildType) -> Result<()> {
        self.setting_mut().build_type = build_type;
        Ok(())
    }

    pub fn checkout(&self) -> Result<()> {
        match self {
            Entry::Remote { url, tools, .. } => {
                let src = Resource::from_url(url)?;
                src.download(&self.src_dir()?)?;
                for tool in tools {
                    let path = self.src_dir()?.join(tool.rel_path());
                    let src = Resource::from_url(&tool.url)?;
                    src.download(&path)?;
                }
            }
            Entry::Local { .. } => {}
        }
        Ok(())
    }

    pub fn clean_cache_dir(&self) -> Result<()> {
        let path = self.src_dir()?;
        info!("Remove cache dir: {}", path.display());
        fs::remove_dir_all(&path).with(&path)?;
        Ok(())
    }

    pub fn update(&self) -> Result<()> {
        match self {
            Entry::Remote { url, tools, .. } => {
                let src = Resource::from_url(url)?;
                src.update(&self.src_dir()?)?;
                for tool in tools {
                    let src = Resource::from_url(&tool.url)?;
                    src.update(&self.src_dir()?.join(tool.rel_path()))?;
                }
            }
            Entry::Local { .. } => {}
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        match self {
            Entry::Remote { name, .. } => name,
            Entry::Local { name, .. } => name,
        }
    }

    pub fn version(&self) -> Option<&Version> {
        match self {
            Entry::Remote { version, .. } => version.as_ref(),
            Entry::Local { version, .. } => version.as_ref(),
        }
    }

    pub fn src_dir(&self) -> Result<PathBuf> {
        Ok(match self {
            Entry::Remote { name, .. } => {
                if !self.setting().project {
                    cache_dir()?.join(name).join("llvm")
                } else {
                    cache_dir()?.join(name)
                }
            }
            Entry::Local { path, .. } => path.into(),
        })
    }

    pub fn build_dir(&self) -> Result<PathBuf> {
        let dir = self.src_dir()?.join("build");
        if !dir.exists() {
            info!("Create build dir: {}", dir.display());
            fs::create_dir_all(&dir).with(&dir)?;
        }
        Ok(dir)
    }

    pub fn clean_build_dir(&self) -> Result<()> {
        let path = self.build_dir()?;
        info!("Remove build dir: {}", path.display());
        fs::remove_dir_all(&path).with(&path)?;
        Ok(())
    }

    pub fn prefix(&self) -> Result<PathBuf> {
        Ok(data_dir()?.join(self.name()))
    }

    pub fn build(&self, nproc: usize) -> Result<()> {
        self.configure()?;
        let build_dir = self.build_dir()?;
        info!("Build LLVM/Clang: {}", build_dir.display());
        process::Command::new("cmake")
            .args([
                "--build",
                &format!("{}", build_dir.display()),
                "--target",
                "install",
            ])
            .args(
                self.setting()
                    .generator
                    .build_option(nproc, self.setting().build_type),
            )
            .check_run()?;
        Ok(())
    }

    fn configure(&self) -> Result<()> {
        let setting = self.setting();
        let mut opts = setting.generator.option();
        let dir = if setting.project {
            self.src_dir()?.join("llvm")
        } else {
            self.src_dir()?
        };
        opts.push(format!("{}", dir.display()));

        opts.push(format!(
            "-DCMAKE_INSTALL_PREFIX={}",
            data_dir()?.join(self.prefix()?).display()
        ));

        opts.push(format!("-DCMAKE_BUILD_TYPE={:?}", setting.build_type));

        // Enable ccache if exists
        if which::which("ccache").is_ok() {
            opts.push("-DLLVM_CCACHE_BUILD=ON".into());
        }

        // Enable lld if exists
        if which::which("lld").is_ok() {
            opts.push("-DLLVM_ENABLE_LLD=ON".into());
        }

        // Target architectures
        if !setting.target.is_empty() {
            opts.push(format!(
                "-DLLVM_TARGETS_TO_BUILD={}",
                setting.target.iter().join(";")
            ));
        }

        // Other options
        for (k, v) in &setting.option {
            opts.push(format!("-D{}={}", k, v));
        }

        process::Command::new("cmake")
            .args(&opts)
            .current_dir(self.build_dir()?)
            .check_run()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_url() {
        let setting = EntrySetting {
            url: Some("http://llvm.org/svn/llvm-project/llvm/trunk".into()),
            ..Default::default()
        };
        let _entry = Entry::parse_setting("url", None, setting).unwrap();
    }

    #[test]
    fn parse_path() {
        let setting = EntrySetting {
            path: Some("~/.config/llvmenv".into()),
            ..Default::default()
        };
        let _entry = Entry::parse_setting("path", None, setting).unwrap();
    }

    #[should_panic]
    #[test]
    fn parse_no_entry() {
        let setting = EntrySetting::default();
        let _entry = Entry::parse_setting("no_entry", None, setting).unwrap();
    }

    #[should_panic]
    #[test]
    fn parse_duplicated() {
        let setting = EntrySetting {
            url: Some("http://llvm.org/svn/llvm-project/llvm/trunk".into()),
            path: Some("~/.config/llvmenv".into()),
            ..Default::default()
        };
        let _entry = Entry::parse_setting("duplicated", None, setting).unwrap();
    }

    #[test]
    fn parse_with_version() {
        let path = "~/.config/llvmenv";
        let version = Version::new(10, 0, 0);
        let setting = EntrySetting {
            path: Some(path.into()),
            ..Default::default()
        };
        let entry = Entry::parse_setting("path", Some(version.clone()), setting.clone()).unwrap();

        assert_eq!(entry.version(), Some(&version));
        assert_eq!(
            entry,
            Entry::Local {
                name: "path".into(),
                version: Some(version),
                path: PathBuf::from(shellexpand::full(path).unwrap().to_string()),
                setting,
            }
        )
    }

    macro_rules! checkout {
        ($major:expr, $minor:expr, $patch: expr) => {
            paste::item! {
                #[ignore]
                #[test]
                fn [< checkout_ $major _ $minor _ $patch >]() {
                    Entry::official($major, $minor, $patch).checkout().unwrap();
                }
            }
        };
    }

    checkout!(13, 0, 0);
    checkout!(12, 0, 1);
    checkout!(12, 0, 0);
    checkout!(11, 1, 0);
    checkout!(11, 0, 0);
    checkout!(10, 0, 1);
    checkout!(10, 0, 0);
    checkout!(9, 0, 1);
    checkout!(8, 0, 1);
    checkout!(9, 0, 0);
    checkout!(8, 0, 0);
    checkout!(7, 1, 0);
    checkout!(7, 0, 1);
    checkout!(7, 0, 0);
    checkout!(6, 0, 1);
    checkout!(6, 0, 0);
    checkout!(5, 0, 2);
    checkout!(5, 0, 1);
    checkout!(4, 0, 1);
    checkout!(4, 0, 0);
    checkout!(3, 9, 1);
    checkout!(3, 9, 0);
}
