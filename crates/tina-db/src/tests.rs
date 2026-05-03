use crate::{ChatKind, MessageBatchInput, TinaDb};

async fn fresh() -> TinaDb {
    let db = TinaDb::in_memory().await.expect("open in-memory db");
    db.create_account("acc1", Some("test"))
        .await
        .expect("create account");
    db
}

const PN: &str = "5511999999999@s.whatsapp.net";
const LID: &str = "11122233344455@lid";
const PN2: &str = "5511888888888@s.whatsapp.net";
const GROUP: &str = "120363400000000001@g.us";
const NEWSLETTER: &str = "120363401000000002@newsletter";

// =================================================================
// ChatKind::infer_from_jid
// =================================================================

#[tokio::test]
async fn infer_kind_from_jid() {
    assert_eq!(ChatKind::infer_from_jid(PN), ChatKind::Dm);
    assert_eq!(ChatKind::infer_from_jid(LID), ChatKind::Dm);
    assert_eq!(ChatKind::infer_from_jid(GROUP), ChatKind::Group);
    assert_eq!(ChatKind::infer_from_jid(NEWSLETTER), ChatKind::Newsletter);
    assert_eq!(
        ChatKind::infer_from_jid("status@broadcast"),
        ChatKind::Status
    );
    assert_eq!(ChatKind::infer_from_jid("foo@unknown"), ChatKind::Unknown);
}

// =================================================================
// register_chat_alias / link_chat
// =================================================================

#[tokio::test]
async fn register_chat_alias_is_idempotent() {
    let db = fresh().await;
    let a = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    let b = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(a, PN);

    let chat = db.get_chat("acc1", PN).await.unwrap().unwrap();
    assert_eq!(chat.kind, "dm");
}

#[tokio::test]
async fn link_chat_creates_alias_for_alt() {
    let db = fresh().await;
    let id = db
        .link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(id, PN);

    // Resolução por LID deve cair no mesmo chat.
    let id2 = db
        .register_chat_alias("acc1", LID, ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(id2, PN);
}

#[tokio::test]
async fn link_chat_merges_separate_chats() {
    let db = fresh().await;
    // Cria dois chats separados (LID-only e PN-only).
    let lid_chat = db
        .register_chat_alias("acc1", LID, ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(lid_chat, LID);

    // Insere mensagens no chat LID.
    db.insert_message(
        "acc1",
        "msg-lid-1",
        &lid_chat,
        None,
        Some("oi"),
        "text",
        100,
        false,
        None,
    )
    .await
    .unwrap();

    let pn_chat = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(pn_chat, PN);
    db.insert_message(
        "acc1",
        "msg-pn-1",
        &pn_chat,
        None,
        Some("ola"),
        "text",
        200,
        false,
        None,
    )
    .await
    .unwrap();

    // Agora descobrimos que são o mesmo chat.
    let final_id = db
        .link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(final_id, PN);

    // Chat antigo (LID) sumiu.
    assert!(db.get_chat("acc1", LID).await.unwrap().is_none());
    // Chat winner (PN) existe.
    assert!(db.get_chat("acc1", PN).await.unwrap().is_some());

    // Mensagens de ambos foram realocadas.
    let n = db.count_messages_for_chat("acc1", PN).await.unwrap();
    assert_eq!(n, 2);

    // Resolver pelos dois aliases retorna o winner.
    assert_eq!(
        db.register_chat_alias("acc1", LID, ChatKind::Dm)
            .await
            .unwrap(),
        PN
    );
    assert_eq!(
        db.register_chat_alias("acc1", PN, ChatKind::Dm)
            .await
            .unwrap(),
        PN
    );
}

#[tokio::test]
async fn group_chat_keeps_jid_as_id() {
    let db = fresh().await;
    let id = db
        .register_chat_alias("acc1", GROUP, ChatKind::Group)
        .await
        .unwrap();
    assert_eq!(id, GROUP);
    let chat = db.get_chat("acc1", GROUP).await.unwrap().unwrap();
    assert_eq!(chat.kind, "group");
}

// =================================================================
// register_contact_alias / link_contact
// =================================================================

#[tokio::test]
async fn register_contact_populates_pn_or_lid() {
    let db = fresh().await;

    let pn_id = db.register_contact_alias("acc1", PN).await.unwrap();
    let pn_contact = db.get_contact("acc1", &pn_id).await.unwrap().unwrap();
    assert_eq!(pn_contact.pn_jid.as_deref(), Some(PN));
    assert_eq!(pn_contact.phone_number.as_deref(), Some("5511999999999"));
    assert!(pn_contact.lid_jid.is_none());

    let lid_id = db.register_contact_alias("acc1", LID).await.unwrap();
    let lid_contact = db.get_contact("acc1", &lid_id).await.unwrap().unwrap();
    assert_eq!(lid_contact.lid_jid.as_deref(), Some(LID));
    assert!(lid_contact.pn_jid.is_none());
}

#[tokio::test]
async fn link_contact_merges_pn_lid_pair() {
    let db = fresh().await;

    // Dois contatos separados aparecem (mensagens em grupos com privacidade
    // distinta, por exemplo).
    let pn_id = db.register_contact_alias("acc1", PN).await.unwrap();
    let lid_id = db.register_contact_alias("acc1", LID).await.unwrap();
    assert_ne!(pn_id, lid_id);

    // Atribui sender a uma mensagem usando o contato LID.
    let chat = db
        .register_chat_alias("acc1", GROUP, ChatKind::Group)
        .await
        .unwrap();
    db.insert_message(
        "acc1",
        "m1",
        &chat,
        Some(&lid_id),
        Some("yo"),
        "text",
        100,
        false,
        None,
    )
    .await
    .unwrap();

    // Descobrimos via push name / participante que PN e LID são a mesma pessoa.
    let winner = db.link_contact("acc1", PN, Some(LID)).await.unwrap();
    assert_eq!(winner, PN);

    // Loser sumiu.
    assert!(db.get_contact("acc1", LID).await.unwrap().is_none());

    // Winner tem ambos os campos preenchidos.
    let merged = db.get_contact("acc1", PN).await.unwrap().unwrap();
    assert_eq!(merged.pn_jid.as_deref(), Some(PN));
    assert_eq!(merged.lid_jid.as_deref(), Some(LID));
    assert_eq!(merged.phone_number.as_deref(), Some("5511999999999"));

    // Mensagem foi reassinada para o winner.
    let msgs = db.get_messages_by_chat("acc1", &chat, 10, 0).await.unwrap();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].sender_contact_id.as_deref(), Some(PN));

    // Resolução por qualquer dos aliases bate no winner.
    assert_eq!(db.register_contact_alias("acc1", PN).await.unwrap(), PN);
    assert_eq!(db.register_contact_alias("acc1", LID).await.unwrap(), PN);
}

