use std::sync::Arc;

use floem::{View, reactive::ReadSignal};

use crate::{command::InternalCommand, config::SparrowConfig, listener::Listener};

use super::action_link;

pub fn web_link(
    text: impl Fn() -> String + 'static,
    uri: impl Fn() -> String + 'static,
    config: ReadSignal<Arc<SparrowConfig>>,
    internal_command: Listener<InternalCommand>,
) -> impl View {
    action_link(
        text,
        move || internal_command.send(InternalCommand::OpenWebUri { uri: uri() }),
        config,
    )
}
