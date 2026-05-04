// Pure echo-confirmation logic, factored out of `history::handle_append`
// so the matching rules can be exercised without a GTK / relm4 factory.
//
// `confirm_pending_echoes` orchestrates the side effects (read the
// factory, rewrite rows, update `seen_message_ids`); this module owns
// the decision step: given the inbound batch of server rows, the queue
// of pending optimistic locals, and a snapshot of the factory, decide
// which locals get replaced by which server rows.

use std::collections::{HashMap, HashSet, VecDeque};

use tina_db::MessageRow;

/// Output of `match_pending_echoes`. `replacements` is pre-sorted by
/// factory index in DESCENDING order so callers can apply
/// `remove(idx); insert(idx, ...)` from the tail forward without index
/// shifts cascading.
#[derive(Debug, Default)]
pub(in crate::components::chat_tab) struct EchoMatch {
    pub replacements: Vec<(usize, MessageRow, bool)>,
    pub confirmed_server_ids: HashSet<String>,
    pub confirmed_local_ids: Vec<String>,
}

/// Match server rows against the pending-echo queues, reporting which
/// locals get replaced by which server rows. Mutates `pending_echoes`
/// in-place: each matched row pops one entry off its body's queue
/// (FIFO — preserves user-typed order when the same text was sent
/// twice) and removes the queue entry once empty.
///
/// `local_idx_state` is the caller's snapshot of `factory[local_id] →
/// (index, is_collapsed)`. Server rows whose matching local is no
/// longer in the factory (e.g. pruned by soft-cap) are silently
/// dropped from `replacements` but still consume the queue entry so a
/// later genuine echo doesn't double-match.
pub(in crate::components::chat_tab) fn match_pending_echoes(
    rows: &[MessageRow],
    pending_echoes: &mut HashMap<String, VecDeque<String>>,
    local_idx_state: &HashMap<String, (usize, bool)>,
) -> EchoMatch {
    let mut out = EchoMatch::default();
    if pending_echoes.is_empty() {
        return out;
    }
    for r in rows {
        if !r.is_from_me {
            continue;
        }
        let body = r.content.clone().unwrap_or_default();
        if body.is_empty() {
            continue;
        }
        let Some(queue) = pending_echoes.get_mut(&body) else {
            continue;
        };
        let Some(local_id) = queue.pop_front() else {
            continue;
        };
        if queue.is_empty() {
            pending_echoes.remove(&body);
        }
        if let Some((idx, was_collapsed)) = local_idx_state.get(&local_id).copied() {
            out.replacements.push((idx, r.clone(), was_collapsed));
            out.confirmed_server_ids.insert(r.message_id.clone());
            out.confirmed_local_ids.push(local_id);
        }
    }
    out.replacements
        .sort_by_key(|(idx, _, _)| std::cmp::Reverse(*idx));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(message_id: &str, content: &str, from_me: bool) -> MessageRow {
        MessageRow {
            message_id: message_id.to_string(),
            chat_id: "chat-1".into(),
            sender_contact_id: None,
            sender_name: None,
            sender_jid: None,
            sender_avatar_path: None,
            content: Some(content.to_string()),
            message_type: "text".into(),
            timestamp: 0,
            is_from_me: from_me,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_path: None,
            media_status: "none".into(),
            media_thumbnail: None,
        }
    }

    fn pending(entries: &[(&str, &[&str])]) -> HashMap<String, VecDeque<String>> {
        entries
            .iter()
            .map(|(body, locals)| {
                (
                    (*body).to_string(),
                    locals.iter().map(|s| (*s).to_string()).collect(),
                )
            })
            .collect()
    }

    fn factory(entries: &[(&str, usize, bool)]) -> HashMap<String, (usize, bool)> {
        entries
            .iter()
            .map(|(id, idx, collapsed)| ((*id).to_string(), (*idx, *collapsed)))
            .collect()
    }

    #[test]
    fn empty_pending_short_circuits() {
        let mut pe: HashMap<String, VecDeque<String>> = HashMap::new();
        let factory = HashMap::new();
        let rows = vec![row("server-1", "hi", true)];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert!(m.replacements.is_empty());
        assert!(m.confirmed_server_ids.is_empty());
        assert!(m.confirmed_local_ids.is_empty());
    }

    #[test]
    fn ignores_inbound_rows() {
        let mut pe = pending(&[("hi", &["local-1"])]);
        let factory = factory(&[("local-1", 0, false)]);
        let rows = vec![row("server-1", "hi", false)];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert!(m.replacements.is_empty());
        // Inbound row didn't match, so the queue is still pending.
        assert_eq!(pe["hi"].len(), 1);
    }

    #[test]
    fn ignores_empty_content() {
        let mut pe = pending(&[("", &["local-1"])]);
        let factory = factory(&[("local-1", 0, false)]);
        let rows = vec![MessageRow {
            content: None,
            ..row("server-1", "", true)
        }];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert!(m.replacements.is_empty());
    }

    #[test]
    fn matches_single_echo_and_drains_queue() {
        let mut pe = pending(&[("hello", &["local-1"])]);
        let factory = factory(&[("local-1", 7, true)]);
        let rows = vec![row("server-1", "hello", true)];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert_eq!(m.replacements.len(), 1);
        let (idx, r, was_collapsed) = &m.replacements[0];
        assert_eq!(*idx, 7);
        assert_eq!(r.message_id, "server-1");
        assert!(*was_collapsed, "must preserve the local's collapse flag");
        assert_eq!(
            m.confirmed_server_ids,
            HashSet::from(["server-1".to_string()])
        );
        assert_eq!(m.confirmed_local_ids, vec!["local-1".to_string()]);
        assert!(
            !pe.contains_key("hello"),
            "drained queue should be removed entirely"
        );
    }

    #[test]
    fn multiple_echoes_same_body_match_in_send_order() {
        // User sent "ok" twice in a row; server echoes both.
        // Regression guard: the first server row must match the first
        // local (FIFO), not the second — otherwise the collapse-seam
        // bug surfaces because we'd preserve the wrong `was_collapsed`.
        let mut pe = pending(&[("ok", &["local-1", "local-2"])]);
        let factory = factory(&[("local-1", 3, false), ("local-2", 4, true)]);
        let rows = vec![
            row("server-A", "ok", true),
            row("server-B", "ok", true),
        ];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert_eq!(m.replacements.len(), 2);

        // Replacements come back sorted by idx DESC (caller mutates
        // factory tail-first), so the highest-indexed entry is first.
        let by_idx: HashMap<_, _> = m
            .replacements
            .iter()
            .map(|(idx, r, c)| (*idx, (r.message_id.clone(), *c)))
            .collect();
        assert_eq!(by_idx[&3], ("server-A".into(), false));
        assert_eq!(by_idx[&4], ("server-B".into(), true));

        // Sort order: 4 before 3 (DESC).
        assert_eq!(m.replacements[0].0, 4);
        assert_eq!(m.replacements[1].0, 3);

        assert!(!pe.contains_key("ok"));
    }

    #[test]
    fn unmatched_body_is_left_alone() {
        let mut pe = pending(&[("hello", &["local-1"])]);
        let factory = factory(&[("local-1", 0, false)]);
        let rows = vec![row("server-1", "different text", true)];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert!(m.replacements.is_empty());
        assert!(m.confirmed_server_ids.is_empty());
        // Queue must still be pending — only matches consume entries.
        assert_eq!(pe["hello"].len(), 1);
    }

    #[test]
    fn matched_local_missing_from_factory_is_dropped_but_still_pops() {
        // Factory pruned the local (soft-cap removed it). We must still
        // pop the queue entry so a later genuine local with the same
        // body doesn't end up double-matched against an old server row.
        let mut pe = pending(&[("ok", &["local-pruned", "local-current"])]);
        let factory = factory(&[("local-current", 5, false)]);
        let rows = vec![
            row("server-A", "ok", true),
            row("server-B", "ok", true),
        ];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        // server-A consumed local-pruned (which isn't in factory) —
        // produces no replacement but DOES drop the queue entry.
        // server-B then matches local-current, which IS in factory.
        assert_eq!(m.replacements.len(), 1);
        assert_eq!(m.replacements[0].0, 5);
        assert_eq!(m.replacements[0].1.message_id, "server-B");
        assert_eq!(m.confirmed_local_ids, vec!["local-current".to_string()]);
        assert!(!pe.contains_key("ok"));
    }

    #[test]
    fn extra_server_rows_with_exhausted_queue_are_skipped() {
        let mut pe = pending(&[("ok", &["local-1"])]);
        let factory = factory(&[("local-1", 0, false)]);
        let rows = vec![
            row("server-A", "ok", true),
            row("server-B", "ok", true), // no local left
        ];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        assert_eq!(m.replacements.len(), 1);
        assert_eq!(m.replacements[0].1.message_id, "server-A");
    }

    #[test]
    fn preserves_collapse_flag_per_local() {
        // Two pending echoes with different collapse flags; ensure each
        // server row picks up its OWN local's flag (not the other's).
        let mut pe = pending(&[("hi", &["L1", "L2"])]);
        let factory = factory(&[("L1", 0, true), ("L2", 1, false)]);
        let rows = vec![row("S1", "hi", true), row("S2", "hi", true)];
        let m = match_pending_echoes(&rows, &mut pe, &factory);
        let by_id: HashMap<_, _> = m
            .replacements
            .iter()
            .map(|(_, r, c)| (r.message_id.clone(), *c))
            .collect();
        assert!(by_id["S1"]);
        assert!(!by_id["S2"]);
    }
}
