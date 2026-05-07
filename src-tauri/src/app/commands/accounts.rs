use crate::app::account_profiles::{
    create_profile, load_profiles, remove_profile, rename_profile, save_profiles, set_agent_state,
    AccountProfile, CreateAccountProfileInput, RemoveAccountProfileInput,
    RenameAccountProfileInput, SetAccountAgentStateInput,
};
use crate::app::activity_log;
use crate::app::log_context;
use crate::app::state::AppState;
use crate::app::sync_engine;
use crate::app::sync_runtime;
use std::fs;
use std::path::{Component, PathBuf};
use std::process::Command;

include!("accounts/profile_commands.rs");
include!("accounts/folder_commands.rs");
include!("accounts/global_sync_commands.rs");
