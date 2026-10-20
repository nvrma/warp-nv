//! The agent deck: a roster of every agent in the window pinned to the top of the vertical tabs
//! panel, sorted so the agents waiting on the user come first.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use warp_core::ui::Icon as WarpIcon;
use warp_core::ui::theme::WarpTheme;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::shimmering_text::{
    ShimmerConfig, ShimmeringTextElement, ShimmeringTextStateHandle,
};
use warpui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Element, Empty, Expanded,
    Flex, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle, Padding, ParentElement,
    Radius, Text,
};
use warpui::fonts::{Properties, Weight};
use warpui::platform::Cursor;
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, EntityId, SingletonEntity};

use super::{
    TypedPane, agent_tab_text_preference, has_unread_activity_for_terminal_view,
    preferred_agent_tab_titles, terminal_agent_text,
};
use crate::ai::agent_management::AgentNotificationsModel;
use crate::ai::agent_management::notifications::NotificationFilter;
use crate::appearance::Appearance;
use crate::ui_components::agent_beacon::{AgentActivity, AgentBeacon, with_alpha};
use crate::ui_components::agent_icon::terminal_view_agent_icon_variant;
use crate::ui_components::icon_with_status::IconWithStatusVariant;
use crate::workspace::action::WorkspaceAction;
use crate::workspace::{PaneViewLocator, Workspace};

/// Rows beyond this are summarized in a "+N more" line so the deck never crowds out the tab list.
const MAX_VISIBLE_ROWS: usize = 6;
const BEACON_DIAMETER: f32 = 14.;
const ROW_CORNER_RADIUS: f32 = 4.;
const ROW_VERTICAL_PADDING: f32 = 4.;
const ACTIVE_ROW_ACCENT_WIDTH: f32 = 2.;
const SECTION_HORIZONTAL_PADDING: f32 = 8.;
const TITLE_FONT_SIZE: f32 = 12.;
const META_FONT_SIZE: f32 = 11.;
const HEADER_FONT_SIZE: f32 = 11.;
const HEADER_CHEVRON_SIZE: f32 = 12.;
const SUMMARY_DOT_SIZE: f32 = 6.;

/// Per-panel interaction state for the agent deck.
#[derive(Default)]
pub(in crate::workspace::view) struct AgentDeckState {
    pub(in crate::workspace::view) collapsed: bool,
    header_mouse_state: MouseStateHandle,
    row_mouse_states: RefCell<HashMap<EntityId, MouseStateHandle>>,
    working_shimmer_states: RefCell<HashMap<EntityId, ShimmeringTextStateHandle>>,
}

impl AgentDeckState {
    fn row_mouse_state(&self, terminal_view_id: EntityId) -> MouseStateHandle {
        self.row_mouse_states
            .borrow_mut()
            .entry(terminal_view_id)
            .or_default()
            .clone()
    }

    fn working_shimmer_state(&self, terminal_view_id: EntityId) -> ShimmeringTextStateHandle {
        self.working_shimmer_states
            .borrow_mut()
            .entry(terminal_view_id)
            .or_default()
            .clone()
    }

    /// Drops handles for agents that are gone so the maps don't grow for the window's lifetime.
    fn retain_entries(&self, entries: &[AgentDeckEntry]) {
        let is_live = |id: &EntityId| entries.iter().any(|entry| entry.terminal_view_id == *id);
        self.row_mouse_states
            .borrow_mut()
            .retain(|id, _| is_live(id));
        self.working_shimmer_states
            .borrow_mut()
            .retain(|id, _| is_live(id));
    }
}

pub(in crate::workspace) struct AgentDeckEntry {
    pub(in crate::workspace) locator: PaneViewLocator,
    pub(in crate::workspace) terminal_view_id: EntityId,
    pub(in crate::workspace) activity: AgentActivity,
    agent_name: String,
    title: String,
    /// When the agent began waiting on the user, taken from its newest notification. The
    /// notification model replaces a pane's notification on every status change, so the newest
    /// one marks the start of the current wait.
    waiting_since: Option<Instant>,
    pub(in crate::workspace) is_focused: bool,
    tab_index: usize,
}