#[tokio::test]
async fn upsert_contact_fields_preserves_existing() {
    let db = fresh().await;
    let id = db.register_contact_alias("acc1", PN).await.unwrap();

    db.upsert_contact_fields(
        "acc1",
        &id,
        None,
        None,
        None,
        Some("Pushname"),
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .await
    .unwrap();

    // Segunda chamada com push_name=None NÃO deve apagar.
    db.upsert_contact_fields(
        "acc1",
        &id,
        None,
        None,
        None,
        None,
        Some("Contact Name"),
        None,
        None,
        None,
        None,
        false,
    )
    .await
    .unwrap();

    let c = db.get_contact("acc1", &id).await.unwrap().unwrap();
    assert_eq!(c.push_name.as_deref(), Some("Pushname"));
    assert_eq!(c.contact_name.as_deref(), Some("Contact Name"));
}

// =================================================================
// list_chat_rows: nome de DM via JOIN, nome de grupo via display_name
// =================================================================

#[tokio::test]
async fn dm_chat_name_resolves_via_contact() {
    let db = fresh().await;
    // Registra chat DM.
    let chat = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    // Contato com mesma identidade ganha um push name.
    let cid = db.register_contact_alias("acc1", PN).await.unwrap();
    db.upsert_contact_fields(
        "acc1",
        &cid,
        None,
        None,
        None,
        Some("João"),
        None,
        None,
        None,
        None,
        None,
        false,
    )
    .await
    .unwrap();
    db.update_chat_last_message("acc1", &chat, "msg1", Some("oi"), 100, false, Some(&cid))
        .await
        .unwrap();

    let rows = db.list_chat_rows("acc1").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "João");
    assert_eq!(rows[0].last_message_preview.as_deref(), Some("oi"));
}

#[tokio::test]
async fn group_chat_name_uses_display_name() {
    let db = fresh().await;
    let chat = db
        .register_chat_alias("acc1", GROUP, ChatKind::Group)
        .await
        .unwrap();
    db.set_chat_display_name("acc1", &chat, Some("Time da firma"))
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &chat, "msg1", Some("opa"), 200, false, None)
        .await
        .unwrap();

    let rows = db.list_chat_rows("acc1").await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Time da firma");
    assert_eq!(rows[0].kind, "group");
}

