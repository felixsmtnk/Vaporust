//! Chat and friends: the friends watcher, chat history, sending images.

use super::*;

pub(crate) fn merge_history(history: Vec<ChatLine>, current: Vec<ChatLine>) -> Vec<ChatLine> {
    let newest = history.iter().map(|l| l.time).max().unwrap_or(0);
    let hist_len = history.len();
    let mut used = vec![false; hist_len];
    let mut out = history;
    for l in current {
        if l.time + 120 < newest {
            continue; // old - already in the history
        }
        let m = (0..hist_len).find(|&i| {
            let h = &out[i];
            !used[i]
                && h.from_self == l.from_self
                && if l.exact && h.exact {
                    h.time == l.time && h.ordinal == l.ordinal
                } else {
                    h.text == l.text && h.time.abs_diff(l.time) <= SAME_MESSAGE_SECS
                }
        });
        match m {
            Some(i) => used[i] = true,
            None => out.push(l),
        }
    }
    out.sort_by_key(|l| (l.time, l.ordinal));
    out
}

pub(crate) async fn reload_history(client: &SteamClient, friend: u64, st: &Shared, ctx: &egui::Context) {
    if let Ok((msgs, more)) = client.chat_history(friend, 50, None).await {
        upd(st, ctx, |s| {
            let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
            if !conv.history_loaded {
                conv.oldest = msgs.iter().map(|m| (m.timestamp, m.ordinal)).min();
                conv.more_history = more;
            }
            let history: Vec<ChatLine> = msgs
                .into_iter()
                .rev()
                .map(|m| ChatLine {
                    from_self: m.from_self,
                    text: m.message,
                    time: m.timestamp as u64,
                    ordinal: m.ordinal,
                    exact: true,
                    reactions: m.reactions,
                })
                .collect();
            // Earlier messages loaded with "Show earlier messages" are older than these 50: they stay
            // (replacing the list wiped them and left a hole in the conversation).
            let first = history.first().map(|l| (l.time, l.ordinal));
            let current = std::mem::take(&mut conv.lines);
            let (older, recent): (Vec<ChatLine>, Vec<ChatLine>) = match first {
                Some(f) => current.into_iter().partition(|l| l.exact && (l.time, l.ordinal) < f),
                None => (Vec::new(), current),
            };
            let mut lines = older;
            lines.extend(merge_history(history, recent));
            conv.lines = lines;
            conv.history_loaded = true;
        });
    }
}

/// Read an image for sending: (bytes, type for Steam, width, height).
/// PNG/JPEG/GIF/WebP are sent as is, others (BMP and the like) are re-encoded to PNG.
pub(crate) fn prepare_image(path: &std::path::Path) -> Result<(Vec<u8>, &'static str, u32, u32), String> {
    use image::ImageFormat;
    let bytes = std::fs::read(path).map_err(|e| trf!("Не удалось прочитать файл: {e}", "Could not read the file: {e}"))?;
    if bytes.len() > 20 * 1024 * 1024 {
        return Err(tr!("Файл больше 20 МБ - Steam такой не примет.", "The file is over 20 MB - Steam does not accept it.").into());
    }
    let format = image::guess_format(&bytes).map_err(|_| tr!("Это не картинка (или формат не поддерживается).", "This is not an image (or the format is not supported).").to_string())?;
    let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
    let (w, h) = reader
        .into_dimensions()
        .map_err(|e| trf!("Не удалось прочитать картинку: {e}", "Could not read the image: {e}"))?;
    let ext = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpeg",
        ImageFormat::Gif => "gif",
        ImageFormat::WebP => "webp",
        _ => {
            let img = image::load_from_memory(&bytes).map_err(|e| trf!("Не удалось прочитать картинку: {e}", "Could not read the image: {e}"))?;
            let mut out = std::io::Cursor::new(Vec::new());
            img.write_to(&mut out, ImageFormat::Png)
                .map_err(|e| trf!("Не удалось перекодировать картинку: {e}", "Could not re-encode the image: {e}"))?;
            return Ok((out.into_inner(), "png", w, h));
        }
    };
    Ok((bytes, ext, w, h))
}

