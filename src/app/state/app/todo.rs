//! Todo item types from Claude's TodoWrite tool call

/// A single todo item from Claude's TodoWrite tool call
#[derive(Clone, Debug)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    pub active_form: String,
}

/// Status of a todo item
#[derive(Clone, Debug, PartialEq)]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}
