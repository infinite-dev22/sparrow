use crate::widgets::popup_menu::show_context_menu;
use crate::widgets::popup_menu::{Menu, MenuItem};
use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use floem::{
    event::EventPropagation,
    ext_event::create_ext_action,
    keyboard::{Key, KeyEvent, Modifiers, NamedKey},
    reactive::{ReadSignal, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
};
use globset::Glob;
use lapce_xi_rope::Rope;
use sparrow_core::{
    command::{EditCommand, FocusCommand},
    mode::Mode,
    selection::Selection,
};
use sparrow_rpc::{
    file::{
        Duplicating, FileNodeItem, FileNodeViewKind, Naming, NamingState, NewNode,
        Renaming,
    },
    proxy::{PathRequest, ProxyResponse},
};

use crate::{
    command::{CommandExecuted, CommandKind, InternalCommand, SparrowCommand},
    config::SparrowConfig,
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::Editors,
    window_tab::CommonData,
};

fn set_loaded_node_open(node: &mut FileNodeItem, open: bool) {
            if node.is_dir { node.open = open; }
            for child in node.children.values_mut() { set_loaded_node_open(child, open); }
            node.update_node_count(&node.path.clone());
        }

fn relative_name(name: &str) -> Option<&Path> {
    if name.trim().is_empty() || name.contains(['\0', '\r', '\n']) {
        return None;
    }
    let path = Path::new(name);
    (path.file_name().is_some()
        && path.components().all(|part| matches!(part, Component::Normal(_))))
    .then_some(path)
}

#[derive(Clone, Debug)]
pub struct FileExplorerData {
    pub root: RwSignal<FileNodeItem>,
    pub naming: RwSignal<Naming>,
    pub naming_editor_data: EditorData,
    pub common: Rc<CommonData>,
    pub scroll_to_line: RwSignal<Option<f64>>,
    left_diff_path: RwSignal<Option<PathBuf>>,
    pub select: RwSignal<Option<FileNodeViewKind>>,
    file_clipboard: RwSignal<Option<(PathBuf, bool)>>,
}

impl KeyPressFocus for FileExplorerData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        (self.naming.with_untracked(Naming::is_accepting_input)
            && condition == Condition::ModalFocus)
            || matches!(
                condition,
                Condition::PanelFocus | Condition::FileExplorerFocus
            )
    }

    fn run_command(
        &self,
        command: &SparrowCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        if self.naming.with_untracked(Naming::is_accepting_input) {
            match command.kind {
                CommandKind::Focus(FocusCommand::ModalClose) => {
                    self.cancel_naming();
                    CommandExecuted::Yes
                }
                CommandKind::Edit(EditCommand::InsertNewLine) => {
                    self.finish_naming();
                    CommandExecuted::Yes
                }
                _ => self.naming_editor_data.run_command(command, count, mods),
            }
        } else {
            let Some(path) = self
                .select
                .get_untracked()
                .and_then(|kind| kind.path().map(Path::to_path_buf))
            else {
                return CommandExecuted::No;
            };
            match command.kind {
                CommandKind::Edit(EditCommand::ClipboardCopy) => {
                    self.copy_file(path, false)
                }
                CommandKind::Edit(EditCommand::ClipboardCut) => {
                    self.copy_file(path, true)
                }
                CommandKind::Edit(EditCommand::ClipboardPaste) => {
                    self.paste_file(path)
                }
                _ => return CommandExecuted::No,
            }
            CommandExecuted::Yes
        }
    }

    fn receive_char(&self, c: &str) {
        if self.naming.with_untracked(Naming::is_accepting_input) {
            self.naming_editor_data.receive_char(c);
        }
    }
}

impl FileExplorerData {
    pub fn new(cx: Scope, editors: Editors, common: Rc<CommonData>) -> Self {
        let path = common.workspace.path.clone().unwrap_or_default();
        let root = cx.create_rw_signal(FileNodeItem {
            path: path.clone(),
            is_dir: true,
            read: false,
            open: false,
            children: HashMap::new(),
            children_open_count: 0,
        });
        let naming = cx.create_rw_signal(Naming::None);
        let naming_editor_data = editors.make_local(cx, common.clone());
        let data = Self {
            root,
            naming,
            naming_editor_data,
            common,
            scroll_to_line: cx.create_rw_signal(None),
            left_diff_path: cx.create_rw_signal(None),
            select: cx.create_rw_signal(None),
            file_clipboard: cx.create_rw_signal(None),
        };
        if data.common.workspace.path.is_some() {
            // only fill in the child files if there is open folder
            data.toggle_expand(&path);
        }
        data
    }

