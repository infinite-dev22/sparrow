//! JetBrains-style project switcher popup.
//!
//! New Project / Open / Clone Repository (/ Import Sample), then
//! "Open Projects" and "Recent Projects" with coloured initials avatars.
//! Keyboard: Up/Down to move, Enter to activate, Esc to close. The first row
//! is pre-selected on open, like the reference.

use floem::{
    AnyView, IntoView, View,
    event::EventListener,
    keyboard::{Key, NamedKey},
    kurbo::Point,
    peniko::Color,
    prelude::create_rw_signal,
    reactive::{
        ReadSignal, RwSignal, SignalGet, SignalUpdate, SignalWith, create_effect,
        use_context,
    },
    style::{CursorStyle, Display},
    text::Weight,
    views::{Decorators, container, dyn_stack, empty, label, stack, svg},
};
use std::{path::Path, rc::Rc, sync::Arc};

use crate::{
    command::{SparrowWorkbenchCommand, WindowCommand},
    config::{SparrowConfig, color::SparrowColor, icon::SparrowIcons},
    db::SparrowDb,
    listener::Listener,
    widgets::{divider::Divider, sparrow_scroll},
    window_tab::WindowTabData,
    workspace::SparrowWorkspace,
};

const PANEL_W: f32 = 300.0;
const ROW_H: f32 = 22.0;
const AVATAR: f32 = 20.0;

/// JetBrains shows "Import Sample..." here. Sparrow has no sample importer yet,
/// so the row is only shown once you return a command from this function.
fn import_sample_command() -> Option<SparrowWorkbenchCommand> {
    None
}

// ───────────────────────────── model ─────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    NewProject,
    Open,
    Clone,
    ImportSample,
}

#[derive(Clone)]
enum Item {
    Action {
        kind: Action,
        sel: usize,
    },
    Header(&'static str),
    Sep,
    Project {
        workspace: Arc<SparrowWorkspace>,
        is_current: bool,
        sel: usize,
    },
}

impl Item {
    fn sel(&self) -> Option<usize> {
        match self {
            Item::Action { sel, .. } | Item::Project { sel, .. } => Some(*sel),
            _ => None,
        }
    }

    fn key(&self, i: usize) -> String {
        match self {
            Item::Action { kind, sel } => format!("{i}:a:{kind:?}:{sel}"),
            Item::Header(t) => format!("{i}:h:{t}"),
            Item::Sep => format!("{i}:s"),
            Item::Project {
                workspace,
                is_current,
                sel,
            } => format!("{i}:p:{:?}:{is_current}:{sel}", workspace.path),
        }
    }
}

#[derive(Clone, Copy)]
struct Ctx {
    config: ReadSignal<Arc<SparrowConfig>>,
    window_command: Listener<WindowCommand>,
    workbench_command: Listener<SparrowWorkbenchCommand>,
    visible: RwSignal<bool>,
    /// Index (among selectable rows) of the highlighted row.
    sel: RwSignal<usize>,
}

impl Ctx {
    fn close(&self) {
        self.visible.set(false);
    }

    fn activate(&self, item: &Item) {
        match item {
            Item::Action { kind, .. } => {
                let cmd = match kind {
                    Action::NewProject => Some(SparrowWorkbenchCommand::FlutterNewProject),
                    Action::Open => Some(SparrowWorkbenchCommand::OpenFile),
                    Action::Clone => Some(SparrowWorkbenchCommand::CloneRepository),
                    Action::ImportSample => import_sample_command(),
                };
                if let Some(cmd) = cmd {
                    self.workbench_command.send(cmd);
                }
            }
            Item::Project {
                workspace,
                is_current,
                ..
            } => {
                if !*is_current {
                    self.window_command.send(WindowCommand::OpenProjectTab {
                        workspace: (**workspace).clone(),
                    });
                }
            }
            Item::Header(_) | Item::Sep => return,
        }
        self.close();
    }
}

// ───────────────────────────── colours ─────────────────────────────

fn fg(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::EDITOR_FOREGROUND)
}
fn dim(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::EDITOR_DIM)
}
fn selected_bg(c: &SparrowConfig) -> Color {
    c.color(SparrowColor::SPARROW_BUTTON_PRIMARY_BACKGROUND)
        .multiply_alpha(0.55)
}

