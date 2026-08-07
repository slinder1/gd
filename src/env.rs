// Copyright © 2026 Advanced Micro Devices, Inc. All rights reserved.
// SPDX-License-Identifier: MIT

use crate::cli::Cli;
use crate::util::Extract;
use anyhow::{Context, Result, bail};
use atomic_counter::{AtomicCounter, RelaxedCounter};
use clap::Parser;
use git2::Repository;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, read_to_string};
use std::path::PathBuf;
use thread_local::ThreadLocal;

pub struct ThreadLocalRepo {
    path: PathBuf,
    repo: ThreadLocal<Repository>,
}

impl ThreadLocalRepo {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            repo: ThreadLocal::new(),
        }
    }
    pub fn get(&self) -> Result<&Repository, git2::Error> {
        self.repo.get_or_try(|| Repository::open(&self.path))
    }
}

#[derive(Default, Serialize, Deserialize)]
pub struct Config {
    remote: String,
    tag_remote: String,
    base_branch: String,
    user_branch_prefix: String,
    reviewer_groups: Option<HashMap<String, Vec<String>>>,
}

fn repo_config_path(filename: &str) -> Option<PathBuf> {
    REPO.get()
        .ok()
        .and_then(|r| r.workdir())
        .map(|wd| wd.join(filename))
        .filter(|p| fs::exists(p).unwrap_or(false))
}

fn user_config_path(filename: &str) -> Option<PathBuf> {
    dirs::config_dir()
        .map(|cd| cd.join(filename))
        .filter(|p| fs::exists(p).unwrap_or(false))
}

fn read_config() -> Result<Config> {
    let path = std::env::var_os("CGH_CONFIG_PATH")
        .map(PathBuf::from)
        .or_else(|| repo_config_path(".cgh.toml"))
        .or_else(|| repo_config_path("cgh.toml"))
        .or_else(|| user_config_path("cgh.toml"));
    let path = match path {
        Some(p) => p,
        None => {
            // We will catch this later when the config is validated, but we cannot
            // fail here or -h/--help will not be reached.
            return Ok(Default::default());
        }
    };
    let contents = read_to_string(path.clone())
        .with_context(|| format!("could not read config file: {path:?}"))?;
    let config: Config = toml::from_str(contents.as_ref())
        .with_context(|| format!("invalid config file: {path:?}"))?;
    Ok(config)
}

lazy_static! {
    static ref REPO: ThreadLocalRepo = ThreadLocalRepo::new(".".into());
    static ref CONFIG: Config = read_config().extract();
    static ref CLI: Cli = Cli::parse();
    static ref BASE_BRANCH_REF: String = format!("refs/heads/{}", base_branch());
    static ref EXEC_IDS: RelaxedCounter = RelaxedCounter::new(0);
}

pub fn validate() -> Result<()> {
    if remote().is_empty() {
        bail!("field `remote` cannot be empty");
    }
    if tag_remote().is_empty() {
        bail!("field `tag_remote` cannot be empty");
    }
    if base_branch().is_empty() {
        bail!("field `base_branch` cannot be empty");
    }
    if !user_branch_prefix().is_empty() && !user_branch_prefix().ends_with('/') {
        bail!("if field `user_branch_prefix` is non-empty it must end with `/`");
    }
    Ok(())
}

pub fn cli() -> &'static Cli {
    &CLI
}

pub fn remote() -> &'static str {
    CLI.globals
        .remote
        .as_deref()
        .unwrap_or(CONFIG.remote.as_str())
}

pub fn tag_remote() -> &'static str {
    CLI.globals
        .tag_remote
        .as_deref()
        .unwrap_or(CONFIG.tag_remote.as_str())
}

pub fn base_branch() -> &'static str {
    CLI.globals
        .base_branch
        .as_deref()
        .unwrap_or(CONFIG.base_branch.as_str())
}

pub fn base_branch_ref() -> &'static str {
    BASE_BRANCH_REF.as_ref()
}

pub fn user_branch_prefix() -> &'static str {
    CLI.globals
        .user_branch_prefix
        .as_deref()
        .unwrap_or(CONFIG.user_branch_prefix.as_str())
}

pub fn reviewer_groups() -> Option<&'static HashMap<String, Vec<String>>> {
    CONFIG.reviewer_groups.as_ref()
}

pub fn dry_run() -> bool {
    CLI.globals.dry_run
}

pub fn verbose() -> bool {
    CLI.globals.verbose
}

pub fn always_echo() -> bool {
    dry_run() || verbose()
}

pub fn repo() -> &'static Repository {
    REPO.get().context("not in a git repo").extract()
}

pub fn next_exec_id() -> usize {
    EXEC_IDS.inc()
}
