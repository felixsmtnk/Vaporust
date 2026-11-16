//! "Experimental features": actions that are barely tested live or visible to other
//! people (selling and trades, version pinning and rollback, comments, votes, awards, groups,
//! blocking, profile editing, badge crafting). Off by default; enabled in
//! "Settings -> Account". Without the switch their buttons are disabled, and the network thread
//! refuses to run them just in case.

use crate::net::Cmd;
use eframe::egui;
use std::sync::atomic::{AtomicBool, Ordering};

static ON: AtomicBool = AtomicBool::new(false);

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

pub fn set(v: bool) {
    ON.store(v, Ordering::Relaxed);
}

pub const HINT: &str = "Экспериментальная функция. Включить: «Настройки -> Аккаунт -> Экспериментальные функции»";

/// Button of an experimental action: disabled without the switch, with a hint why.
pub fn button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_enabled(on(), egui::Button::new(text)).on_disabled_hover_text(HINT)
}

pub fn small_button(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.add_enabled(on(), egui::Button::new(text).small()).on_disabled_hover_text(HINT)
}

/// Explanation instead of a whole block of actions.
pub fn note(ui: &mut egui::Ui, what: &str) {
    ui.weak(format!(
        "{what} - экспериментальная функция, сейчас выключена. Включить: «Настройки -> Аккаунт -> Экспериментальные функции»."
    ));
}

/// Commands the network thread runs only with the switch on.
pub fn guarded(cmd: &Cmd) -> bool {
    use crate::social::SocialCmd;
    use crate::store::StoreCmd;
    use crate::trade::MarketCmd;
    matches!(
        cmd,
        Cmd::GroupMembership { .. }
            | Cmd::PostComment { .. }
            | Cmd::DeleteComment { .. }
            | Cmd::ShotComment { .. }
            | Cmd::ShotVote { .. }
            | Cmd::CommReply { .. }
            | Cmd::CreateTopic { .. }
            | Cmd::Award { .. }
            | Cmd::ReviewSave { .. }
            | Cmd::WishlistReorder(_)
            | Cmd::FamilyInvite { .. }
            | Cmd::FamilyCancel { .. }
            | Cmd::WsVote { .. }
            | Cmd::WsFavorite { .. }
            | Cmd::Downgrade { .. }
            | Cmd::ProfileEditSave(_)
            | Cmd::PrivacySave(_)
            | Cmd::AvatarUpload(_)
            | Cmd::CraftBadge { .. }
            | Cmd::Social(SocialCmd::Block(..))
            | Cmd::Store(StoreCmd::ReviewVote { .. })
            | Cmd::Market(
                MarketCmd::Sell { .. } | MarketCmd::RemoveListing { .. } | MarketCmd::CancelOrder { .. } | MarketCmd::Offer { .. }
            )
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn guard_list() {
        use crate::net::Cmd;
        assert!(super::guarded(&Cmd::WsVote { id: 1, up: true }));
        assert!(super::guarded(&Cmd::Social(crate::social::SocialCmd::Block(1, true))));
        assert!(!super::guarded(&Cmd::Social(crate::social::SocialCmd::AddFriend(1))));
        assert!(!super::guarded(&Cmd::RefreshLibrary));
        assert!(!super::guarded(&Cmd::SendChat { to: 1, text: "x".into() }));
    }
}
