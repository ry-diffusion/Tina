// Shared SQL/string helpers used by every domain submodule.

/// "?,?,?" repeated `n` times (with commas) — for placeholders in
/// dynamically-sized IN(...) / VALUES(...) clauses.
pub(super) fn repeat_csv(token: &str, n: usize) -> String {
    let mut s = String::with_capacity(token.len() * n + n);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push_str(token);
    }
    s
}

pub(super) fn server_of(j: &str) -> &str {
    j.rsplit_once('@').map(|(_, s)| s).unwrap_or("")
}

/// Derive (pn_jid, lid_jid) from the (jid, alt_lid) pair using the
/// server suffix to decide which slot each value belongs in.
pub(super) fn derive_pn_lid(jid: &str, alt: Option<&str>) -> (Option<String>, Option<String>) {
    let mut pn = None;
    let mut lid = None;
    match server_of(jid) {
        "lid" => lid = Some(jid.to_string()),
        "s.whatsapp.net" | "c.us" | "hosted" => pn = Some(jid.to_string()),
        _ => {}
    }
    if let Some(a) = alt {
        match server_of(a) {
            "lid" if lid.is_none() => lid = Some(a.to_string()),
            "s.whatsapp.net" | "c.us" | "hosted" if pn.is_none() => pn = Some(a.to_string()),
            _ => {}
        }
    }
    (pn, lid)
}

pub(super) fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
