use floem::text::{
    Attrs, AttrsList, FamilyOwned, LineHeightValue, Style, TextLayout, Weight,
};
use lapce_xi_rope::Rope;
use lsp_types::MarkedString;
use pulldown_cmark::{CodeBlockKind, CowStr, Event, Options, Parser, Tag};
use smallvec::SmallVec;
use sparrow_core::{language::SparrowLanguage, syntax::Syntax};

use crate::config::{SparrowConfig, color::SparrowColor};

#[derive(Clone)]
pub enum MarkdownContent {
    Text(TextLayout),
    LinkedText {
        layout: TextLayout,
        links: Vec<(std::ops::Range<usize>, String)>,
    },
    Image {
        url: String,
        title: String,
    },
    Separator,
}

pub fn parse_markdown(
    text: &str,
    line_height: f64,
    config: &SparrowConfig,
) -> Vec<MarkdownContent> {
    parse_markdown_inner(text, line_height, config, false)
}
pub fn parse_document(text: &str, config: &SparrowConfig) -> Vec<MarkdownContent> {
    parse_markdown_inner(text, 1.5, config, true)
}
fn parse_markdown_inner(
    text: &str,
    line_height: f64,
    config: &SparrowConfig,
    document: bool,
) -> Vec<MarkdownContent> {
    let mut links = Vec::new();
    let mut res = Vec::new();

    let mut current_text = String::new();
    let code_font_family: Vec<FamilyOwned> =
        FamilyOwned::parse_list(&config.editor.font_family).collect();

    let ui_font_family = config.ui.font_family();
    let default_attrs = Attrs::new()
        .family(&ui_font_family)
        .color(config.color(SparrowColor::EDITOR_FOREGROUND))
        .font_size(config.ui.font_size() as f32)
        .line_height(LineHeightValue::Normal(line_height as f32));
    let mut attr_list = AttrsList::new(default_attrs.clone());

    let mut builder_dirty = false;

    let mut pos = 0;

    let mut tag_stack: SmallVec<[(usize, Tag); 4]> = SmallVec::new();

    let parser = Parser::new_ext(
        text,
        Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_HEADING_ATTRIBUTES,
    );
    let mut last_text = CowStr::from("");
    // Whether we should add a newline on the next entry
    // This is used so that we don't emit newlines at the very end of the generation
    let mut add_newline = false;
    for event in parser {
        // Add the newline since we're going to be outputting more
        if add_newline {
            current_text.push('\n');
            builder_dirty = true;
            pos += 1;
            add_newline = false;
        }

        match event {
            Event::Start(tag) => {
                if document && matches!(tag, Tag::Item) {
                    current_text.push_str("• ");
                    pos += 4;
                    builder_dirty = true;
                }
                tag_stack.push((pos, tag));
            }
            Event::End(end_tag) => {
                if let Some((start_offset, tag)) = tag_stack.pop() {
                    if end_tag != tag.to_end() {
                        tracing::warn!("Mismatched markdown tag");
                        continue;
                    }

                    if let Some(mut attrs) = attribute_for_tag(
                        default_attrs.clone(),
                        &tag,
                        &code_font_family,
                        config,
                    ) {
                        if document {
                            if let Tag::Heading { level, .. } = &tag {
                                attrs = attrs.font_size(
                                    config.ui.font_size() as f32
                                        * match level {
                                            pulldown_cmark::HeadingLevel::H1 => 2.3,
                                            pulldown_cmark::HeadingLevel::H2 => 1.85,
                                            _ => 1.5,
                                        },
                                );
                            }
                        }
                        attr_list
                            .add_span(start_offset..pos.max(start_offset), attrs);
                    }

                    if should_add_newline_after_tag(&tag) {
                        if document {
                            match &tag {
                                Tag::Paragraph
                                | Tag::Heading { .. }
                                | Tag::CodeBlock(_) => {
                                    current_text.push_str("\n\n");
                                    pos += 2;
                                }
                                Tag::Item | Tag::TableRow => {
                                    if !current_text.ends_with('\n') {
                                        current_text.push('\n');
                                        pos += 1;
                                    }
                                }
                                Tag::TableCell => {
                                    current_text.push_str("  │  ");
                                    pos += 7;
                                }
                                _ => {}
                            }
                        } else {
                            add_newline = true;
                        }
                    }

                    match &tag {
                        Tag::Link { dest_url, .. } => {
                            links.push((start_offset..pos, dest_url.to_string()))
                        }
                        Tag::Heading { .. } if document => {
                            let slug = current_text[start_offset..pos]
                                .trim()
                                .to_lowercase()
                                .chars()
                                .filter_map(|c| {
                                    if c.is_alphanumeric() || c == '-' || c == '_' {
                                        Some(c)
                                    } else if c.is_whitespace() {
                                        Some('-')
                                    } else {
                                        None
                                    }
                                })
                                .collect::<String>();
                            links.push((
                                start_offset..start_offset,
                                format!("#{slug}"),
                            ));
                        }
                        Tag::CodeBlock(kind) => {
                            let language =
                                if let CodeBlockKind::Fenced(language) = kind {
                                    md_language_to_sparrow_language(language)
                                } else {
                                    None
                                };

                            highlight_as_code(
                                &mut attr_list,
                                default_attrs.clone().family(&code_font_family),
                                language,
                                &last_text,
                                start_offset,
                                config,
                            );
                            builder_dirty = true;
                        }
                        Tag::Image {
                            link_type: _,
                            dest_url: dest,
                            title,
                            id: _,
                        } => {
                            // TODO: Are there any link types that would change how the
                            // image is rendered?

                            if builder_dirty {
                                let mut text_layout = TextLayout::new();
                                text_layout.set_text(&current_text, attr_list, None);
                                res.push(if document {
                                    MarkdownContent::LinkedText {
                                        layout: text_layout,
                                        links: std::mem::take(&mut links),
                                    }
                                } else {
                                    MarkdownContent::Text(text_layout)
                                });
                                attr_list = AttrsList::new(default_attrs.clone());
                                current_text.clear();
                                pos = 0;
                                builder_dirty = false;
                            }

                            res.push(MarkdownContent::Image {
                                url: dest.to_string(),
                                title: title.to_string(),
                            });
                        }
                        _ => {
                            // Presumably?
                            builder_dirty = true;
                        }
                    }
                } else {
                    tracing::warn!("Unbalanced markdown tag")
                }
            }
            Event::Text(text) => {
                if let Some((_, tag)) = tag_stack.last() {
                    if should_skip_text_in_tag(tag) {
                        continue;
                    }
                }
                current_text.push_str(&text);
                pos += text.len();
                last_text = text;
                builder_dirty = true;
            }
            Event::Code(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs.clone().family(&code_font_family),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            // TODO: Some minimal 'parsing' of html could be useful here, since some things use
            // basic html like `<code>text</code>`.
            Event::Html(text) => {
                attr_list.add_span(
                    pos..pos + text.len(),
                    default_attrs
                        .clone()
                        .family(&code_font_family)
                        .color(config.color(SparrowColor::MARKDOWN_BLOCKQUOTE)),
                );
                current_text.push_str(&text);
                pos += text.len();
                builder_dirty = true;
            }
            Event::HardBreak => {
                current_text.push('\n');
                pos += 1;
                builder_dirty = true;
            }
            Event::SoftBreak => {
                current_text.push(' ');
                pos += 1;
                builder_dirty = true;
            }
            Event::Rule => {
                if document {
                    current_text.push_str("────────────────────\n");
                    pos += "────────────────────\n".len();
                    builder_dirty = true;
                }
            }
            Event::FootnoteReference(_text) => {}
            Event::TaskListMarker(checked) => {
                if document {
                    let marker = if checked { "☑ " } else { "☐ " };
                    current_text.push_str(marker);
                    pos += marker.len();
                    builder_dirty = true;
                }
            }
            Event::InlineHtml(_) => {} // TODO(panekj): Implement
            Event::InlineMath(_) => {} // TODO(panekj): Implement
            Event::DisplayMath(_) => {} // TODO(panekj): Implement
        }
    }

    if builder_dirty {
        let mut text_layout = TextLayout::new();
        text_layout.set_text(&current_text, attr_list, None);
        res.push(if document {
            MarkdownContent::LinkedText {
                layout: text_layout,
                links: std::mem::take(&mut links),
            }
        } else {
            MarkdownContent::Text(text_layout)
        });
    }

    res
}

fn attribute_for_tag<'a>(
    default_attrs: Attrs<'a>,
    tag: &Tag,
    code_font_family: &'a [FamilyOwned],
    config: &SparrowConfig,
) -> Option<Attrs<'a>> {
    use pulldown_cmark::HeadingLevel;
    match tag {
        Tag::Heading {
            level,
            id: _,
            classes: _,
            attrs: _,
        } => {
            // The size calculations are based on the em values given at
            // https://drafts.csswg.org/css2/#html-stylesheet
            let font_scale = match level {
                HeadingLevel::H1 => 2.0,
                HeadingLevel::H2 => 1.5,
                HeadingLevel::H3 => 1.17,
                HeadingLevel::H4 => 1.0,
                HeadingLevel::H5 => 0.83,
                HeadingLevel::H6 => 0.75,
            };
            let font_size = font_scale * config.ui.font_size() as f64;
            Some(
                default_attrs
                    .font_size(font_size as f32)
                    .weight(Weight::SEMIBOLD),
            )
        }
        Tag::BlockQuote(_block_quote) => Some(
            default_attrs
                .style(Style::Italic)
                .color(config.color(SparrowColor::MARKDOWN_BLOCKQUOTE)),
        ),
        Tag::CodeBlock(_) => Some(default_attrs.family(code_font_family)),
        Tag::Emphasis => Some(default_attrs.style(Style::Italic)),
        Tag::Strong => Some(default_attrs.weight(Weight::SEMIBOLD)),
        // TODO: Strikethrough support
        Tag::Link {
            link_type: _,
            dest_url: _,
            title: _,
            id: _,
        } => {
            // TODO: Link support
            Some(default_attrs.color(config.color(SparrowColor::EDITOR_LINK)))
        }
        // All other tags are currently ignored
        _ => None,
    }
}

