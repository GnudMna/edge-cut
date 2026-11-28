//! UI (`WebView`) とのやり取り
//!
//! UI から届くコマンド (`command`) と、UI へ送る画面状態 (`snapshot`) を定義する

mod command;
mod snapshot;

pub(super) use self::command::{Command, Effect, ResizeEdge, WindowAction, parse_command};
pub(super) use self::snapshot::{BundledView, WindowView};
