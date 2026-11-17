//! Smaller Steam actions (reviews, awards, cart, curators, group chats, Workshop votes).

use super::*;

/// Reread the latest conversation messages (after sending an image).
/// Second-tier commands (profile, chat, community, store, family, Workshop).
pub(crate) async fn second_plan(client: &SteamClient, cmd: Cmd, st: &Shared, ctx: &egui::Context) {
    use crate::actions as a;
    let msg = |r: Result<(), String>, ok: &str| match r {
        Ok(()) => ok.to_string(),
        Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
    };
    let country = {
        let s = st.lock().unwrap().store.clone();
        let c = s.lock().unwrap().country.clone();
        if c.is_empty() { "US".to_string() } else { c }
    };
    match cmd {
        Cmd::ProfileEditLoad => {
            let r = crate::profileedit::load(client).await;
            upd(st, ctx, |s| s.profile_edit = Some(r));
        }
        Cmd::ProfileEditSave(e) => {
            let r = crate::profileedit::save(client, &e).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Профиль сохранен.", "Profile saved.")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::PrivacySave(p) => {
            let r = crate::profileedit::save_privacy(client, &p).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Настройки приватности сохранены.", "Privacy settings saved.")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::AvatarUpload(p) => {
            upd(st, ctx, |s| s.profile_edit_msg = Some(tr!("Загружаю аватар...", "Uploading the avatar...").into()));
            let r = crate::profileedit::upload_avatar(client, &p).await;
            let fresh = crate::profileedit::load(client).await;
            upd(st, ctx, |s| {
                s.profile_edit_msg = Some(msg(r, tr!("Аватар обновлен (в списках друзей появится не сразу).", "Avatar updated (friend lists show it with a delay).")));
                s.profile_edit = Some(fresh);
            });
        }
        Cmd::SetNickname(friend, nick) => {
            let r = crate::profileedit::set_nickname(client, friend, &nick).await;
            upd(st, ctx, |s| {
                if r.is_ok() {
                    if let Some(f) = s.roster.write().unwrap().get_mut(&friend) {
                        f.nickname = if nick.trim().is_empty() { None } else { Some(nick.trim().to_string()) };
                    }
                }
                s.friend_msg = Some(msg(r, if nick.trim().is_empty() { tr!("Ник убран.", "Nickname removed.") } else { tr!("Ник сохранен.", "Nickname saved.") }));
            });
        }
        Cmd::ChatReaction { friend, time, ordinal, kind, name, add } => {
            let r = a::chat_reaction(client, friend, time, ordinal, kind, &name, add).await;
            if let Err(e) = r {
                upd(st, ctx, |s| Arc::make_mut(&mut s.chats).entry(friend).or_default().error = Some(trf!("Реакция не поставлена: {e}", "Reaction not added: {e}")));
            }
            reload_history(client, friend, st, ctx).await;
        }
        Cmd::CreateChatGroup { name, invitees } => {
            let r = a::create_chat_group(client, &name, &invitees).await;
            let groups = client.my_chat_groups().await;
            upd(st, ctx, |s| {
                s.action_msg = Some(match r {
                    Ok(_) => trf!("Групповой чат «{name}» создан, приглашения отправлены.", "Group chat \"{name}\" created, invitations sent."),
                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                });
                if let Ok(g) = groups {
                    s.chat_groups = Some(Ok(g));
                }
            });
        }
        Cmd::InviteToChatGroup { group, friend } => {
            let r = a::invite_to_chat_group(client, group, friend).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Приглашение отправлено.", "Invitation sent."))));
        }
        Cmd::ReviewLoad(appid) => {
            let r = a::review_load(client, appid).await;
            upd(st, ctx, |s| {
                s.my_reviews.insert(appid, r);
            });
        }
        Cmd::ReviewSave { appid, review } => {
            let r = a::review_save(client, appid, &review).await;
            let fresh = a::review_load(client, appid).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Обзор опубликован.", "Review published.")));
                s.my_reviews.insert(appid, fresh);
            });
        }
        Cmd::CreateTopic { appid, title, text } => {
            let r = a::create_topic(client, appid, &title, &text).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(match r {
                    Ok(url) => trf!("Тема создана: {url}", "Topic created: {url}"),
                    Err(e) => trf!("Не получилось: {e}", "Failed: {e}"),
                });
            });
        }
        Cmd::AwardConfig => {
            let r = a::award_config(client).await;
            upd(st, ctx, |s| s.award_config = Some(r));
        }
        Cmd::Award { target_type, target, reaction } => {
            let r = a::award(client, target_type, &target, reaction).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Награда отправлена.", "Award sent."))));
        }
        Cmd::WishlistReorder(items) => {
            let r = a::wishlist_reorder(client, &items).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, tr!("Порядок желаемого сохранен.", "Wishlist order saved."))));
        }
        Cmd::CartLoad => {
            let r = a::cart_get(client, &country).await;
            upd(st, ctx, |s| s.cart = Some(r));
        }
        Cmd::CartAdd(packageid) => {
            let r = a::cart_add(client, packageid, &country).await;
            let cart = a::cart_get(client, &country).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Добавлено в корзину.", "Added to the cart.")));
                s.cart = Some(cart);
            });
        }
        Cmd::CartRemove(line) => {
            let r = a::cart_remove(client, line, &country).await;
            let cart = a::cart_get(client, &country).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Убрано из корзины.", "Removed from the cart.")));
                s.cart = Some(cart);
            });
        }
        Cmd::AppPackages(appid) => {
            let r = a::app_packages(appid, &country).await;
            upd(st, ctx, |s| {
                s.app_packages.insert(appid, r);
            });
        }
        Cmd::Curators(appid) => {
            let r = a::curators_for_app(appid).await;
            let store = st.lock().unwrap().store.clone();
            store.lock().unwrap().curators.insert(appid, r);
            ctx.request_repaint();
        }
        Cmd::FamilyInvite { family, steamid, role } => {
            let r = a::family_invite(client, &family, steamid, role).await;
            let me = client.my_steam_id().unwrap_or(0);
            let fresh = crate::family::load(client, me).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Приглашение в семью отправлено (Steam попросит подтвердить по почте).", "Family invitation sent (Steam asks to confirm by email).")));
                s.family_group = Some(fresh);
            });
        }
        Cmd::FamilyCancel { family, steamid } => {
            let r = a::family_cancel(client, &family, steamid).await;
            let me = client.my_steam_id().unwrap_or(0);
            let fresh = crate::family::load(client, me).await;
            upd(st, ctx, |s| {
                s.action_msg = Some(msg(r, tr!("Приглашение отменено.", "Invitation cancelled.")));
                s.family_group = Some(fresh);
            });
        }
        Cmd::WsCollections { appid, text } => {
            let r = a::ws_collections(client, appid, &text).await;
            upd(st, ctx, |s| {
                s.ws_collections.insert((appid, text), r);
            });
        }
        Cmd::WsCollectionItems(id) => {
            let r = a::ws_collection_items(id).await;
            upd(st, ctx, |s| {
                s.ws_collection_items.insert(id, r);
            });
        }
        Cmd::WsVote { id, up } => {
            let r = a::ws_vote(client, id, up).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, if up { tr!("Вы оценили мод: нравится.", "You rated the mod: like.") } else { tr!("Вы оценили мод: не нравится.", "You rated the mod: dislike.") })));
        }
        Cmd::WsFavorite { id, appid, add } => {
            let r = a::ws_favorite(client, id, appid, add).await;
            upd(st, ctx, |s| s.action_msg = Some(msg(r, if add { tr!("Добавлено в избранное.", "Added to favorites.") } else { tr!("Убрано из избранного.", "Removed from favorites.") })));
        }
        _ => {}
    }
}
