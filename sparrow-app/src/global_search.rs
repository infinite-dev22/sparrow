use std::{ops::Range, path::PathBuf, rc::Rc};

use floem::{
    ext_event::create_ext_action,
    keyboard::Modifiers,
    reactive::{Memo, RwSignal, Scope, SignalGet, SignalUpdate, SignalWith},
    views::VirtualVector,
};
use indexmap::IndexMap;
use lapce_xi_rope::Rope;
use sparrow_core::{mode::Mode, selection::Selection};
use sparrow_rpc::proxy::{ProxyResponse, SearchMatch};

use crate::{
    command::{CommandExecuted, CommandKind},
    editor::EditorData,
    keypress::{KeyPressFocus, condition::Condition},
    main_split::MainSplitData,
    window_tab::CommonData,
};

#[derive(Clone)]
pub struct SearchMatchData {
    pub expanded: RwSignal<bool>,
    pub matches: RwSignal<im::Vector<SearchMatch>>,
    pub line_height: Memo<f64>,
}

impl SearchMatchData {
    pub fn height(&self) -> f64 {
        let line_height = self.line_height.get();
        let count = if self.expanded.get() {
            self.matches.with(|m| m.len()) + 1
        } else {
            1
        };
        line_height * count as f64
    }
}

#[derive(Clone, Debug)]
pub struct GlobalSearchData {
    pub editor: EditorData,
    pub search_result: RwSignal<IndexMap<PathBuf, SearchMatchData>>,
    pub main_split: MainSplitData,
    pub common: Rc<CommonData>,
    published_query: RwSignal<Option<(String, bool, bool, bool)>>,
    request_id: RwSignal<u64>,
}

impl KeyPressFocus for GlobalSearchData {
    fn get_mode(&self) -> Mode {
        Mode::Insert
    }

    fn check_condition(&self, condition: Condition) -> bool {
        matches!(condition, Condition::PanelFocus)
    }

    fn run_command(
        &self,
        command: &crate::command::SparrowCommand,
        count: Option<usize>,
        mods: Modifiers,
    ) -> CommandExecuted {
        match &command.kind {
            CommandKind::Workbench(_) => {}
            CommandKind::Scroll(_) => {}
            CommandKind::Focus(_) => {}
            CommandKind::Edit(_)
            | CommandKind::Move(_)
            | CommandKind::MultiSelection(_) => {
                return self.editor.run_command(command, count, mods);
            }
            CommandKind::MotionMode(_) => {}
        }
        CommandExecuted::No
    }

    fn receive_char(&self, c: &str) {
        self.editor.receive_char(c);
    }
}

impl VirtualVector<(PathBuf, SearchMatchData)> for GlobalSearchData {
    fn total_len(&self) -> usize {
        self.search_result.with(|result| {
            result
                .iter()
                .map(|(_, data)| {
                    if data.expanded.get() {
                        data.matches.with(|m| m.len()) + 1
                    } else {
                        1
                    }
                })
                .sum()
        })
    }

    fn slice(
        &mut self,
        _range: Range<usize>,
    ) -> impl Iterator<Item = (PathBuf, SearchMatchData)> {
        self.search_result.get().into_iter()
    }
}

impl GlobalSearchData {
    pub fn new(cx: Scope, main_split: MainSplitData) -> Self {
        let common = main_split.common.clone();
        let editor = main_split.editors.make_local(cx, common.clone());
        let search_result = cx.create_rw_signal(IndexMap::new());

        let global_search = Self {
            editor,
            search_result,
            main_split,
            common,
            published_query: cx.create_rw_signal(None),
            request_id: cx.create_rw_signal(0),
        };

        {
            let global_search = global_search.clone();
            let buffer = global_search.editor.doc().buffer;
            cx.create_effect(move |_| {
                let pattern = buffer.with(|buffer| buffer.to_string());
                let case_sensitive = global_search.common.find.case_sensitive(true);
                let whole_word = global_search.common.find.whole_words.get();
                let is_regex = global_search.common.find.is_regex.get();
                global_search.request_id.update(|id| *id += 1);
                let request = global_search.request_id.get_untracked();
                if global_search.published_query.with_untracked(|published| {
                    published
                        .as_ref()
                        .is_some_and(|(query, case, words, regex)| {
                            query == &pattern
                                && *case == case_sensitive
                                && *words == whole_word
                                && *regex == is_regex
                        })
                }) {
                    return;
                }
                global_search.published_query.set(None);
                if pattern.is_empty() {
                    global_search.search_result.update(|r| r.clear());
                    return;
                }
                let send = {
                    let global_search = global_search.clone();
                    create_ext_action(cx, move |result| {
                        if global_search.request_id.try_get_untracked()
                            == Some(request)
                            && let Ok(ProxyResponse::GlobalSearchResponse {
                                matches,
                            }) = result
                        {
                            global_search.update_matches(matches);
                        }
                    })
                };
                global_search.common.proxy.global_search(
                    pattern,
                    case_sensitive,
                    whole_word,
                    is_regex,
                    move |result| {
                        send(result);
                    },
                );
            });
        }

        {
            let buffer = global_search.editor.doc().buffer;
            let main_split = global_search.main_split.clone();
            cx.create_effect(move |_| {
                let content = buffer.with(|buffer| buffer.to_string());
                main_split.set_find_pattern(Some(content));
            });
        }

        global_search
    }

    fn update_matches(&self, matches: IndexMap<PathBuf, Vec<SearchMatch>>) {
        let current = self.search_result.get_untracked();

        self.search_result.set(
            matches
                .into_iter()
                .map(|(path, matches)| {
                    let match_data =
                        current.get(&path).cloned().unwrap_or_else(|| {
                            SearchMatchData {
                                expanded: self.common.scope.create_rw_signal(true),
                                matches: self
                                    .common
                                    .scope
                                    .create_rw_signal(im::Vector::new()),
                                line_height: self.common.ui_line_height,
                            }
                        });

                    match_data.matches.set(matches.into());

                    (path, match_data)
                })
                .collect(),
        );
    }

    pub fn set_pattern(&self, pattern: String) {
        self.published_query.set(None);
        self.write_pattern(pattern);
    }

    fn write_pattern(&self, pattern: String) {
        let pattern_len = pattern.len();
        self.editor.doc().reload(Rope::from(pattern), true);
        self.editor
            .cursor()
            .update(|cursor| cursor.set_insert(Selection::region(0, pattern_len)));
    }

    pub(crate) fn publish_results(
        &self,
        pattern: String,
        hits: &[crate::project_search::Hit],
        case_sensitive: bool,
        whole_words: bool,
        regex: bool,
    ) {
        let mut matches = IndexMap::<PathBuf, Vec<SearchMatch>>::new();
        for hit in hits {
            matches
                .entry(hit.path.clone())
                .or_default()
                .push(SearchMatch {
                    line: hit.line,
                    start: hit.span.start,
                    end: hit.span.end,
                    line_content: hit.text.to_string(),
                });
        }
        floem::reactive::batch(|| {
            self.request_id.update(|id| *id += 1);
            self.published_query.set(Some((
                pattern.clone(),
                case_sensitive,
                whole_words,
                regex,
            )));
            self.common.find.set_case_sensitive(case_sensitive);
            self.common.find.whole_words.set(whole_words);
            self.common.find.is_regex.set(regex);
            self.write_pattern(pattern);
            self.update_matches(matches);
        });
    }
}