#[tokio::test]
async fn dm_falls_back_to_phone_then_jid() {
    let db = fresh().await;
    let chat = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &chat, "msg1", Some("..."), 100, false, None)
        .await
        .unwrap();

    let rows = db.list_chat_rows("acc1").await.unwrap();
    // Sem contato registrado ainda — JOIN cai no chat_id como último fallback.
    // Quando register_chat_alias cria um chat, ele NÃO cria contato; logo o
    // JOIN não acha registro. O LEFT JOIN deixa name fallback no chat_id.
    assert_eq!(rows[0].name, PN);

    // Agora registra contato e popula phone_number.
    let _cid = db.register_contact_alias("acc1", PN).await.unwrap();
    let rows = db.list_chat_rows("acc1").await.unwrap();
    // phone_number = "5511999999999" (já populado via register_contact_alias).
    assert_eq!(rows[0].name, "5511999999999");
}

#[tokio::test]
async fn list_chat_rows_orders_by_last_message_ts_desc() {
    let db = fresh().await;
    let a = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    let b = db
        .register_chat_alias("acc1", PN2, ChatKind::Dm)
        .await
        .unwrap();
    let g = db
        .register_chat_alias("acc1", GROUP, ChatKind::Group)
        .await
        .unwrap();

    db.update_chat_last_message("acc1", &a, "ma", Some("a"), 100, false, None)
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &b, "mb", Some("b"), 300, false, None)
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &g, "mg", Some("g"), 200, false, None)
        .await
        .unwrap();

    let rows = db.list_chat_rows("acc1").await.unwrap();
    let ids: Vec<&str> = rows.iter().map(|r| r.chat_id.as_str()).collect();
    assert_eq!(ids, vec![PN2, GROUP, PN]);
}

#[tokio::test]
async fn update_chat_last_message_does_not_overwrite_with_older() {
    let db = fresh().await;
    let chat = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();

    db.update_chat_last_message("acc1", &chat, "newer", Some("nova"), 200, true, None)
        .await
        .unwrap();
    // Mensagem mais antiga não deve sobrescrever.
    db.update_chat_last_message("acc1", &chat, "older", Some("velha"), 100, false, None)
        .await
        .unwrap();

    let c = db.get_chat("acc1", &chat).await.unwrap().unwrap();
    assert_eq!(c.last_message_id.as_deref(), Some("newer"));
    assert_eq!(c.last_message_preview.as_deref(), Some("nova"));
    assert!(c.last_message_from_me);
}

// =================================================================
// merge: lookup integrity post-merge
// =================================================================

#[tokio::test]
async fn merged_chat_lookup_via_either_alias() {
    let db = fresh().await;
    db.link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();

    let by_pn = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    let by_lid = db
        .register_chat_alias("acc1", LID, ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(by_pn, by_lid);
}

#[tokio::test]
async fn get_chat_by_alias_finds_via_either_form() {
    let db = fresh().await;
    db.link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();
    let by_pn = db.get_chat_by_alias("acc1", PN).await.unwrap();
    let by_lid = db.get_chat_by_alias("acc1", LID).await.unwrap();
    assert!(by_pn.is_some());
    assert!(by_lid.is_some());
    assert_eq!(by_pn.unwrap().chat_id, by_lid.unwrap().chat_id);
}

#[tokio::test]
async fn merging_chats_preserves_last_message_when_loser_is_newer() {
    let db = fresh().await;
    let lid = db
        .register_chat_alias("acc1", LID, ChatKind::Dm)
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &lid, "m-recent", Some("recente"), 500, false, None)
        .await
        .unwrap();

    let pn = db
        .register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    db.update_chat_last_message("acc1", &pn, "m-old", Some("velha"), 100, false, None)
        .await
        .unwrap();

    // Merge LID → PN: a última mensagem (mais recente, do loser) deve ficar.
    db.link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();
    let merged = db.get_chat("acc1", PN).await.unwrap().unwrap();
    assert_eq!(merged.last_message_id.as_deref(), Some("m-recent"));
    assert_eq!(merged.last_message_ts, Some(500));
}

// =================================================================
// run_message_batch / find_dm_chat_ids_for_aliases
// =================================================================

#[tokio::test]
async fn run_message_batch_dedupes_resolves_and_aggregates_last_message() {
    let db = fresh().await;
    // 3 mensagens no mesmo chat — só 1 register_chat_alias deve rodar.
    let messages = vec![
        MessageBatchInput {
            message_id: "m1",
            chat_jid: PN,
            sender_jid: Some(PN),
            content: Some("oi"),
            message_type: "text",
            timestamp: 100,
            is_from_me: false,
            raw_json: None,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_thumbnail: None,
        },
        MessageBatchInput {
            message_id: "m2",
            chat_jid: PN,
            sender_jid: Some(PN),
            content: Some("ok"),
            message_type: "text",
            timestamp: 300, // mais nova
            is_from_me: false,
            raw_json: None,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_thumbnail: None,
        },
        MessageBatchInput {
            message_id: "m3",
            chat_jid: PN,
            sender_jid: Some(PN),
            content: Some("mid"),
            message_type: "text",
            timestamp: 200,
            is_from_me: false,
            raw_json: None,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_thumbnail: None,
        },
    ];
    let res = db.run_message_batch("acc1", None, &messages).await.unwrap();
    assert_eq!(res.affected_chat_ids, vec![PN.to_string()]);

    // Last message do chat = a com timestamp 300.
    let chat = db.get_chat("acc1", PN).await.unwrap().unwrap();
    assert_eq!(chat.last_message_id.as_deref(), Some("m2"));
    assert_eq!(chat.last_message_ts, Some(300));
    assert_eq!(chat.last_message_preview.as_deref(), Some("ok"));

    let count = db.count_messages_for_chat("acc1", PN).await.unwrap();
    assert_eq!(count, 3);
}

