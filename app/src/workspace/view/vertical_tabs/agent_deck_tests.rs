use std::time::Duration;

use instant::Instant;
use warpui::EntityId;

use super::*;
use crate::pane_group::PaneId;

fn entry(activity: AgentActivity, tab_index: usize) -> AgentDeckEntry {
    AgentDeckEntry {
        locator: PaneViewLocator {
            pane_group_id: EntityId::new(),
            pane_id: PaneId::dummy_pane_id(),
        },
        terminal_view_id: EntityId::new(),
        activity,
        agent_name: "Claude Code".to_owned(),
        title: format!("{activity:?} in tab {tab_index}"),
        waiting_since: None,
        is_focused: false,
        tab_index,
    }
}

fn focused(mut entry: AgentDeckEntry) -> AgentDeckEntry {
    entry.is_focused = true;
    entry
}

fn waiting_for(mut entry: AgentDeckEntry, waited: Duration, now: Instant) -> AgentDeckEntry {
    entry.waiting_since = Some(now - waited);
    entry
}

#[test]
fn sort_puts_urgent_agents_first_then_longest_waiting_then_tab_order() {
    let now = Instant::now();
    let mut entries = vec![
        entry(AgentActivity::Idle, 0),
        entry(AgentActivity::Working, 3),
        entry(AgentActivity::Working, 1),
        waiting_for(
            entry(AgentActivity::NeedsInput, 4),
            Duration::from_secs(5),
            now,
        ),
        waiting_for(
            entry(AgentActivity::NeedsInput, 2),
            Duration::from_secs(90),
            now,
        ),
        entry(AgentActivity::Done, 5),
    ];
    sort_entries(&mut entries);

    let order: Vec<(AgentActivity, usize)> = entries
        .iter()
        .map(|entry| (entry.activity, entry.tab_index))
        .collect();
    assert_eq!(
        order,
        vec![
            (AgentActivity::NeedsInput, 2),
            (AgentActivity::NeedsInput, 4),
            (AgentActivity::Done, 5),
            (AgentActivity::Working, 1),
            (AgentActivity::Working, 3),
            (AgentActivity::Idle, 0),
        ]
    );
}

#[test]
fn next_agent_skips_agents_that_do_not_need_attention() {
    let entries = vec![
        entry(AgentActivity::Working, 0),
        entry(AgentActivity::Errored, 1),
        entry(AgentActivity::Idle, 2),
    ];
    assert_eq!(
        next_agent_needing_attention(&entries),
        Some(entries[1].locator)
    );
}

#[test]
fn next_agent_cycles_past_the_focused_agent_and_wraps() {
    let entries = vec![
        entry(AgentActivity::NeedsInput, 0),
        focused(entry(AgentActivity::NeedsInput, 1)),
        entry(AgentActivity::Done, 2),
    ];
    assert_eq!(
        next_agent_needing_attention(&entries),
        Some(entries[2].locator)
    );

    let entries = vec![
        entry(AgentActivity::NeedsInput, 0),
        entry(AgentActivity::Errored, 1),
        focused(entry(AgentActivity::Done, 2)),
    ];
    assert_eq!(
        next_agent_needing_attention(&entries),
        Some(entries[0].locator)
    );
}

#[test]
fn next_agent_is_none_when_only_the_focused_agent_is_waiting() {
    let entries = vec![
        focused(entry(AgentActivity::NeedsInput, 0)),
        entry(AgentActivity::Working, 1),
    ];
    assert_eq!(next_agent_needing_attention(&entries), None);
    assert_eq!(next_agent_needing_attention(&[]), None);
}

#[test]
fn waiting_duration_is_compact() {
    assert_eq!(format_waiting_duration(Duration::from_secs(0)), "now");
    assert_eq!(format_waiting_duration(Duration::from_secs(59)), "now");
    assert_eq!(format_waiting_duration(Duration::from_secs(60)), "1m");
    assert_eq!(format_waiting_duration(Duration::from_secs(59 * 60)), "59m");
    assert_eq!(format_waiting_duration(Duration::from_secs(60 * 60)), "1h");
    assert_eq!(
        format_waiting_duration(Duration::from_secs(5 * 60 * 60 + 59)),
        "5h"
    );
}