    fn copy_file(&self, path: PathBuf, cut: bool) {
        if self.common.workspace.path.as_ref() == Some(&path) {
            return;
        }
        use floem::views::editor::{
            core::register::Clipboard, text::SystemClipboard,
        };
        SystemClipboard::new().put_string(path.to_string_lossy());
        self.file_clipboard.set(Some((path, cut)));
    }

    pub fn finish_file_move(&self, source: &Path) {
        if self.file_clipboard.with_untracked(|item| {
            item.as_ref()
                .is_some_and(|(path, cut)| *cut && path == source)
        }) {
            self.file_clipboard.set(None);
        }
    }

    fn clipboard_file(&self) -> Option<(PathBuf, bool)> {
        use floem::views::editor::{
            core::register::Clipboard, text::SystemClipboard,
        };
        let item = self.file_clipboard.get_untracked()?;
        (SystemClipboard::new().get_string().as_deref()
            == Some(item.0.to_string_lossy().as_ref()))
        .then_some(item)
    }

    fn paste_file(&self, target: PathBuf) {
        let Some((source, cut)) = self.clipboard_file() else {
            return;
        };
        let base = if self.is_dir(&target) {
            target
        } else {
            target.parent().unwrap_or(&target).to_path_buf()
        };
        // Name entry also handles conflicts without ever replacing an existing file.
        let data = self.clone();
        let read_base = base.clone();
        self.read_dir_cb(&read_base, move |read| {
            if !read {
                return;
            }
            data.root.update(|root| {
                if let Some(node) = root.get_file_node_mut(&base) {
                    node.open = true;
                }
                root.update_node_count_recursive(&base);
            });
            let is_dir = data.is_dir(&source);
            data.begin_naming(Naming::Duplicating(Duplicating {
                state: NamingState::Naming,
                path: source,
                destination: Some(base),
                is_dir,
                move_on_finish: cut,
                editor_needs_reset: false,
            }));
        });
    }

    pub fn rename_path(&self, path: PathBuf) {
        if self.common.workspace.path.as_ref() == Some(&path) {
            return;
        }
        self.begin_naming(Naming::Renaming(Renaming {
            state: NamingState::Naming,
            path,
            editor_needs_reset: false,
        }));
    }

    fn request_rename(&self, path: PathBuf) {
        if !self.common.workspace.kind.is_remote() && crate::dart::is_dart_path(&path) {
            self.common.sparrow_command.send(SparrowCommand {
                kind: CommandKind::Workbench(crate::command::SparrowWorkbenchCommand::RenameActiveFile),
                data: Some(serde_json::json!(path)),
            });
        } else {
            self.rename_path(path);
        }
    }

    /// Rename the currently selected file, if it exists.
    pub fn rename_selected(&self) {
        if let Some(path) = self.select.get_untracked().and_then(|file| {
            file.path().map(Path::to_path_buf)
        }) {
            self.request_rename(path);
        }
    }

    /// Trash the currently selected file, if it exists.
    pub fn trash_selected(&self) {
        if let Some(file) = self.select.get_untracked() {
            if let Some(path) = file.path() {
                self.common.proxy.trash_path(path.to_path_buf(), |res| {
                    if let Err(err) = res {
                        tracing::warn!("Failed to trash path: {:?}", err);
                    }
                })
            }
        }
    }

    /// Reload the file explorer data via reading the root directory.  
    /// Note that this will not update immediately.
    pub fn reload(&self) {
        let path = self.root.with_untracked(|root| root.path.clone());
        self.read_dir(&path);
    }

    pub fn refresh_created_path(&self, path: &Path) {
        let base = self.root.with_untracked(|root| {
            path.ancestors().skip(1).find(|parent| {
                root.get_file_node(parent).is_some_and(|node| node.is_dir)
            }).map(Path::to_path_buf)
        });
        if let Some(base) = base {
            let data = self.clone();
            let path = path.to_path_buf();
            self.read_dir_cb(&base, move |read| {
                if read {
                    data.reveal_in_file_tree(path);
                }
            });
        }
    }