/// Every agent pane in the workspace, most urgent first. Agents that have waited longest come
/// first within the same activity, then tab order breaks ties.
pub(in crate::workspace) fn collect_agent_deck_entries(
    workspace: &Workspace,
    app: &AppContext,
) -> Vec<AgentDeckEntry> {
    let notifications = AgentNotificationsModel::as_ref(app).notifications();
    let title_preference = agent_tab_text_preference(app);
    let mut entries = Vec::new();

    for (tab_index, tab) in workspace.tabs.iter().enumerate() {
        let pane_group = tab.pane_group.as_ref(app);
        let focused_pane_id = pane_group.focused_pane_id(app);
        for pane_id in pane_group.visible_pane_ids() {
            let TypedPane::Terminal(terminal_pane) = pane_group.resolve_pane_type(pane_id, app)
            else {
                continue;
            };
            let terminal_view = terminal_pane.terminal_view(app);
            let terminal_view = terminal_view.as_ref(app);
            let (agent_name, status) = match terminal_view_agent_icon_variant(terminal_view, app) {
                Some(IconWithStatusVariant::CLIAgent { agent, status, .. }) => {
                    (agent.display_name().to_string(), status)
                }
                Some(IconWithStatusVariant::OzAgent { status, .. }) => {
                    ("Warp Agent".to_string(), status)
                }
                Some(
                    IconWithStatusVariant::Neutral { .. }
                    | IconWithStatusVariant::NeutralElement { .. }
                    | IconWithStatusVariant::CustomAvatar { .. },
                )
                | None => continue,
            };

            let terminal_view_id = terminal_view.id();
            let has_unread = has_unread_activity_for_terminal_view(terminal_view_id, app);
            let activity = AgentActivity::from_status(status.as_ref(), has_unread);
            let waiting_since = activity
                .needs_attention()
                .then(|| {
                    notifications
                        .items_filtered(NotificationFilter::All)
                        .find(|item| item.terminal_view_id == terminal_view_id)
                        .map(|item| item.created_at)
                })
                .flatten();

            // CLI agents are titled by their task prompt rather than their summary, which changes
            // with every tool call.
            let agent_text = terminal_agent_text(terminal_view, app);
            let (conversation_title, _) = preferred_agent_tab_titles(&agent_text, title_preference);
            let title = agent_text
                .cli_agent_latest_user_prompt
                .or(agent_text.cli_agent_title)
                .or(conversation_title)
                .or_else(|| {
                    let pane = pane_group.pane_by_id(pane_id)?;
                    let title = pane
                        .pane_configuration()
                        .as_ref(app)
                        .title()
                        .trim()
                        .to_owned();
                    (!title.is_empty()).then_some(title)
                })
                .unwrap_or_else(|| agent_name.clone());

            entries.push(AgentDeckEntry {
                locator: PaneViewLocator {
                    pane_group_id: tab.pane_group.id(),
                    pane_id,
                },
                terminal_view_id,
                activity,
                agent_name,
                title,
                waiting_since,
                is_focused: tab_index == workspace.active_tab_index && focused_pane_id == pane_id,
                tab_index,
            });
        }
    }

    sort_entries(&mut entries);
    entries
}

