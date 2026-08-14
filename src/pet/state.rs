//! Pet state machine (tech §6.1). M2 interaction + M3 reminder.

use tracing::debug;

/// Idle animation identifier (string key into animation library).
pub type IdleAnimation = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReminderStage {
    MovingToCenter,
    Showing,
    Feeding,
    Returning,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PetState {
    Idle(IdleAnimation),
    Watching,
    Reminder(ReminderStage),
    MenuOpen,
    Dragging,
    HiddenAtEdge(Edge),
}

impl PetState {
    pub fn name(&self) -> &'static str {
        match self {
            PetState::Idle(_) => "Idle",
            PetState::Watching => "Watching",
            PetState::Reminder(_) => "Reminder",
            PetState::MenuOpen => "MenuOpen",
            PetState::Dragging => "Dragging",
            PetState::HiddenAtEdge(_) => "HiddenAtEdge",
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, PetState::Idle(_))
    }

    pub fn is_reminder(&self) -> bool {
        matches!(self, PetState::Reminder(_))
    }
}

/// Numeric priority for preemption (PET-08). Higher = stronger.
pub fn state_priority(state: &PetState) -> u8 {
    match state {
        PetState::Dragging => 100,
        PetState::Reminder(_) => 80,
        PetState::MenuOpen => 70,
        PetState::HiddenAtEdge(_) => 50,
        PetState::Watching => 20,
        PetState::Idle(_) => 10,
    }
}

/// Whether `proposed` may interrupt `current` (PET-08).
pub fn can_interrupt(current: &PetState, proposed: &PetState) -> bool {
    state_priority(proposed) > state_priority(current)
}

/// Centralized transitions. Returns Err(rejected_to) if transition is illegal.
pub fn try_transition(from: &PetState, to: PetState) -> Result<PetState, PetState> {
    let allowed = match (from, &to) {
        // -- M1 core --
        (PetState::Idle(_), PetState::Dragging) => true,
        (PetState::Dragging, PetState::Idle(_)) => true,
        (PetState::Idle(_), PetState::Idle(_)) => true,

        // -- M2: mouse interaction --
        (PetState::Idle(_), PetState::Watching) => true,
        (PetState::Watching, PetState::Idle(_)) => true,
        (PetState::Watching, PetState::Dragging) => true,

        // -- M2: edge --
        (PetState::Idle(_), PetState::HiddenAtEdge(_)) => true,
        (PetState::HiddenAtEdge(_), PetState::Idle(_)) => true,
        (PetState::HiddenAtEdge(_), PetState::Dragging) => true,
        (PetState::Dragging, PetState::HiddenAtEdge(_)) => true,

        // -- M3: reminder stages --
        (PetState::Idle(_), PetState::Reminder(ReminderStage::MovingToCenter)) => true,
        (PetState::Watching, PetState::Reminder(ReminderStage::MovingToCenter)) => true,
        (PetState::HiddenAtEdge(_), PetState::Reminder(ReminderStage::MovingToCenter)) => true,
        (
            PetState::Reminder(ReminderStage::MovingToCenter),
            PetState::Reminder(ReminderStage::Showing),
        ) => true,
        (
            PetState::Reminder(ReminderStage::Showing),
            PetState::Reminder(ReminderStage::Feeding),
        ) => true,
        (
            PetState::Reminder(ReminderStage::Feeding),
            PetState::Reminder(ReminderStage::Returning),
        ) => true,
        (PetState::Reminder(ReminderStage::Returning), PetState::Idle(_)) => true,
        // Stage refresh / same discriminant
        (PetState::Reminder(_), PetState::Reminder(_)) => true,
        // RM-07: drag interrupts any reminder stage
        (PetState::Reminder(_), PetState::Dragging) => true,
        // After drag with pending reminder, re-enter
        (PetState::Dragging, PetState::Reminder(ReminderStage::MovingToCenter)) => true,

        // -- M4: menu --
        (PetState::Idle(_), PetState::MenuOpen) => true,
        (PetState::Watching, PetState::MenuOpen) => true,
        (PetState::MenuOpen, PetState::Idle(_)) => true,
        (PetState::MenuOpen, PetState::Dragging) => true,
        (PetState::MenuOpen, PetState::MenuOpen) => true,
        (PetState::HiddenAtEdge(_), PetState::MenuOpen) => true,

        _ => std::mem::discriminant(from) == std::mem::discriminant(&to),
    };

    if allowed {
        debug!(from = from.name(), to = to.name(), "pet state transition");
        Ok(to)
    } else {
        debug!(
            from = from.name(),
            to = to.name(),
            "pet state transition rejected"
        );
        Err(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_to_dragging() {
        assert!(try_transition(&PetState::Idle("x".into()), PetState::Dragging).is_ok());
    }

    #[test]
    fn idle_to_watching() {
        assert!(try_transition(&PetState::Idle("x".into()), PetState::Watching).is_ok());
    }

    #[test]
    fn reminder_stage_flow() {
        let mut s = PetState::Idle("x".into());
        s = try_transition(&s, PetState::Reminder(ReminderStage::MovingToCenter)).unwrap();
        s = try_transition(&s, PetState::Reminder(ReminderStage::Showing)).unwrap();
        s = try_transition(&s, PetState::Reminder(ReminderStage::Feeding)).unwrap();
        s = try_transition(&s, PetState::Reminder(ReminderStage::Returning)).unwrap();
        assert!(try_transition(&s, PetState::Idle("y".into())).is_ok());
    }

    #[test]
    fn reminder_can_be_dragged() {
        assert!(try_transition(
            &PetState::Reminder(ReminderStage::Showing),
            PetState::Dragging
        )
        .is_ok());
    }

    #[test]
    fn reminder_priority_over_watching() {
        assert!(can_interrupt(
            &PetState::Watching,
            &PetState::Reminder(ReminderStage::MovingToCenter)
        ));
    }

    #[test]
    fn dragging_priority_over_reminder() {
        assert!(can_interrupt(
            &PetState::Reminder(ReminderStage::Showing),
            &PetState::Dragging
        ));
    }

    #[test]
    fn watching_to_dragging() {
        assert!(try_transition(&PetState::Watching, PetState::Dragging).is_ok());
    }

    #[test]
    fn priority_ordering() {
        assert!(
            state_priority(&PetState::Dragging)
                > state_priority(&PetState::Reminder(ReminderStage::Showing))
        );
        assert!(
            state_priority(&PetState::Reminder(ReminderStage::Showing))
                > state_priority(&PetState::Watching)
        );
    }
}