/// Decides whether newlines should be added after a specific markdown tag
fn should_add_newline_after_tag(tag: &Tag) -> bool {
    !matches!(
        tag,
        Tag::Emphasis | Tag::Strong | Tag::Strikethrough | Tag::Link { .. }
    )
}

/// Whether it should skip the text node after a specific tag  
/// For example, images are skipped because it emits their title as a separate text node.  
fn should_skip_text_in_tag(tag: &Tag) -> bool {
    matches!(tag, Tag::Image { .. })
}

fn md_language_to_sparrow_language(lang: &str) -> Option<SparrowLanguage> {
    // TODO: There are many other names commonly used that should be supported
    SparrowLanguage::from_name(lang)
}

/// Highlight the text in a richtext builder like it was a markdown codeblock
pub fn highlight_as_code(
    attr_list: &mut AttrsList,
    default_attrs: Attrs,
    language: Option<SparrowLanguage>,
    text: &str,
    start_offset: usize,
    config: &SparrowConfig,
) {
    let syntax = language.map(Syntax::from_language);

    let styles = syntax
        .map(|mut syntax| {
            syntax.parse(0, Rope::from(text), None);
            syntax.styles
        })
        .unwrap_or(None);

    if let Some(styles) = styles {
        for (range, style) in styles.iter() {
            if let Some(color) = style
                .fg_color
                .as_ref()
                .and_then(|fg| config.style_color(fg))
            {
                attr_list.add_span(
                    start_offset + range.start..start_offset + range.end,
                    default_attrs.clone().color(color),
                );
            }
        }
    }
}