fn sort_entries(entries: &mut [AgentDeckEntry]) {
    entries.sort_by(|a, b| {
        a.activity
            .cmp(&b.activity)
            .then_with(|| match (a.waiting_since, b.waiting_since) {
                (Some(a_since), Some(b_since)) => a_since.cmp(&b_since),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.tab_index.cmp(&b.tab_index))
    });
}

/// The next agent to visit when cycling through the agents that need the user: the first one in
/// urgency order after the currently focused agent, wrapping around.
pub(in crate::workspace) fn next_agent_needing_attention(
    entries: &[AgentDeckEntry],
) -> Option<PaneViewLocator> {
    let waiting: Vec<&AgentDeckEntry> = entries
        .iter()
        .filter(|entry| entry.activity.needs_attention())
        .collect();
    let start = waiting
        .iter()
        .position(|entry| entry.is_focused)
        .map_or(0, |focused_index| focused_index + 1);
    waiting
        .iter()
        .cycle()
        .skip(start)
        .take(waiting.len())
        .find(|entry| !entry.is_focused)
        .map(|entry| entry.locator)
}

/// Compact age for a waiting agent, e.g. "now", "4m", "2h".
fn format_waiting_duration(waited: Duration) -> String {
    let minutes = waited.as_secs() / 60;
    match minutes {
        0 => "now".to_owned(),
        1..=59 => format!("{minutes}m"),
        _ => format!("{}h", minutes / 60),
    }
}

/// Renders the deck, or `None` when the window has no agents so the panel stays uncluttered.
pub(in crate::workspace::view) fn render_agent_deck(
    state: &AgentDeckState,
    workspace: &Workspace,
    app: &AppContext,
) -> Option<Box<dyn Element>> {
    let entries = collect_agent_deck_entries(workspace, app);
    state.retain_entries(&entries);
    if entries.is_empty() {
        return None;
    }

    let appearance = Appearance::as_ref(app);
    let theme = appearance.theme();

    let mut section = Flex::column()
        .with_main_axis_size(MainAxisSize::Min)
        .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
        .with_child(render_header(state, &entries, appearance));

    if !state.collapsed {
        let mut rows = Flex::column()
            .with_main_axis_size(MainAxisSize::Min)
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_spacing(1.);
        for entry in entries.iter().take(MAX_VISIBLE_ROWS) {
            rows.add_child(render_row(state, entry, appearance));
        }
        let overflow = entries.len().saturating_sub(MAX_VISIBLE_ROWS);
        if overflow > 0 {
            rows.add_child(
                Container::new(
                    Text::new_inline(
                        format!("+{overflow} more"),
                        appearance.ui_font_family(),
                        META_FONT_SIZE,
                    )
                    .with_color(theme.sub_text_color(theme.background()).into())
                    .finish(),
                )
                .with_padding_left(BEACON_DIAMETER + 16.)
                .with_padding_top(2.)
                .finish(),
            );
        }
        section.add_child(rows.finish());
    }

    Some(
        Container::new(section.finish())
            .with_padding(Padding::uniform(SECTION_HORIZONTAL_PADDING).with_top(6.))
            .with_border(Border::bottom(1.).with_border_color(theme.outline().into()))
            .finish(),
    )
}

fn render_header(
    state: &AgentDeckState,
    entries: &[AgentDeckEntry],
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let font_family = appearance.ui_font_family();
    let sub_text = theme.sub_text_color(theme.background());

    let count_of = |activity: AgentActivity| {
        entries
            .iter()
            .filter(|entry| entry.activity == activity)
            .count()
    };
    let mut summary = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.);
    for activity in [
        AgentActivity::NeedsInput,
        AgentActivity::Errored,
        AgentActivity::Working,
        AgentActivity::Done,
    ] {
        let count = count_of(activity);
        if count > 0 {
            summary.add_child(render_summary_count(activity, count, appearance));
        }
    }

    let chevron = if state.collapsed {
        WarpIcon::ChevronRight
    } else {
        WarpIcon::ChevronDown
    };
    let collapsed = state.collapsed;

    Hoverable::new(state.header_mouse_state.clone(), move |mouse_state| {
        let label_color = if mouse_state.is_hovered() {
            theme.main_text_color(theme.background())
        } else {
            sub_text
        };
        let label = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_spacing(4.)
            .with_child(
                ConstrainedBox::new(chevron.to_warpui_icon(label_color).finish())
                    .with_width(HEADER_CHEVRON_SIZE)
                    .with_height(HEADER_CHEVRON_SIZE)
                    .finish(),
            )
            .with_child(
                Text::new_inline("AGENTS", font_family, HEADER_FONT_SIZE)
                    .with_color(label_color.into())
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .finish(),
            )
            .finish();

        Container::new(
            Flex::row()
                .with_main_axis_size(MainAxisSize::Max)
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(label)
                .with_child(summary.finish())
                .finish(),
        )
        .with_padding(Padding::uniform(4.).with_bottom(if collapsed { 6. } else { 4. }))
        .finish()
    })
    .with_cursor(Cursor::PointingHand)
    .on_click(|ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::ToggleAgentDeck);
    })
    .finish()
}

fn render_summary_count(
    activity: AgentActivity,
    count: usize,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let color = activity.color(theme);
    Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(3.)
        .with_child(
            ConstrainedBox::new(
                Container::new(Empty::new().finish())
                    .with_background_color(color)
                    .with_corner_radius(CornerRadius::with_all(Radius::Percentage(50.)))
                    .finish(),
            )
            .with_width(SUMMARY_DOT_SIZE)
            .with_height(SUMMARY_DOT_SIZE)
            .finish(),
        )
        .with_child(
            Text::new_inline(
                count.to_string(),
                appearance.monospace_font_family(),
                HEADER_FONT_SIZE,
            )
            .with_color(color)
            .finish(),
        )
        .finish()
}

