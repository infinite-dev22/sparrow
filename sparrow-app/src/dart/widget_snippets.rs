use lsp_types::{CompletionItem, CompletionItemKind, InsertTextFormat};

const STATELESS: &str = "class ${1:MyWidget} extends StatelessWidget {
  const ${1:MyWidget}({super.key});

  @override
  Widget build(BuildContext context) {
    return ${0:const Placeholder()};
  }
}";

const STATEFUL: &str = "class ${1:MyWidget} extends StatefulWidget {
  const ${1:MyWidget}({super.key});

  @override
  State<${1:MyWidget}> createState() => _${1:MyWidget}State();
}

class _${1:MyWidget}State extends State<${1:MyWidget}> {
  @override
  Widget build(BuildContext context) {
    return ${0:const Placeholder()};
  }
}";

pub(crate) fn items(input: &str, indentation: &str) -> Vec<CompletionItem> {
    if !input.starts_with("st") {
        return Vec::new();
    }
    [
        ("stless", "Flutter stateless widget", STATELESS),
        ("stful", "Flutter stateful widget", STATEFUL),
    ]
    .into_iter()
    .filter(|(trigger, _, _)| trigger.starts_with(input))
    .map(|(trigger, description, snippet)| CompletionItem {
        label: trigger.into(),
        detail: Some(description.into()),
        kind: Some(CompletionItemKind::SNIPPET),
        sort_text: Some(format!("!{trigger}")),
        insert_text: Some(snippet.replace('\n', &format!("\n{indentation}"))),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        ..CompletionItem::default()
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snippet::Snippet;

    #[test]
    fn widget_snippets_expand_with_linked_names_and_a_body_placeholder() {
        for (trigger, names) in [("stful", 6), ("stless", 2)] {
            let items = items(trigger, "    ");
            assert_eq!(items.len(), 1);
            let snippet: Snippet =
                items[0].insert_text.as_ref().unwrap().parse().unwrap();
            assert_eq!(
                snippet
                    .tabs(0)
                    .iter()
                    .filter(|(index, _)| *index == 1)
                    .count(),
                names
            );
            assert_eq!(snippet.tabs(0).last().unwrap().0, 0);
            assert!(snippet.text().contains("\n      @override"));
        }
        assert!(items("start", "").is_empty());
    }
}