// ───────────────────────────── the popup ─────────────────────────────

pub fn project_switcher(
    window_tab_data: Rc<WindowTabData>,
    window_tabs: ReadSignal<im::Vector<(RwSignal<usize>, Rc<WindowTabData>)>>,
    origin: RwSignal<Point>,
) -> impl View {
    let visible = window_tab_data.project_menu_visible;
    let config = window_tab_data.common.config;
    let window_origin = window_tab_data.common.window_origin;
    let title_height = window_tab_data.title_height;
    let current_tab_id = window_tab_data.window_tab_id; // copied out: no Rc in closures
    let db: Arc<SparrowDb> = use_context().unwrap();

    let ctx = Ctx {
        config,
        window_command: window_tab_data.common.window_common.window_command,
        workbench_command: window_tab_data.common.workbench_command,
        visible,
        sel: RwSignal::new(0),
    };

    // Recents (minus already-open projects), refreshed on open.
    let recent_workspaces = create_rw_signal(Vec::<Arc<SparrowWorkspace>>::new());
    create_effect(move |_| {
        if visible.get() {
            ctx.sel.set(0);
            if let Ok(recent) = db.recent_workspaces() {
                let open: Vec<_> = window_tabs.with_untracked(|tabs| {
                    tabs.iter().map(|(_, t)| t.workspace.path.clone()).collect()
                });
                recent_workspaces.set(
                    recent
                        .into_iter()
                        .filter(|ws| !open.contains(&ws.path))
                        .map(Arc::new)
                        .collect(),
                );
            }
        }
    });

    // Flat list of rows; `sel` numbers only the selectable ones.
    let items: RwSignal<Vec<Item>> = RwSignal::new(Vec::new());
    let selectable: RwSignal<usize> = RwSignal::new(0);
    create_effect(move |_| {
        let tabs = window_tabs.get();
        let recent = recent_workspaces.get();

        let mut out: Vec<Item> = Vec::new();
        let mut sel = 0usize;
        let mut push_action = |out: &mut Vec<Item>, kind: Action| {
            out.push(Item::Action { kind, sel });
            sel += 1;
        };
        push_action(&mut out, Action::NewProject);
        push_action(&mut out, Action::Open);
        push_action(&mut out, Action::Clone);
        if import_sample_command().is_some() {
            push_action(&mut out, Action::ImportSample);
        }

        if !tabs.is_empty() {
            out.push(Item::Sep);
            out.push(Item::Header("Open Projects"));
            for (_, tab) in tabs.iter() {
                out.push(Item::Project {
                    workspace: tab.workspace.clone(),
                    is_current: tab.window_tab_id == current_tab_id,
                    sel,
                });
                sel += 1;
            }
        }
        if !recent.is_empty() {
            out.push(Item::Sep);
            out.push(Item::Header("Recent Projects"));
            for ws in recent {
                out.push(Item::Project {
                    workspace: ws,
                    is_current: false,
                    sel,
                });
                sel += 1;
            }
        }
        selectable.set(sel);
        items.set(out);
    });

    let list = dyn_stack(
        move || items.get().into_iter().enumerate().collect::<Vec<_>>(),
        |(i, item)| item.key(*i),
        move |(_, item)| item_view(ctx, item),
    )
        .style(|s| s.flex_col().width_full());

    let step = move |delta: isize| {
        let n = selectable.get_untracked();
        if n > 0 {
            let cur = ctx.sel.get_untracked() as isize;
            ctx.sel.set((cur + delta).rem_euclid(n as isize) as usize);
        }
    };

    let panel = container(
        sparrow_scroll(list).style(|s| s.max_height(520.0).width_full()),
    )
        .keyboard_navigable()
        .on_key_down(
            Key::Named(NamedKey::ArrowDown),
            |m| m.is_empty(),
            move |_| step(1),
        )
        .on_key_down(
            Key::Named(NamedKey::ArrowUp),
            |m| m.is_empty(),
            move |_| step(-1),
        )
        .on_key_down(
            Key::Named(NamedKey::Enter),
            |m| m.is_empty(),
            move |_| {
                let cur = ctx.sel.get_untracked();
                let item = items.with_untracked(|v| {
                    v.iter().find(|i| i.sel() == Some(cur)).cloned()
                });
                if let Some(item) = item {
                    ctx.activate(&item);
                }
            },
        )
        .on_key_down(
            Key::Named(NamedKey::Escape),
            |m| m.is_empty(),
            move |_| ctx.close(),
        )
        .on_click_stop(|_| {}); // clicks inside don't close it

    let panel_id = panel.id();
    create_effect(move |_| {
        if visible.get() {
            panel_id.request_focus();
        }
    });

    let panel = panel.style(move |s| {
        let c = config.get();
        let pt = origin.get() - window_origin.get().to_vec2();
        s.absolute()
            .margin_left(pt.x as f32)
            .margin_top(title_height.get() as f32 + 2.0)
            .width(PANEL_W)
            .padding(8.0)
            .background(c.color(SparrowColor::PANEL_BACKGROUND))
            .border(1.0)
            .border_radius(10.0)
            .border_color(c.color(SparrowColor::SPARROW_BORDER))
            .box_shadow_blur(16.0)
            .box_shadow_v_offset(4.0)
            .box_shadow_color(c.color(SparrowColor::SPARROW_DROPDOWN_SHADOW))
    });

    stack((
        // Click-outside catcher
        empty()
            .on_click_stop(move |_| ctx.close())
            .style(|s| s.absolute().size_full()),
        panel,
    ))
        .style(move |s| {
            s.absolute()
                .size_full()
                .display(if visible.get() {
                    Display::Flex
                } else {
                    Display::None
                })
                .z_index(100)
        })
}