    /// Toggle whether the directory is expanded or not.  
    /// Does nothing if the path does not exist or is not a directory.
    pub fn toggle_expand(&self, path: &Path) {
        let Some(Some(read)) = self.root.try_update(|root| {
            let read = if let Some(node) = root.get_file_node_mut(path) {
                if !node.is_dir {
                    return None;
                }
                node.open = !node.open;
                Some(node.read)
            } else {
                None
            };

            if Some(true) == read {
                root.update_node_count_recursive(path);
            }
            read
        }) else {
            return;
        };

        // Read the directory's files if they haven't been read yet
        if !read {
            self.read_dir(path);
        }
    }

    pub fn read_dir(&self, path: &Path) {
        self.read_dir_cb(path, |_| {});
    }

    /// Read the directory's information and update the file explorer tree.  
    /// `done : FnOnce(was_read: bool)` is called when the operation is completed, whether success,
    /// failure, or ignored.
    pub fn read_dir_cb(&self, path: &Path, done: impl FnOnce(bool) + 'static) {
        let root = self.root;
        let data = self.clone();
        let config = self.common.config;
        let send = {
            let path = path.to_path_buf();
            create_ext_action(self.common.scope, move |result| {
                let Ok(ProxyResponse::ReadDirResponse { mut items, .. }) = result
                else {
                    done(false);
                    return;
                };

                root.update(|root| {
                    // Get the node for this path, which should already exist if we're calling
                    // read_dir on it.
                    if let Some(node) = root.get_file_node_mut(&path) {
                        // TODO: do not recreate glob every time we read a directory
                        // Retain only items that are not excluded from view by the configuration
                        match Glob::new(&config.get().editor.files_exclude) {
                            Ok(glob) => {
                                let matcher = glob.compile_matcher();
                                items.retain(|i| !matcher.is_match(&i.path));
                            }
                            Err(e) => tracing::error!(
                                target:"files_exclude",
                                "Failed to compile glob: {}",
                                e
                            ),
                        }

                        node.read = true;

                        // Remove paths that no longer exist
                        let removed_paths: Vec<PathBuf> = node
                            .children
                            .keys()
                            .filter(|p| !items.iter().any(|i| &&i.path == p))
                            .map(PathBuf::from)
                            .collect();
                        for path in removed_paths {
                            node.children.remove(&path);
                        }

                        // Reread dirs that were already read and add new paths
                        for item in items {
                            if let Some(existing) = node.children.get(&item.path) {
                                if existing.read {
                                    data.read_dir(&existing.path);
                                }
                            } else {
                                node.children.insert(item.path.clone(), item);
                            }
                        }
                    }
                    root.update_node_count_recursive(&path);
                });

                done(true);
            })
        };

        // Ask the proxy for the directory information
        self.common
            .proxy
            .read_dir(PathRequest::Path(path.to_path_buf()), send);
    }

    /// Returns `true` if `path` exists in the file explorer tree and is a directory, `false`
    /// otherwise.
    fn is_dir(&self, path: &Path) -> bool {
        self.root.with_untracked(|root| {
            root.get_file_node(path).is_some_and(|node| node.is_dir)
        })
    }

    pub fn begin_naming(&self, naming: Naming) {
        if self.naming.with_untracked(|current| {
            matches!(current.state(), Some(NamingState::Pending))
        }) {
            return;
        }
        let (path, is_dir) = match &naming {
            Naming::None => return,
            Naming::Renaming(rename) => (Some(&rename.path), self.is_dir(&rename.path)),
            Naming::NewNode(node) => (None, node.is_dir),
            Naming::Duplicating(copy) => (Some(&copy.path), copy.is_dir),
        };
        let name = path
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let selection_end = if is_dir {
            name.len()
        } else {
            let base = name.strip_prefix('.').unwrap_or(&name);
            name.len() - base.len() + base.find('.').unwrap_or(base.len())
        };
        self.naming_editor_data.doc().reload(Rope::from(name.as_ref()), true);
        self.naming_editor_data.cursor().update(|cursor| {
            cursor.set_insert(Selection::region(0, selection_end));
        });
        self.naming.set(naming);
    }

    pub fn create_entry(&self, base_path: PathBuf, is_dir: bool) {
        self.begin_naming(Naming::NewNode(NewNode {
            state: NamingState::Naming,
            base_path,
            is_dir,
            editor_needs_reset: false,
        }));
    }

