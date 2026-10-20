//! Mirrors the number of unread agent notifications onto the app's Dock icon, so agents waiting on
//! the user stay visible while Warp is in the background.

use warp_core::features::FeatureFlag;
use warpui::{Entity, ModelContext, SingletonEntity};

use crate::ai::agent_management::notifications::NotificationFilter;
use crate::ai::agent_management::{AgentManagementEvent, AgentNotificationsModel};

/// The Dock truncates long badges, so counts past this collapse to "99+".
const MAX_BADGE_COUNT: usize = 99;

pub struct AgentDockBadgeModel {
    /// The label most recently pushed to the Dock, to skip redundant platform calls.
    current_label: Option<String>,
}

impl Entity for AgentDockBadgeModel {
    type Event = ();
}

impl SingletonEntity for AgentDockBadgeModel {}

impl AgentDockBadgeModel {
    pub(crate) fn new(ctx: &mut ModelContext<Self>) -> Self {
        let notifications = AgentNotificationsModel::handle(ctx);
        ctx.subscribe_to_model(&notifications, |me, _, event, ctx| match event {
            AgentManagementEvent::NotificationAdded { .. }
            | AgentManagementEvent::NotificationUpdated
            | AgentManagementEvent::AllNotificationsMarkedRead => me.sync(ctx),
            AgentManagementEvent::ConversationNeedsAttention { .. } => {}
        });

        Self {
            current_label: None,
        }
    }

    fn sync(&mut self, ctx: &mut ModelContext<Self>) {
        let unread_count = AgentNotificationsModel::as_ref(ctx)
            .notifications()
            .filtered_count(NotificationFilter::Unread);
        let label = FeatureFlag::AgentDeck
            .is_enabled()
            .then(|| badge_label(unread_count))
            .flatten();
        if label != self.current_label {
            ctx.set_dock_badge_label(label.as_deref());
            self.current_label = label;
        }
    }
}

fn badge_label(count: usize) -> Option<String> {
    match count {
        0 => None,
        1..=MAX_BADGE_COUNT => Some(count.to_string()),
        _ => Some(format!("{MAX_BADGE_COUNT}+")),
    }
}

#[cfg(test)]
#[path = "dock_badge_tests.rs"]
mod tests;
