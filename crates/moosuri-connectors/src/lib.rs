//! Bounded external protocol adapters. Application authority stays with the caller.
pub mod audio;
pub mod calendar;
pub mod github_catalog;
pub mod github_skills;
pub mod gmail;
pub mod model;
pub mod parcel;
pub mod source;
pub mod web;

pub mod browser;

pub mod push;

pub mod chatgpt_oauth;

pub mod invitation;

#[cfg(unix)]
pub mod calls;
#[cfg(unix)]
pub mod work_sessions;

#[cfg(unix)]
pub mod claude_code;
