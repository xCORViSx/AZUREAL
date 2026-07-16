//! Deferred session-bubble navigation for partially rendered live histories.

use std::path::PathBuf;

use super::App;
use crate::events::DisplayEvent;

/// A previous-bubble jump waiting for deferred session history to render.
pub(crate) struct PendingSessionBubbleJump {
    /// Zero-based target among bubbles accepted by the navigation mode.
    target_ordinal: usize,
    /// Whether assistant bubbles participate in the requested navigation.
    include_assistant: bool,
    /// Scroll position that must remain unchanged while rendering completes.
    expected_scroll: usize,
    /// Store session that owned the request when navigation began.
    session_id: Option<i64>,
    /// Worktree that owned the request when navigation began.
    worktree_path: Option<PathBuf>,
}

/// Returns whether an event produces a bubble accepted by the navigation mode.
fn event_is_navigable_bubble(event: &DisplayEvent, include_assistant: bool) -> bool {
    match event {
        DisplayEvent::AssistantText { .. } => include_assistant,
        DisplayEvent::UserMessage { content, .. } => {
            !content.starts_with("This session is being continued from a previous conversation")
        }
        _ => false,
    }
}

/// Deferred bubble-navigation operations for application state.
impl App {
    /// Queue a previous-bubble jump when its target precedes the rendered tail.
    ///
    /// Returns `true` when a full-history render was requested and the caller
    /// must leave the current viewport in place until that render completes.
    pub(super) fn defer_previous_bubble_jump(&mut self, include_assistant: bool) -> bool {
        let visible_events_start = self.visible_session_events_start();
        if visible_events_start == 0 {
            return false;
        }

        let prefix_end = visible_events_start.min(self.display_events.len());
        let target_ordinal = self.display_events[..prefix_end]
            .iter()
            .filter(|event| event_is_navigable_bubble(event, include_assistant))
            .count()
            .checked_sub(1);
        let Some(target_ordinal) = target_ordinal else {
            return false;
        };

        self.pending_session_bubble_jump = Some(PendingSessionBubbleJump {
            target_ordinal,
            include_assistant,
            expected_scroll: self.session_scroll,
            session_id: self.current_session_id,
            worktree_path: self
                .current_worktree()
                .and_then(|worktree| worktree.worktree_path.clone()),
        });
        self.invalidate_render_cache();
        true
    }

    /// Complete a previous-bubble jump after deferred history has fully rendered.
    pub(crate) fn complete_pending_session_bubble_jump(&mut self) {
        let Some(pending) = self.pending_session_bubble_jump.take() else {
            return;
        };
        let current_worktree_path = self
            .current_worktree()
            .and_then(|worktree| worktree.worktree_path.as_ref());
        if pending.session_id != self.current_session_id
            || pending.worktree_path.as_ref() != current_worktree_path
            || pending.expected_scroll != self.session_scroll
        {
            return;
        }

        if let Some(&(line_idx, _)) = self
            .message_bubble_positions
            .iter()
            .filter(|(_, is_user)| pending.include_assistant || *is_user)
            .nth(pending.target_ordinal)
        {
            self.session_scroll = line_idx.saturating_sub(2).min(self.session_max_scroll());
        }
    }
}
