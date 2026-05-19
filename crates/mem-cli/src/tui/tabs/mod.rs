pub(super) mod activity;
pub(super) mod agents;
pub(super) mod embeddings;
pub(super) mod errors;
pub(super) mod memories;
pub(super) mod project;
pub(super) mod query;
pub(super) mod resume;
pub(super) mod review;
pub(super) mod watchers;

use crossterm::event::Event;

use super::app::{App, TabKind};

pub(in crate::tui) struct TabContext {
    pub(in crate::tui) project: String,
    pub(in crate::tui) active_tab: TabKind,
    pub(in crate::tui) error_count: usize,
}

impl TabContext {
    pub(in crate::tui) fn new(app: &App) -> Self {
        Self {
            project: app.project.clone(),
            active_tab: app.active_tab,
            error_count: super::app::error_count(app),
        }
    }
}

pub(in crate::tui) struct TabRenderContext<'a> {
    pub(in crate::tui) app: &'a App,
}

impl<'a> TabRenderContext<'a> {
    pub(in crate::tui) fn new(app: &'a App) -> Self {
        Self { app }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui) enum TabAction {
    None,
    Redraw,
    SwitchTab(TabKind),
    Quit,
}

impl TabAction {
    pub(in crate::tui) fn handled(self) -> bool {
        !matches!(self, Self::None)
    }
}

pub(in crate::tui) fn dispatch_update(
    active_tab: TabKind,
    event: &Event,
    app: &mut App,
) -> TabAction {
    let mut ctx = TabContext::new(app);
    let _ = (&ctx.project, ctx.active_tab);
    let _ = (
        TabAction::Redraw,
        TabAction::SwitchTab(active_tab),
        TabAction::Quit,
    );
    match active_tab {
        TabKind::Memories => memories::update(event, &mut app.memories, &mut ctx),
        TabKind::Agents => agents::update(event, &mut app.agents, &mut ctx),
        TabKind::Query => query::update(event, &mut app.query, &mut ctx),
        TabKind::Activity => activity::update(event, &mut app.activity, &mut ctx),
        TabKind::Errors => errors::update(event, &mut app.errors, &mut ctx),
        TabKind::Project => project::update(event, &mut app.project_tab, &mut ctx),
        TabKind::Review => review::update(event, &mut app.review, &mut ctx),
        TabKind::Watchers => watchers::update(event, &mut app.watchers, &mut ctx),
        TabKind::Embeddings => embeddings::update(event, &mut app.embeddings, &mut ctx),
        TabKind::Resume => resume::update(event, &mut app.resume, &mut ctx),
    }
}
