use std::fs;
use std::time::Duration;

use settings::Setting as _;
use warp::cmd_or_ctrl_shift;
use warp::features::FeatureFlag;
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::util::ExactLine;
use warp::integration_testing::terminal::{
    assert_active_block_output_for_single_terminal_in_tab, assert_terminal_bootstrapped,
    execute_long_running_command, wait_until_bootstrapped_single_pane_for_tab,
};
use warp::integration_testing::workspace::assert_focused_tab_index;
use warp::workspace::tab_settings::TabSettings;
use warpui_core::integration::TestStep;
use warpui_core::{SingletonEntity, async_assert};

use crate::Builder;

/// Emits the OSC 777 events a Warp CLI-agent plugin sends, so a pane looks like a plugin-backed
/// agent session without running a real agent. Args: agent, final event, delay, title words.
const AGENT_SIM_SCRIPT: &str = r#"#!/usr/bin/env bash
agent="$1"; final_event="$2"; delay="$3"; shift 3; title="$*"
emit() { printf '\033]777;notify;warp://cli-agent;%s\007' "$1"; }
base="\"v\":1,\"agent\":\"$agent\""
emit "{$base,\"event\":\"session_start\"}"
emit "{$base,\"event\":\"prompt_submit\",\"query\":\"$title\"}"
sleep "$delay"
case "$final_event" in
  blocked) emit "{$base,\"event\":\"permission_request\",\"summary\":\"Wants to run cargo test\",\"tool_name\":\"Bash\",\"tool_input\":{\"command\":\"cargo test -p warp\"}}" ;;
  done) emit "{$base,\"event\":\"stop\",\"response\":\"All tests pass.\"}" ;;
esac
echo "agent-sim: $final_event"
sleep 600
"#;

fn simulate_agent(
    tab_index: usize,
    agent: &str,
    final_event: &str,
    delay_secs: u64,
    title: &str,
) -> TestStep {
    execute_long_running_command(
        tab_index,
        format!("bash ~/agent_sim.sh {agent} {final_event} {delay_secs} {title}"),
    )
}

/// Opens a tab and waits for its shell. The stock bootstrap step can't be used because its default
/// assertions require tab 0 to be idle, and tab 0 is running a simulated agent.
fn open_new_tab(tab_index: usize) -> [TestStep; 2] {
    [
        TestStep::new("Open a new tab").with_keystrokes(&[cmd_or_ctrl_shift("t")]),
        TestStep::new(&format!("Wait for tab {tab_index} to bootstrap"))
            .set_timeout(Duration::from_secs(20))
            .add_named_assertion(
                format!("tab {tab_index} shell is bootstrapped"),
                assert_terminal_bootstrapped(tab_index, 0),
            ),
    ]
}

fn wait_for_agent_event(tab_index: usize, final_event: &'static str) -> TestStep {
    TestStep::new(&format!(
        "Wait for the agent in tab {tab_index} to report '{final_event}'"
    ))
    .set_timeout(Duration::from_secs(30))
    .add_named_assertion(
        format!("tab {tab_index} printed the agent-sim marker"),
        assert_active_block_output_for_single_terminal_in_tab(
            ExactLine::from(format!("agent-sim: {final_event}")),
            tab_index,
        ),
    )
}

/// Boots Warp with vertical tabs and four simulated CLI agents (one blocked on a permission
/// request, one finished but unseen, two working), then captures the agent deck and the agent
/// notification toasts. Run manually with a real display:
///
/// ```sh
/// WARPUI_USE_REAL_DISPLAY_IN_INTEGRATION_TESTS=1 \
///   cargo run -p integration --bin integration -- test_agent_deck_screenshot
/// ```
pub fn test_agent_deck_screenshot() -> Builder {
    FeatureFlag::VerticalTabs.set_enabled(true);
    FeatureFlag::HOANotifications.set_enabled(true);
    FeatureFlag::AgentDeck.set_enabled(true);

    let mut builder = Builder::new()
        .with_real_display()
        .with_setup(|utils| {
            fs::write(utils.test_dir().join("agent_sim.sh"), AGENT_SIM_SCRIPT)
                .expect("should write the agent simulator script");
        })
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Enable vertical tabs").add_named_assertion(
                "vertical tabs setting is on",
                |app, _| {
                    TabSettings::handle(app).update(app, |settings, ctx| {
                        settings
                            .use_vertical_tabs
                            .set_value(true, ctx)
                            .expect("vertical tabs setting should update");
                        async_assert!(*settings.use_vertical_tabs)
                    })
                },
            ),
        )
        .with_step(simulate_agent(
            0,
            "claude",
            "blocked",
            8,
            "Migrate auth to OAuth 2.1",
        ));

    for step in open_new_tab(1) {
        builder = builder.with_step(step);
    }
    builder = builder.with_step(simulate_agent(
        1,
        "gemini",
        "working",
        0,
        "Draft v0.4 release notes",
    ));
    for step in open_new_tab(2) {
        builder = builder.with_step(step);
    }
    builder = builder.with_step(simulate_agent(
        2,
        "opencode",
        "done",
        4,
        "Fix flaky websocket test",
    ));
    for step in open_new_tab(3) {
        builder = builder.with_step(step);
    }

    builder
        .with_step(simulate_agent(
            3,
            "claude",
            "working",
            0,
            "Refactor the agent deck",
        ))
        .with_step(wait_for_agent_event(2, "done"))
        .with_step(wait_for_agent_event(0, "blocked"))
        .with_step(
            TestStep::new("Let entrance motion settle")
                .set_post_step_pause(Duration::from_millis(900)),
        )
        .with_step(TestStep::new("Capture the agent deck").with_take_screenshot("agent_deck.png"))
        .with_step(jump_to_next_agent_needing_attention(0))
        .with_step(
            TestStep::new("Capture after jumping to the blocked agent")
                .with_take_screenshot("agent_deck_after_jump.png"),
        )
        .with_step(jump_to_next_agent_needing_attention(2))
        .with_step(
            TestStep::new("Capture after jumping to the finished agent")
                .with_take_screenshot("agent_deck_after_second_jump.png"),
        )
}

fn jump_to_next_agent_needing_attention(expected_tab_index: usize) -> TestStep {
    TestStep::new("Jump to the next agent needing attention")
        .with_keystrokes(&[if cfg!(target_os = "macos") {
            "cmd-shift-L"
        } else {
            "ctrl-shift-L"
        }])
        .add_named_assertion(
            format!("tab {expected_tab_index} is focused"),
            assert_focused_tab_index(expected_tab_index),
        )
        .set_post_step_pause(Duration::from_millis(600))
}