/// Background tasks for the session: friend list and incoming messages.
/// Both end by themselves when the connection closes (logout).
pub(crate) fn start_social(client: &SteamClient, roster: Arc<RwLock<Roster>>, st: &Shared, ctx: &egui::Context) {
    let names = roster.clone(); // sender names for notifications
    // Friends: Aurelia announces the status itself ("invisible" by default) and watches for changes.
    {
        let (client, ctx) = (client.clone(), ctx.clone());
        let gen = SESSION_GEN.load(Ordering::Relaxed);
        tokio::spawn(async move {
            // Repaint every 4 s while the watcher runs so friend statuses refresh
            // (more often is not needed: when idle this is the main source of frames).
            // After a logout or another login (SESSION_GEN changes) the watcher is dropped, and with it
            // its copy of the old Steam connection.
            let ticker = async {
                loop {
                    tokio::time::sleep(crate::lite::secs(4, 15)).await;
                    if SESSION_GEN.load(Ordering::Relaxed) != gen {
                        return;
                    }
                    ctx.request_repaint();
                }
            };
            tokio::select! {
                _ = client.run_friends_watcher(roster) => {}
                _ = ticker => {}
            }
        });
    }
    // Incoming messages.
    // Our own listener keeps the server ordinal of each message (steam-vent-chat drops it).
    let Ok(events) = client.chat_events() else {
        return;
    };
    let gen = SESSION_GEN.load(Ordering::Relaxed); // the login number is set by logged_in
    let (st, ctx) = (st.clone(), ctx.clone());
    // Bound to this login (TASK_SESSION): a message that arrives while the account changes is checked
    // under the state lock and dropped, not only by the check at the top of the loop.
    tokio::spawn(TASK_SESSION.scope(gen, async move {
        use aurelia::steam_client::RawChatKind;
        use tokio_stream::StreamExt;
        let mut events = std::pin::pin!(events);
        while let Some(m) = events.next().await {
            if SESSION_GEN.load(Ordering::Relaxed) != gen {
                break;
            }
            let friend = m.friend;
            let (from_self, text) = match m.kind {
                RawChatKind::Message => (false, m.message), // with BBCode: parsed by chatfmt
                // Own message sent from another device (for example from Steam on a phone).
                RawChatKind::Echo => (true, m.message),
                RawChatKind::GameInvite => (false, tr!("[приглашение в игру]", "[game invite]").to_string()),
                RawChatKind::Typing => {
                    upd(&st, &ctx, |s| {
                        Arc::make_mut(&mut s.chats).entry(friend).or_default().typing_at = Some(Instant::now());
                    });
                    continue;
                }
            };
            let line = ChatLine {
                from_self,
                text,
                time: if m.timestamp > 0 { m.timestamp } else { now_unix() },
                ordinal: m.ordinal,
                exact: m.timestamp > 0,
                ..Default::default()
            };
            upd(&st, &ctx, |s| {
                let conv = Arc::make_mut(&mut s.chats).entry(friend).or_default();
                // Our own message comes back as an echo: SendChat may have added it already
                // (same server time and number) - then it is not added twice.
                let dup = line.exact
                    && conv.lines.iter().rev().take(50).any(|l| l.exact && l.from_self == line.from_self && l.time == line.time && l.ordinal == line.ordinal);
                if !dup {
                    conv.lines.push(line);
                }
                conv.typing_at = None;
                if !from_self {
                    conv.unread += 1;
                }
            });
            // The account changed while the message was being written: no notification for the old one.
            if stale_task() {
                break;
            }
            if !from_self {
                // Sound, taskbar flashing, tray dot and a popup notification.
                let (name, accent) = names
                    .read()
                    .ok()
                    .and_then(|r| {
                        r.get(&friend).map(|f| {
                            let in_game = f.game_app_id.is_some_and(|g| g != 0);
                            (f.persona_name.clone(), crate::popup::status_color(f.persona_state, in_game))
                        })
                    })
                    .unwrap_or((None, None));
                let text = conv_text(&st, friend);
                if !text.trim().is_empty() {
                    crate::notify::new_message(friend, name, text, accent);
                }
            }
        }
    }));
}

/// Last message of a conversation (with BBCode; for the notification).
pub(crate) fn conv_text(st: &Shared, friend: u64) -> String {
    let s = st.lock().unwrap();
    s.chats
        .get(&friend)
        .and_then(|c| c.lines.last())
        .map(|l| l.text.clone())
        .unwrap_or_default()
}
