//! Type-erased wrappers for database-managed components.

mod feedback;
mod feedback_with_id;
mod input;
mod interrupt;

pub(super) use feedback::{AnyFeedback, FeedbackWrapper};
pub(super) use feedback_with_id::FeedbackWithIdWrapper;
pub(super) use input::{AnyInput, InputWrapper};
pub(super) use interrupt::{AnyInterrupt, InterruptWrapper};
