// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use anyhow::{Context, Result, bail};
use git2::{Branch, Config, Repository};
use std::fmt::Debug;
use std::process::{Command, Output};

pub fn exec_impl(cmd: &mut Command) -> Result<Output> {
    let id = crate::env::next_exec_id();
    if crate::env::always_echo() {
        eprintln!("exec-{}: {:?}", id, cmd);
    }
    let output = cmd
        .output()
        .with_context(|| format!("exec-failed: {:?}", cmd))?;
    if crate::env::always_echo() || !output.status.success() {
        for line in String::from_utf8_lossy(output.stdout.as_ref()).lines() {
            eprintln!("exec-{}-stdout: {}", id, line);
        }
        for line in String::from_utf8_lossy(output.stderr.as_ref()).lines() {
            eprintln!("exec-{}-stderr: {}", id, line);
        }
    }
    if !output.status.success() {
        bail!("exec-{}-status-non-zero: {:?}", id, output.status);
    }
    Ok(output)
}

macro_rules! exec {
    ($cmd:ident) => {
        $crate::util::exec_impl(&mut $cmd)?
    };
    (dry_return=$dry_return:expr, $cmd:ident) => {{
        if $crate::env::dry_run() {
            eprintln!("would-exec: {:?}", $cmd);
            return Ok($dry_return);
        } else {
            $crate::util::exec!($cmd)
        }
    }};
}
pub(crate) use exec;

pub trait Extract {
    type T;

    fn extract(self) -> Self::T;
}

impl<T, E: Debug> Extract for std::result::Result<T, E> {
    type T = T;
    fn extract(self) -> T {
        match self {
            Ok(x) => x,
            Err(e) => {
                eprint!("Error: {e:?}");
                std::process::exit(-1);
            }
        }
    }
}

pub trait RepoExt {
    fn head_branch(&self) -> Result<Branch<'_>>;
    fn branch_config<'repo>(&self, branch: &'repo Branch) -> Result<BranchConfig<'repo>>;
}

impl RepoExt for Repository {
    fn head_branch<'repo>(&'repo self) -> Result<Branch<'repo>> {
        let branch = self.head().context("unknown HEAD")?;
        if !branch.is_branch() {
            bail!("HEAD is not a branch");
        }
        Ok(Branch::wrap(branch))
    }
    fn branch_config<'repo>(&self, branch: &'repo Branch) -> Result<BranchConfig<'repo>> {
        BranchConfig::new(self, branch)
    }
}

pub struct BranchConfig<'repo> {
    branch_name: &'repo str,
    config: Config,
}

impl<'repo> BranchConfig<'repo> {
    pub fn new(repo: &Repository, branch: &'repo Branch<'_>) -> Result<Self> {
        let branch_name = branch_name(branch)?;
        let config = config(repo)?;
        Ok(Self {
            branch_name,
            config,
        })
    }
    fn format_key(&self, key: &str) -> String {
        format!("branch.{}.praddle-{key}", self.branch_name)
    }
    pub fn get(&self, key: &str) -> Result<String> {
        let config_key = self.format_key(key);
        let value_result = self.config.get_entry(config_key.as_str());
        let value = match value_result {
            Ok(ce) => ce
                .value()
                .context("error getting config entry value")?
                .to_string(),
            // all the config values can safely default to the empty string
            Err(e) if e.code() == git2::ErrorCode::NotFound => "".to_string(),
            Err(e) => return Err(anyhow::Error::new(e).context("error getting config entry")),
        };
        Ok(value)
    }
    pub fn set(&mut self, key: &str, val: &str) -> Result<()> {
        let config_key = self.format_key(key);
        self.config
            .set_str(config_key.as_str(), val)
            .with_context(|| format!("could not update config key {config_key}"))?;
        Ok(())
    }
}

fn branch_name<'repo>(branch: &'repo Branch) -> Result<&'repo str> {
    branch
        .name()
        .context("HEAD branch has no name")?
        .context("HEAD branch name is not valid utf-8")
}

fn config(repo: &Repository) -> Result<Config> {
    repo.config().context("repo has no config")
}
