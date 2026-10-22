//! The Coding area: project-scoped code work with a model, not an IDE.
//!
//! A *coding run* takes a project (its folder is the workspace), a model reached through the
//! Harness Gateway and a task. The model works with the tool engine's file and shell tools
//! (the ones the Playground offers); around it this module adds what makes edits safe to hand
//! over:
//!
//! * [`guard`]: workspace permissions. Edits stay in the project folder and out of `.git`,
//!   shell commands need the user's approval each time and a few are refused outright.
//! * [`checkpoint`]: a restore point before every run and patch (a git tree through a private
//!   index when the folder is a repository, a file snapshot otherwise), the changed-file list
//!   with diffs for the review view, and undo of everything or of single files.
//! * [`patch`]: a unified diff applied as one undoable step.
//! * [`diff`]: unified diffs made and applied.
//! * [`run`]: the run itself: progress events, approvals, the summary of what changed.
//!
//! The Tauri commands (`src-tauri/src/commands/coding.rs`) are thin wrappers over these.

pub mod checkpoint;
pub mod diff;
pub mod guard;
pub mod patch;
pub mod run;

pub use checkpoint::{Backend, ChangeStatus, Checkpoint, FileChange, Store};
pub use guard::{check_command, check_write_path, CommandCheck, WorkspacePermissions};
pub use run::{answer_approval, run_task, Event, RunHandle, RunRequest, RunSummary};