    /// Choose the directory for File → New using the Project selection or active editor.
    pub fn creation_base(&self, editor_path: Option<&Path>) -> Option<PathBuf> {
        let root = self.common.workspace.path.as_deref()?;
        let selected = self.select.get_untracked();
        let base = if self.common.focus.get_untracked() == crate::window_tab::Focus::Panel(
            crate::panel::kind::PanelKind::FileExplorer,
        ) {
            selected.as_ref().and_then(FileNodeViewKind::path)
        } else {
            editor_path
        };
        let base = base.filter(|path| path.starts_with(root)).unwrap_or(root);
        Some(if base == root || self.is_dir(base) {
            base.to_path_buf()
        } else {
            base.parent().unwrap_or(root).to_path_buf()
        })
    }

    pub fn finish_naming(&self) {
        let naming = self.naming.get_untracked();
        if !naming.is_accepting_input() {
            return;
        }
        let content = self.naming_editor_data.text();
        let name = content.slice_to_cow(..);
        let Some(relative) = relative_name(&name) else {
            self.naming.update(|naming| {
                naming.set_err("Enter a relative name without '..' or line breaks.".into())
            });
            return;
        };

        let command = match naming {
            Naming::None => return,
            Naming::Renaming(rename) => {
                let Some(parent) = rename.path.parent() else { return };
                let new_path = parent.join(relative);
                if new_path == rename.path {
                    self.cancel_naming();
                    return;
                }
                InternalCommand::FinishRenamePath { current_path: rename.path, new_path }
            }
            Naming::NewNode(node) => InternalCommand::FinishNewNode {
                is_dir: node.is_dir,
                path: node.base_path.join(relative),
            },
            Naming::Duplicating(copy) => {
                let Some(parent) = copy.destination.as_deref().or_else(|| copy.path.parent())
                    else { return };
                let path = parent.join(relative);
                if copy.move_on_finish {
                    InternalCommand::FinishRenamePath { current_path: copy.path, new_path: path }
                } else {
                    InternalCommand::FinishDuplicate { source: copy.path, path }
                }
            }
        };
        self.naming.update(Naming::set_pending);
        self.common.internal_command.send(command);
    }

    pub fn cancel_naming(&self) {
        if self.naming.with_untracked(Naming::is_accepting_input) {
            self.naming.set(Naming::None);
        }
    }

    /// Project-tree navigation keeps focus in Project until a file is opened.
    pub fn navigate_key(&self, key: &KeyEvent) -> bool {
        if self.naming.with_untracked(Naming::is_accepting_input)
            || !key.modifiers.is_empty()
        {
            return false;
        }
        let selected = self
            .select
            .get_untracked()
            .and_then(|kind| kind.path().map(Path::to_path_buf));
        let nodes = self.root.with_untracked(visible_project_nodes);
        if nodes.is_empty() {
            return false;
        }
        let index = selected
            .as_ref()
            .and_then(|path| nodes.iter().position(|node| &node.0 == path))
            .unwrap_or(0);
        let (path, is_dir, open) = &nodes[index];
        let mut next = Some(index);
        match &key.key.logical_key {
            Key::Named(NamedKey::ArrowDown) => {
                next = Some((index + 1).min(nodes.len() - 1))
            }
            Key::Named(NamedKey::ArrowUp) => next = Some(index.saturating_sub(1)),
            Key::Named(NamedKey::Home) => next = Some(0),
            Key::Named(NamedKey::End) => next = Some(nodes.len() - 1),
            Key::Named(NamedKey::ArrowRight) => {
                if *is_dir && !*open {
                    self.toggle_expand(path);
                } else if *is_dir {
                    if nodes.get(index + 1)
                        .is_some_and(|child| child.0.parent() == Some(path.as_path()))
                    {
                        next = Some(index + 1);
                    }
                }
            }
            Key::Named(NamedKey::ArrowLeft) => {
                if *is_dir && *open {
                    self.toggle_expand(path);
                } else {
                    next = path.parent().and_then(|parent| {
                        nodes.iter().position(|node| node.0 == parent)
                    });
                }
            }
            Key::Named(NamedKey::Enter | NamedKey::F4) => {
                if *is_dir {
                    self.toggle_expand(path);
                } else {
                    self.common.focus.set(crate::window_tab::Focus::Workbench);
                    self.common.internal_command.send(
                        InternalCommand::OpenAndConfirmedFile { path: path.clone() },
                    );
                }
            }
            _ => return false,
        }
        if let Some(index) = next {
            self.select
                .set(Some(FileNodeViewKind::Path(nodes[index].0.clone())));
            self.scroll_to_line.set(Some(index as f64));
        }
        true
    }