fn render_row(
    state: &AgentDeckState,
    entry: &AgentDeckEntry,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let activity = entry.activity;
    let activity_color = activity.color(theme);
    let locator = entry.locator;
    let is_focused = entry.is_focused;

    let title = Text::new_inline(
        entry.title.clone(),
        appearance.ui_font_family(),
        TITLE_FONT_SIZE,
    )
    .with_clip(ClipConfig::ellipsis())
    .with_color(theme.main_text_color(theme.background()).into())
    .finish();

    let content = Flex::row()
        .with_main_axis_size(MainAxisSize::Max)
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(8.)
        .with_child(
            AgentBeacon::new(activity, activity_color, BEACON_DIAMETER)
                .with_waiting_since(entry.waiting_since)
                .finish(),
        )
        .with_child(Expanded::new(1., title).finish())
        .with_child(render_trailing_label(
            state,
            entry,
            activity_color,
            appearance,
        ))
        .finish();

    Hoverable::new(
        state.row_mouse_state(entry.terminal_view_id),
        move |mouse_state| {
            let mut row = Container::new(content)
                .with_padding(Padding::uniform(ROW_VERTICAL_PADDING).with_horizontal(8.))
                .with_corner_radius(CornerRadius::with_all(Radius::Pixels(ROW_CORNER_RADIUS)));
            if let Some(background) = row_background(is_focused, mouse_state.is_hovered(), theme) {
                row = row.with_background_color(background);
            }
            if is_focused {
                row = row.with_border(
                    Border::left(ACTIVE_ROW_ACCENT_WIDTH).with_border_color(theme.accent().into()),
                );
            }
            row.finish()
        },
    )
    .with_cursor(Cursor::PointingHand)
    .on_click(move |ctx, _, _| {
        ctx.dispatch_typed_action(WorkspaceAction::FocusPane(locator));
    })
    .finish()
}

/// The right edge of a row: the tinted activity, with a shimmer across "Working" while the agent
/// is busy and how long it has waited when it needs the user. Idle agents show their name instead,
/// since there is no state worth calling out.
fn render_trailing_label(
    state: &AgentDeckState,
    entry: &AgentDeckEntry,
    activity_color: ColorU,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let font_family = appearance.ui_font_family();
    let sub_text = theme.sub_text_color(theme.background()).into_solid();

    let label = match entry.activity {
        AgentActivity::Working => ShimmeringTextElement::new(
            AgentActivity::Working.label(),
            font_family,
            META_FONT_SIZE,
            with_alpha(activity_color, 0.65),
            activity_color,
            ShimmerConfig {
                period: Duration::from_millis(1800),
                shimmer_radius: 3,
                padding: 4,
            },
            state.working_shimmer_state(entry.terminal_view_id),
        )
        .finish(),
        AgentActivity::Idle => {
            Text::new_inline(entry.agent_name.clone(), font_family, META_FONT_SIZE)
                .with_color(sub_text)
                .finish()
        }
        AgentActivity::NeedsInput | AgentActivity::Errored | AgentActivity::Done => {
            Text::new_inline(entry.activity.label(), font_family, META_FONT_SIZE)
                .with_color(activity_color)
                .finish()
        }
    };

    let mut trailing = Flex::row()
        .with_cross_axis_alignment(CrossAxisAlignment::Center)
        .with_spacing(6.)
        .with_child(label);
    if let Some(since) = entry.waiting_since {
        trailing.add_child(
            Text::new_inline(
                format_waiting_duration(since.elapsed()),
                appearance.monospace_font_family(),
                META_FONT_SIZE,
            )
            .with_color(sub_text)
            .finish(),
        );
    }
    trailing.finish()
}

fn row_background(is_focused: bool, is_hovered: bool, theme: &WarpTheme) -> Option<ColorU> {
    if is_focused {
        Some(internal_colors::fg_overlay_2(theme).into_solid())
    } else if is_hovered {
        Some(internal_colors::fg_overlay_1(theme).into_solid())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "agent_deck_tests.rs"]
mod tests;