pub fn from_marked_string(
    text: MarkedString,
    config: &SparrowConfig,
) -> Vec<MarkdownContent> {
    match text {
        MarkedString::String(text) => parse_markdown(&text, 1.8, config),
        // This is a short version of a code block
        MarkedString::LanguageString(code) => {
            // TODO: We could simply construct the MarkdownText directly
            // Simply construct the string as if it was written directly
            parse_markdown(
                &format!("```{}\n{}\n```", code.language, code.value),
                1.8,
                config,
            )
        }
    }
}

pub fn from_plaintext(
    text: &str,
    line_height: f64,
    config: &SparrowConfig,
) -> Vec<MarkdownContent> {
    let ui_font_family = config.ui.font_family();
    let mut text_layout = TextLayout::new();
    text_layout.set_text(
        text,
        AttrsList::new(
            Attrs::new()
                .family(&ui_font_family)
                .color(config.color(SparrowColor::EDITOR_FOREGROUND))
                .font_size(config.ui.font_size() as f32)
                .line_height(LineHeightValue::Normal(line_height as f32)),
        ),
        None,
    );
    vec![MarkdownContent::Text(text_layout)]
}

#[cfg(test)]
mod document_preview_tests {
    use super::*;
    #[test]
    fn rendered_links_keep_byte_offsets_after_unicode_bullets_and_emphasis() {
        let config = SparrowConfig::load(
            &crate::workspace::SparrowWorkspace::default(),
            &[],
            &[],
        );
        let blocks = parse_document(
            "# Heading\n\n- [héllo](lib/main.dart)\n\n**Live** text",
            &config,
        );
        let MarkdownContent::LinkedText { layout, links } = blocks.first().unwrap()
        else {
            panic!("No rendered document")
        };
        let text = layout
            .lines()
            .iter()
            .map(|line| line.text())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("• héllo"));
        let (range, target) = links
            .iter()
            .find(|(_, target)| target == "lib/main.dart")
            .unwrap();
        assert_eq!(&text[range.clone()], "héllo");
        assert_eq!(target, "lib/main.dart");
    }
}