    pub fn click(&self, path: &Path, config: ReadSignal<Arc<SparrowConfig>>) {
        if self.is_dir(path) {
            self.toggle_expand(path);
        } else if !config.get_untracked().core.file_explorer_double_click {
            self.common
                .internal_command
                .send(InternalCommand::OpenFile {
                    path: path.to_path_buf(),
                })
        }
    }

    pub fn reveal_in_file_tree(&self, path: PathBuf) {
        let done = self
            .root
            .try_update(|root| {
                // the directories in which the file are located are all read and opened
                if root.get_file_node(&path).is_some() {
                    for current_path in path.ancestors() {
                        if let Some(file) = root.get_file_node_mut(current_path) {
                            if file.is_dir {
                                file.open = true;
                            }
                        }
                    }
                    root.update_node_count_recursive(&path);
                    true
                } else {
                    // read and open the directories in which the file are located
                    let mut read_dir = None;
                    // Whether the file is in the workspace
                    let mut exist = false;
                    for current_path in path.ancestors() {
                        if let Some(file) = root.get_file_node_mut(current_path) {
                            exist = true;
                            if file.is_dir {
                                file.open = true;
                                if !file.read {
                                    read_dir = Some(current_path.to_path_buf())
                                }
                            }
                            break;
                        }
                    }
                    if let (true, Some(dir)) = (exist, read_dir) {
                        let explorer = self.clone();
                        let select_path = path.clone();
                        self.read_dir_cb(&dir, move |_| {
                            explorer.reveal_in_file_tree(select_path);
                        })
                    }
                    false
                }
            })
            .unwrap_or(false);
        if done {
            let (found, line) =
                self.root.with_untracked(|x| x.find_file_at_line(&path));
            if found {
                self.scroll_to_line.set(Some(line));
                self.select.set(Some(FileNodeViewKind::Path(path)));
            }
        }
    }

    pub fn double_click(
        &self,
        path: &Path,
        config: ReadSignal<Arc<SparrowConfig>>,
    ) -> EventPropagation {
        if !self.is_dir(path) {
            self.common.focus.set(crate::window_tab::Focus::Workbench);
        }
        if self.is_dir(path) {
            EventPropagation::Continue
        } else if config.get_untracked().core.file_explorer_double_click {
            self.common.internal_command.send(
                InternalCommand::OpenAndConfirmedFile {
                    path: path.to_path_buf(),
                },
            );
            EventPropagation::Stop
        } else {
            self.common
                .internal_command
                .send(InternalCommand::MakeConfirmed);
            EventPropagation::Stop
        }
    }

    pub fn creation_menu(&self, base_path_a: PathBuf) -> Menu {
        let common = self.common.clone();
        let workspace_path = self.common.workspace.path.as_deref().unwrap_or(&base_path_a);
        let mut new_menu = Menu::new("New");

        for (title, is_dir) in [("File…", false), ("Directory…", true)] {
            let base_path = base_path_a.clone();
            let data = self.clone();
            new_menu = new_menu.entry(MenuItem::new(title).action(move || {
                data.create_entry(base_path.clone(), is_dir);
            }));
        }

        new_menu = new_menu.separator();
        let mut state_menu=Menu::new("State Management");
        let scaffold_root = crate::scaffold::engine::context(workspace_path,&base_path_a).map(|ctx|ctx.root).unwrap_or_else(|_|workspace_path.to_path_buf());
        for template in crate::scaffold::catalog(&scaffold_root) {
            let base=base_path_a.clone(); let commands=common.internal_command;
            let is_state = template.category==crate::scaffold::engine::Category::State;
            let item=MenuItem::new(template.title.clone()).action(move ||commands.send(InternalCommand::NewScaffold{base:base.clone(),template:template.clone()}));
            if is_state {state_menu=state_menu.entry(item);} else {new_menu=new_menu.entry(item);}
        }
        new_menu.entry(state_menu)
    }

    pub fn set_tree_open(&self, open: bool) {
        self.root.update(|root| {
            set_loaded_node_open(root, open);
            root.open = true;
            root.update_node_count(&root.path.clone());
        });
    }