#[tokio::test]
async fn run_message_batch_emits_active_chat_message_ids() {
    let db = fresh().await;
    let messages = vec![
        MessageBatchInput {
            message_id: "m-active-1",
            chat_jid: PN,
            sender_jid: Some(PN),
            content: Some("a"),
            message_type: "text",
            timestamp: 100,
            is_from_me: false,
            raw_json: None,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_thumbnail: None,
        },
        MessageBatchInput {
            message_id: "m-other",
            chat_jid: GROUP,
            sender_jid: None,
            content: Some("b"),
            message_type: "text",
            timestamp: 100,
            is_from_me: true,
            raw_json: None,
            media_mimetype: None,
            media_filename: None,
            media_duration_secs: None,
            media_width: None,
            media_height: None,
            media_size_bytes: None,
            media_sha256: None,
            media_thumbnail: None,
        },
    ];
    let res = db
        .run_message_batch("acc1", Some(PN), &messages)
        .await
        .unwrap();
    assert_eq!(res.active_chat_message_ids, vec!["m-active-1".to_string()]);
    assert_eq!(res.affected_chat_ids.len(), 2);
}

#[tokio::test]
async fn run_message_batch_skips_duplicates_via_insert_or_ignore() {
    let db = fresh().await;
    let msg = MessageBatchInput {
        message_id: "dup",
        chat_jid: PN,
        sender_jid: Some(PN),
        content: Some("primeira"),
        message_type: "text",
        timestamp: 100,
        is_from_me: false,
        raw_json: None,
        media_mimetype: None,
        media_filename: None,
        media_duration_secs: None,
        media_width: None,
        media_height: None,
        media_size_bytes: None,
        media_sha256: None,
        media_thumbnail: None,
    };
    let r1 = db.run_message_batch("acc1", None, &[msg]).await.unwrap();
    assert_eq!(r1.affected_chat_ids.len(), 1);

    // Re-roda com o mesmo ID — INSERT OR IGNORE ⇒ não vira affected.
    let r2 = db.run_message_batch("acc1", None, &[msg]).await.unwrap();
    assert_eq!(r2.affected_chat_ids.len(), 0);

    let n = db.count_messages_for_chat("acc1", PN).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn find_dm_chat_ids_for_aliases_returns_dms_only() {
    let db = fresh().await;
    // 2 DMs e 1 grupo.
    db.register_chat_alias("acc1", PN, ChatKind::Dm)
        .await
        .unwrap();
    db.register_chat_alias("acc1", LID, ChatKind::Dm)
        .await
        .unwrap();
    db.register_chat_alias("acc1", GROUP, ChatKind::Group)
        .await
        .unwrap();

    // Bulk lookup deve trazer só os DMs, mesmo com o alias do grupo na lista.
    let ids = db
        .find_dm_chat_ids_for_aliases("acc1", &[PN, LID, GROUP, "naoexiste@s.whatsapp.net"])
        .await
        .unwrap();
    let mut ids = ids;
    ids.sort();
    assert_eq!(ids, vec![LID.to_string(), PN.to_string()]);
}

#[tokio::test]
async fn find_dm_chat_ids_for_aliases_resolves_via_alias_table() {
    let db = fresh().await;
    // DM criado com PN; alias adicional pra LID.
    db.link_chat("acc1", PN, Some(LID), ChatKind::Dm)
        .await
        .unwrap();

    let by_lid = db
        .find_dm_chat_ids_for_aliases("acc1", &[LID])
        .await
        .unwrap();
    assert_eq!(by_lid, vec![PN.to_string()]); // chat_id = PN, alcançado via alias LID
}

#[tokio::test]
async fn linking_same_form_twice_is_noop() {
    let db = fresh().await;
    let id1 = db
        .link_chat("acc1", PN, Some(PN), ChatKind::Dm)
        .await
        .unwrap();
    let id2 = db
        .link_chat("acc1", PN, Some(PN), ChatKind::Dm)
        .await
        .unwrap();
    assert_eq!(id1, id2);
    assert_eq!(id1, PN);
}
