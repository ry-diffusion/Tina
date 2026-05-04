// `TinaDb` and its domain-specific impls. Each submodule extends the
// struct with `pub async fn` methods grouped by entity.
//
// Several methods take ≥7 parameters because they bind directly into a
// SQL row — wrapping them in a struct for the sake of clippy would just
// move the column list one indirection away from the `INSERT/UPDATE`
// statement that depends on it. Allow the lint at the module level.
#![allow(clippy::too_many_arguments)]
//
//
//   * `db`              — pool open/migrate
//   * `accounts`        — accounts CRUD
//   * `chats`           — chat resolver, display name, last message,
//                         row queries
//   * `contacts`        — contacts resolver + bulk batch
//   * `groups`          — groups/newsletters + DM lookup helper
//   * `messages`        — single-message read + insert paths
//   * `messages_batch`  — bulk message ingestion (history sync)
//   * `media`           — download status + avatar persistence
//   * `aliases`         — resolver internals shared across submodules
//   * `merge`           — alias-collision merge transactions
//   * `util`            — small SQL/string helpers
//
// `TinaDb::pool` is exposed as the only "raw" surface; everything else
// goes through these typed methods.

mod accounts;
mod aliases;
mod chats;
mod contacts;
mod db;
mod groups;
mod media;
mod merge;
mod messages;
mod messages_batch;
mod util;

pub use db::TinaDb;