    pub fn secondary_click(&self, path: &Path) {
        self.select
            .set(Some(FileNodeViewKind::Path(path.to_path_buf())));
        self.common.focus.set(crate::window_tab::Focus::Panel(
            crate::panel::kind::PanelKind::FileExplorer,
        ));
        let common = self.common.clone();
        let path_a = path.to_owned();
        let left_diff_path = self.left_diff_path;
        // TODO: should we just pass is_dir into secondary click?
        let is_dir = self.is_dir(path);

        let Some(workspace_path) = self.common.workspace.path.as_ref() else {
            // There is no context menu if we are not in a workspace
            return;
        };

        let is_workspace = path == workspace_path;

        let base_path_a = if is_dir {
            Some(path_a.clone())
        } else {
            path_a.parent().map(ToOwned::to_owned)
        };
        let base_path_a = base_path_a.as_ref().unwrap_or(workspace_path);

        let mut menu = Menu::new("");
        menu=menu.entry(self.creation_menu(base_path_a.clone()));

        if is_workspace {menu=menu.entry(crate::git_actions::menu(common.internal_command,None));}
        if is_workspace && crate::dart::is_flutter_project(workspace_path) {for(title,args)in[("Build Project",vec!["build"]),("Clean Project",vec!["clean"]),("Run All Tests",vec!["test"]),("Run All Tests with Coverage",vec!["test","--coverage"])]{let commands=common.sparrow_command;menu=menu.entry(MenuItem::new(title).action(move ||commands.send(SparrowCommand{kind:CommandKind::Workbench(crate::command::SparrowWorkbenchCommand::FlutterProjectTask),data:Some(serde_json::json!(args))})));}}


        let source=if crate::dart::is_dart_path(&path_a){self.naming_editor_data.doc().editors.with_editors_untracked(|editors|editors.values().find_map(|editor|{
            let doc=editor.doc();doc.content.with_untracked(|c|c.path().filter(|p|*p==&path_a).map(|_|doc.buffer.with_untracked(|b|b.to_string())))
        })).or_else(|| std::fs::read_to_string(&path_a).ok())}else{None};
        if !is_dir && crate::dart::is_dart_path(&path_a) && source.as_deref().is_some_and(crate::dart::regex::has_entry_point) {
            for (label, command) in [
                (
                    "Run",
                    crate::command::SparrowWorkbenchCommand::RunCurrentFile,
                ),
                (
                    "Debug",
                    crate::command::SparrowWorkbenchCommand::DebugCurrentFile,
                ),
                ("Profile",crate::command::SparrowWorkbenchCommand::ProfileCurrentFile),
            ] {
                if label=="Profile" && (crate::dart::test_runner::is_test_file(&path_a)||!common.workspace.path.as_deref().is_some_and(crate::dart::is_flutter_project)){continue;}
                let path = path_a.clone();
                let commands = common.sparrow_command;
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                menu = menu.entry(
                    MenuItem::new(format!("{label} '{name}'")).action(move || {
                        commands.send(crate::command::SparrowCommand {
                            kind: crate::command::CommandKind::Workbench(
                                command.clone(),
                            ),
                            data: Some(
                                serde_json::to_value(&path)
                                    .expect("path serializes"),
                            ),
                        });
                    }),
                );
            }
        }

        if is_dir {for (title,command) in [("Find in Files…",crate::command::SparrowWorkbenchCommand::FindInFiles),("Replace in Files…",crate::command::SparrowWorkbenchCommand::ReplaceInFiles)] {let path=path_a.clone();let commands=common.sparrow_command;menu=menu.entry(MenuItem::new(title).action(move ||commands.send(SparrowCommand{kind:CommandKind::Workbench(command.clone()),data:Some(serde_json::json!(path))})));}}
        let target=path_a.clone();let commands=common.sparrow_command;
        menu=menu.entry(MenuItem::new("Reformat Code…").action(move ||commands.send(SparrowCommand{kind:CommandKind::Workbench(crate::command::SparrowWorkbenchCommand::ReformatCodeOptions),data:Some(serde_json::json!(target))})));
        menu = menu.separator();

        for (label, cut) in [("Cut", true), ("Copy", false)] {
            let data = self.clone();
            let path = path_a.clone();
            menu = menu.entry(
                MenuItem::new(label)
                    .enabled(!is_workspace)
                    .action(move || data.copy_file(path.clone(), cut)),
            );
        }
        let data = self.clone();
        let target = path_a.clone();
        menu = menu
            .entry(
                MenuItem::new("Paste...")
                    .enabled(self.clipboard_file().is_some())
                    .action(move || data.paste_file(target.clone())),
            )
            .separator();

        if !common.workspace.kind.is_remote() {
            let file=path_a.clone();let folder=if is_dir{file.clone()}else{file.parent().unwrap_or(&file).to_path_buf()};let files=folder.clone();let terminal=folder.clone();let commands=common.sparrow_command;let internal=common.internal_command;
            let mut ancestry=Menu::new("Path Popup");for ancestor in file.ancestors(){let path=ancestor.to_path_buf();let data=self.clone();let title=path.display().to_string();ancestry=ancestry.entry(MenuItem::new(title).action(move ||data.reveal_in_file_tree(path.clone())));}
            menu=menu.entry(Menu::new("Open In")
                .entry(MenuItem::new("Files").action(move ||{let _=open::that(&files);}))
                .entry(MenuItem::new("Open in Associated Application").action(move ||{let _=open::that(&file);}))
                .entry(ancestry)
                .entry(MenuItem::new("Terminal").action(move ||{
                    internal.send(crate::command::InternalCommand::NewTerminal{profile:Some(sparrow_rpc::terminal::TerminalProfile{name:"Terminal".into(),workdir:lsp_types::Url::from_directory_path(&terminal).ok(),..Default::default()})});
                    commands.send(crate::command::SparrowCommand{kind:crate::command::CommandKind::Workbench(crate::command::SparrowWorkbenchCommand::ShowPanel),data:Some(serde_json::json!(crate::panel::kind::PanelKind::Terminal))});
                })));
        }

        if !is_dir && crate::dart::is_dart_path(&path_a) {
            let mut refactor=Menu::new("Refactor");
            let path=path_a.clone();let commands=common.internal_command;
            refactor=refactor.entry(MenuItem::new("Available Refactorings…").action(move ||commands.send(InternalCommand::QuickFixAt{location:crate::editor::location::EditorLocation{path:path.clone(),position:None,scroll_offset:None,ignore_unconfirmed:false,same_editor_tab:false}})));

            for(title,command)in[("Rename…",crate::command::SparrowWorkbenchCommand::RenameActiveFile),("Move File…",crate::command::SparrowWorkbenchCommand::MoveActiveFile),("Safe Delete…",crate::command::SparrowWorkbenchCommand::SafeDeleteFile)] {let path=path_a.clone();let commands=common.sparrow_command;refactor=refactor.entry(MenuItem::new(title).action(move ||commands.send(SparrowCommand{kind:CommandKind::Workbench(command.clone()),data:Some(serde_json::json!(path))})));}
            let path=path_a.clone();let data=self.clone();refactor=refactor.entry(MenuItem::new("Copy File…").action(move ||data.begin_naming(Naming::Duplicating(Duplicating{state:NamingState::Naming,path:path.clone(),destination:None,is_dir:false,move_on_finish:false,editor_needs_reset:false}))));menu=menu.entry(refactor);
        }
        if !is_workspace {
            let path = path_a.clone();
            let rename = self.clone();
            menu = menu.entry(MenuItem::new("Rename…").action(move || {
                rename.request_rename(path.clone());
            }));

            let path = path_a.clone();
            let duplicate = self.clone();
            menu = menu.entry(MenuItem::new("Duplicate…").action(move || {
                duplicate.begin_naming(Naming::Duplicating(Duplicating {
                    state: NamingState::Naming,
                    path: path.clone(),
                    destination: None,
                    is_dir,
                    move_on_finish: false,
                    editor_needs_reset: false,
                }));
            }));

            // TODO: it is common for shift+right click to make 'Move file to trash' an actual
            // Delete, which can be useful for large files.
            let path = path_a.clone();
            let proxy = common.proxy.clone();

            menu = menu.entry(MenuItem::new("Delete").action(move || {
                proxy.trash_path(path.clone(), |res| {
                    if let Err(err) = res {
                        tracing::warn!("Failed to trash path: {:?}", err);
                    }
                })
            }));
        }

        menu = menu.separator();

        let copy_path = path_a.clone();
        let copy_workspace = common.workspace.path.clone();
        menu =
            menu.entry(MenuItem::new("Copy Path/Reference...").action(move || {
                let path = copy_path.clone();
                let root = copy_workspace.clone();
                floem::action::exec_after(std::time::Duration::ZERO, move |_| {
                    show_context_menu(
                        crate::menus::copy_path_menu(path, root),
                        None,
                    );
                });
            }));

        menu = menu.separator();

        let path = path_a.clone();
        menu = menu.entry(
            MenuItem::new("Select for Compare")
                .enabled(!is_dir)
                .action(move || left_diff_path.set(Some(path.clone()))),
        );

        if let Some(left_path) = self.left_diff_path.get_untracked() {
            let common = self.common.clone();
            let right_path = path_a.to_owned();

            let comparable = !is_dir && !self.is_dir(&left_path);
            menu = menu.entry(
                MenuItem::new("Compare with Selected")
                    .enabled(comparable)
                    .action(move || {
                        common.internal_command.send(
                            InternalCommand::OpenDiffFiles {
                                left_path: left_path.clone(),
                                right_path: right_path.clone(),
                            },
                        )
                    }),
            )
        }

        menu = menu.separator();

        let internal_command = common.internal_command;
        menu = menu.entry(MenuItem::new("Refresh").action(move || {
            internal_command.send(InternalCommand::ReloadFileExplorer);
        }));

        show_context_menu(menu, None);
    }