// ───────────────────────────── rows ─────────────────────────────

fn item_view(ctx: Ctx, item: Item) -> AnyView {
    let config = ctx.config;
    match item.clone() {
        Item::Sep => Divider::new(config).spacing(6.0).into_view().into_any(),
        Item::Header(text) => label(move || text.to_string())
            .style(move |s| {
                s.font_size(12.0)
                    .padding_horiz(9.0)
                    .padding_top(2.0)
                    .padding_bottom(4.0)
                    .color(dim(&config.get()))
                    .selectable(false)
            })
            .into_any(),
        Item::Action { kind, sel } => {
            let (text, icon) = match kind {
                Action::NewProject => ("New Project...", None),
                Action::Open => ("Open...", Some(SparrowIcons::DIRECTORY)),
                Action::Clone => ("Clone Repository...", Some(SparrowIcons::SCM)),
                Action::ImportSample => ("Import Sample...", None),
            };
            stack((
                container(
                    svg(move || {
                        icon.map(|i| config.get().ui_svg(i)).unwrap_or_default()
                    })
                        .style(move |s| {
                            s.size(15.0, 15.0).color(fg(&config.get()))
                        }),
                )
                    .style(|s| {
                        s.width(20.0)
                            .flex_shrink(0.0_f32)
                            .items_center()
                    }),
                label(move || text.to_string()).style(move |s| {
                    s.font_size(13.0).color(fg(&config.get())).selectable(false)
                }),
            ))
                .on_event_stop(EventListener::PointerEnter, move |_| ctx.sel.set(sel))
                .on_click_stop(move |_| ctx.activate(&item))
                .style(move |s| row_style(ctx, s, sel).height(ROW_H).items_center())
                .into_any()
        }
        Item::Project {
            workspace,
            is_current: _,
            sel,
        } => {
            let path = workspace.path.as_deref();
            let path_str = path.map(display_path).unwrap_or_default();
            let name = path
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "Untitled".into());
            let initials = initials(&name);
            let color = avatar_color(&name);

            stack((
                container(label(move || initials.clone()).style(|s| {
                    s.font_size(10.0)
                        .font_weight(Weight::BOLD)
                        .color(Color::WHITE)
                        .selectable(false)
                }))
                    .style(move |s| {
                        s.size(AVATAR, AVATAR)
                            .flex_shrink(0.0_f32)
                            .border_radius(5.0)
                            .items_center()
                            .justify_center()
                            .background(color)
                            .margin_right(8.0)
                    }),
                stack((
                    container(label(move || name.clone()).style(move |s| {
                        s.font_size(13.0)
                            .min_width(0.0)
                            .text_ellipsis()
                            .color(fg(&config.get()))
                            .selectable(false)
                    }))
                        .style(|s| s.height(AVATAR).min_width(0.0).items_center()),
                    label(move || path_str.clone()).style(move |s| {
                        s.font_size(12.0)
                            .text_ellipsis()
                            .color(dim(&config.get()))
                            .selectable(false)
                    }),
                ))
                    .style(|s| s.flex_col().flex_grow(1.0_f32).min_width(0.0)),
            ))
                .on_event_stop(EventListener::PointerEnter, move |_| ctx.sel.set(sel))
                .on_click_stop(move |_| ctx.activate(&item))
                .style(move |s| {
                    row_style(ctx, s, sel)
                        .items_start()
                        .padding_vert(4.0)
                })
                .into_any()
        }
    }
}

