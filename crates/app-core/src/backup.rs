//! What a backup holds of the store, and putting it back (PROTOCOL §11.5, D-0165).
//!
//! A backup keeps every chat and every message as its `Content` protobuf (as sent or received),
//! with the files this device has, by the writer's canonical order: messages by sent time, then
//! chat, then ID. It never keeps a disappearing message (its timer is a promise to everyone in
//! the chat that it goes), a view-once message, a deleted one, sessions or keys.
//!
//! These run on a backup's own thread, on a second connection to the store
//! ([`Store::open`]; the store is in WAL mode, so it reads while the app writes).

use prost::Message as _;
use rusqlite::params;
use songbird_content::schema::{Content, content::Body};

use crate::messenger::{data_row, view_once_in};
use crate::store::{MessageRow, Status, Store};
use crate::{Error, Result};

/// A message a backup keeps, and the indexes of its files kept on this device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    pub row: MessageRow,
    pub files: Vec<u32>,
}

impl Store {
    /// The chats a backup keeps: (ID, is a group).
    pub fn backup_chats(&self) -> Result<Vec<([u8; 16], bool)>> {
        Ok(self.chats()?.into_iter().map(|c| (c.id, c.group)).collect())
    }

    /// How many messages a backup keeps.
    pub fn backup_count(&self) -> Result<u32> {
        let mut n = 0u32;
        self.backup_messages(|_| {
            n = n.saturating_add(1);
            Ok(())
        })?;
        Ok(n)
    }

    /// Each message a backup keeps, in the canonical order.
    pub fn backup_messages(&self, mut each: impl FnMut(Kept) -> Result<()>) -> Result<()> {
        let mut st = self
            .db_ref()
            .prepare(
                "SELECT chat, author, id, sent_ms, stored_ms, text, content, status FROM messages
                 WHERE expires_ms = 0 AND length(content) > 0
                 ORDER BY sent_ms, chat, id",
            )
            .map_err(|_| Error::Storage)?;
        let mut files = self
            .db_ref()
            .prepare("SELECT idx FROM files WHERE author = ?1 AND id = ?2 ORDER BY idx")
            .map_err(|_| Error::Storage)?;
        let rows = st
            .query_map([], crate::store::row_message)
            .map_err(|_| Error::Storage)?;
        for r in rows {
            let row = r.map_err(|_| Error::Storage)??;
            if view_once_in(&row.content) {
                continue;
            }
            let idx: Vec<u32> = files
                .query_map(params![&row.author[..], &row.id[..]], |r| {
                    r.get::<_, i64>(0)
                })
                .map_err(|_| Error::Storage)?
                .filter_map(|v| v.ok().and_then(|v| u32::try_from(v).ok()))
                .collect();
            each(Kept { row, files: idx })?;
        }
        Ok(())
    }

    /// Puts a chat back (before its messages).
    pub fn restore_chat(&self, id: &[u8; 16], group: bool) -> Result<()> {
        if group {
            self.ensure_group_chat(id)
        } else {
            self.db_ref()
                .execute(
                    "INSERT INTO chats (id, is_group, last_ms, preview, unread) VALUES (?1, 0, 0, '', 0)
                     ON CONFLICT (id) DO NOTHING",
                    [&id[..]],
                )
                .map(|_| ())
                .map_err(|_| Error::Storage)
        }
    }

    /// Puts a message back from its `Content`, as read; its files follow with
    /// [`Store::put_file`]. Answers whether it was new here. A message that isn't a chat
    /// message, or one with a timer or view-once, is refused ([`Error::Malformed`]): a backup
    /// never holds those.
    pub fn restore_message(
        &mut self,
        chat: &[u8; 16],
        author: &[u8; 16],
        content: &[u8],
        stored_ms: u64,
        status: Status,
    ) -> Result<bool> {
        let c = Content::decode(content).map_err(|_| Error::Malformed)?;
        let Some(Body::Data(d)) = &c.body else {
            return Err(Error::Malformed);
        };
        if d.expire_timer_s != 0 || view_once_in(content) {
            return Err(Error::Malformed);
        }
        let mut row = data_row(&c, d, *chat, *author, status)?;
        row.stored_ms = stored_ms;
        // As sent or received: the exact bytes the backup holds.
        row.content = content.to_vec();
        self.insert_message(&row, false)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "tests")]
    use super::*;
    use songbird_content::schema::{DataMessage, Message, Text, data_message};

    fn content(id: u8, text: &str, timer: u32, view_once: bool) -> Vec<u8> {
        Content {
            required_version: 1,
            message_id: vec![id; 16],
            sent_at_ms: u64::from(id) * 1000,
            body: Some(Body::Data(DataMessage {
                expire_timer_s: timer,
                kind: Some(data_message::Kind::Message(Message {
                    text: Some(Text {
                        body: text.into(),
                        ..Default::default()
                    }),
                    view_once,
                    ..Default::default()
                })),
                ..Default::default()
            })),
            ..Default::default()
        }
        .encode_to_vec()
    }

    #[test]
    fn a_backup_keeps_what_it_should_and_puts_it_back() {
        let mut a = Store::memory(&[1; 32]).unwrap();
        let (chat, me) = ([5; 16], [6; 16]);
        for (id, timer, once) in [(1u8, 0, false), (2, 60, false), (3, 0, true), (4, 0, false)] {
            let c = Content::decode(content(id, "hi", timer, once).as_slice()).unwrap();
            let Some(Body::Data(d)) = &c.body else {
                unreachable!()
            };
            let mut row = data_row(&c, d, chat, me, Status::Read).unwrap();
            row.stored_ms = 9;
            a.insert_message(&row, false).unwrap();
        }
        a.put_file(&me, &[4; 16], 0, b"photo").unwrap();
        a.mark_deleted(&me, &[1; 16]).unwrap();
        let mut kept = Vec::new();
        a.backup_messages(|k| {
            kept.push(k);
            Ok(())
        })
        .unwrap();
        // Only #4: #1 deleted, #2 disappearing, #3 view-once.
        assert_eq!(kept.len(), 1);
        assert_eq!((kept[0].row.id, kept[0].files.clone()), ([4; 16], vec![0]));
        assert_eq!(a.backup_count().unwrap(), 1);

        let mut b = Store::memory(&[2; 32]).unwrap();
        b.restore_chat(&chat, false).unwrap();
        let k = &kept[0];
        assert!(
            b.restore_message(&chat, &k.row.author, &k.row.content, 9, k.row.status)
                .unwrap()
        );
        b.put_file(&me, &[4; 16], 0, b"photo").unwrap();
        let back = b.message(&me, &[4; 16]).unwrap().unwrap();
        assert_eq!(
            (back.text.as_str(), back.content.clone()),
            ("hi", k.row.content.clone())
        );
        assert_eq!(
            b.file(&me, &[4; 16], 0).unwrap().unwrap().as_slice(),
            b"photo"
        );
        // Again: nothing new.
        assert!(
            !b.restore_message(&chat, &k.row.author, &k.row.content, 9, k.row.status)
                .unwrap()
        );
        assert_eq!(
            b.restore_message(&chat, &me, &content(7, "x", 30, false), 9, Status::Read),
            Err(Error::Malformed)
        );
    }
}