    pub fn middle_click(&self, path: &Path) -> EventPropagation {
        if self.is_dir(path) {
            EventPropagation::Continue
        } else {
            self.common
                .internal_command
                .send(InternalCommand::OpenFileInNewTab {
                    path: path.to_path_buf(),
                });
            EventPropagation::Stop
        }
    }
}

fn visible_project_nodes(root: &FileNodeItem) -> Vec<(PathBuf, bool, bool)> {
    fn visit(node: &FileNodeItem, out: &mut Vec<(PathBuf, bool, bool)>) {
        out.push((node.path.clone(), node.is_dir, node.open));
        if node.is_dir && node.open {
            for child in node.sorted_children() {
                visit(child, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(root, &mut out);
    out
}
#[cfg(test)]
mod project_navigation_tests {
    use super::*;
    #[test]
    fn project_popup_accepts_nested_names_and_rejects_parent_paths() {
        assert_eq!(relative_name("models/café.dart"), Some(Path::new("models/café.dart")));
        for invalid in ["", "   ", ".", "lib/..", "../outside", "/absolute", "two\nlines"] {
            assert_eq!(relative_name(invalid), None, "{invalid:?}");
        }
    }

    #[test]
    fn expansion_and_collapse_recount_loaded_descendants() {
        let mut root = FileNodeItem { path: PathBuf::from("/p"), is_dir: true,
            read: true, open: true, children: HashMap::new(), children_open_count: 0 };
        let mut folder = root.clone(); folder.path = PathBuf::from("/p/lib");
        let mut file = folder.clone(); file.path = PathBuf::from("/p/lib/a.dart"); file.is_dir = false;
        folder.children.insert(file.path.clone(), file);
        root.children.insert(folder.path.clone(), folder);
        set_loaded_node_open(&mut root, true);
        assert_eq!(root.children_open_count, 2);
        set_loaded_node_open(&mut root, false);
        root.open = true;
        root.update_node_count(&root.path.clone());
        assert_eq!(root.children_open_count, 1);
        assert!(!root.children.values().next().unwrap().open);
    }
    #[test]
    fn visible_tree_uses_explorer_order_and_skips_collapsed_descendants() {
        let node = |path: &str, is_dir: bool, open: bool| FileNodeItem {
            path: PathBuf::from(path),
            is_dir,
            read: true,
            open,
            children: HashMap::new(),
            children_open_count: 0,
        };
        let mut root = node("/project", true, true);
        let mut lib = node("/project/lib", true, false);
        lib.children.insert(
            PathBuf::from("/project/lib/main.dart"),
            node("/project/lib/main.dart", false, false),
        );
        root.children.insert(lib.path.clone(), lib);
        root.children.insert(
            PathBuf::from("/project/README.md"),
            node("/project/README.md", false, false),
        );
        assert_eq!(
            visible_project_nodes(&root)
                .iter()
                .map(|node| node.0.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("/project"),
                PathBuf::from("/project/lib"),
                PathBuf::from("/project/README.md")
            ]
        );
        root.children
            .get_mut(&PathBuf::from("/project/lib"))
            .unwrap()
            .open = true;
        assert_eq!(
            visible_project_nodes(&root)[2].0,
            PathBuf::from("/project/lib/main.dart")
        );
    }
}