fn row_style(ctx: Ctx, s: floem::style::Style, sel: usize) -> floem::style::Style {
    let c = ctx.config.get();
    let bg = selected_bg(&c);
    let selected = ctx.sel.get() == sel;
    s.width_full()
        .padding_horiz(9.0)
        .border_radius(5.0)
        .apply_if(selected, |s| s.background(bg))
        .hover(|s| s.cursor(CursorStyle::Pointer))
}

// ───────────────────────────── helpers ─────────────────────────────

/// First letter of the first and last word: `widget-gallery` → `WG`,
/// `sparrow_test_app` → `SA`, `kunlun` → `K`.
fn initials(name: &str) -> String {
    let parts: Vec<&str> = name
        .split(|c: char| c == '-' || c == '_' || c == ' ' || c == '.')
        .filter(|p| !p.is_empty())
        .collect();
    let first = |p: &str| p.chars().next().map(|c| c.to_uppercase().to_string());
    match parts.as_slice() {
        [] => "?".to_string(),
        [one] => first(one).unwrap_or_default(),
        [a, .., z] => format!("{}{}", first(a).unwrap_or_default(), first(z).unwrap_or_default()),
    }
}

fn avatar_color(name: &str) -> Color {
    let hash = name.bytes().fold(0u32, |a, b| a.wrapping_add(b as u32));
    match hash % 5 {
        0 => Color::from_rgb8(0xE0, 0x7A, 0x5F),
        1 => Color::from_rgb8(0x2A, 0xA1, 0xB8),
        2 => Color::from_rgb8(0xB0, 0x4F, 0xC0),
        3 => Color::from_rgb8(0x50, 0xA1, 0x4F),
        _ => Color::from_rgb8(0x7A, 0x4F, 0xE0),
    }
}

/// `/home/me/x` -> `~/x`
fn display_path(path: &Path) -> String {
    let full = path.to_string_lossy().to_string();
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if !home.is_empty() && full.starts_with(&home) {
        format!("~{}", &full[home.len()..])
    } else {
        full
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avatar_initials() {
        assert_eq!(initials("widget-gallery"), "WG");
        assert_eq!(initials("sparrow_test_app"), "SA");
        assert_eq!(initials("kunlun"), "K");
        assert_eq!(initials("untitled"), "U");
        assert_eq!(initials("---"), "?");
    }
}