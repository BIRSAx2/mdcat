// Copyright Mouhieddine Sabir <me@mouhieddine.dev>

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

//! Build a table of contents from a Markdown document's headings.

use std::collections::HashMap;

use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};

struct Heading {
    level: u8,
    title: String,
}

fn collect_headings(markdown: &str, options: Options) -> Vec<Heading> {
    let mut headings = Vec::new();
    let mut current: Option<Heading> = None;
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                current = Some(Heading {
                    level: level as u8,
                    title: String::new(),
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(heading) = current.take() {
                    headings.push(heading);
                }
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(heading) = current.as_mut() {
                    heading.title.push_str(&text);
                }
            }
            _ => {}
        }
    }
    headings
}

/// Turn a heading title into a GitHub-style anchor slug.
///
/// This only approximates GitHub's actual slugger (it drops all punctuation
/// rather than special-casing a few categories), but it's close enough for
/// tools that resolve `#fragment`s by slugifying headings the same way.
fn slugify(title: &str, used: &mut HashMap<String, usize>) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for c in title.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
            last_was_dash = false;
        } else if matches!(c, ' ' | '-' | '_') && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    let slug = if slug.is_empty() { "section" } else { slug };

    let count = used.entry(slug.to_string()).or_insert(0);
    let unique_slug = if *count == 0 {
        slug.to_string()
    } else {
        format!("{slug}-{count}")
    };
    *count += 1;
    unique_slug
}

struct Node {
    title: String,
    slug: String,
    children: Vec<Node>,
}

/// Build a tree of `Node`s from a flat, depth-first list of headings.
///
/// `min_level` is the shallowest level still accepted as a child of the
/// caller; recursion stops as soon as a heading shallower than that is seen,
/// leaving it for the caller to pick up as a sibling.
fn build_tree(entries: &[(u8, String, String)], idx: &mut usize, min_level: u8) -> Vec<Node> {
    let mut nodes = Vec::new();
    while let Some((level, title, slug)) = entries.get(*idx) {
        if *level < min_level {
            break;
        }
        *idx += 1;
        let children = build_tree(entries, idx, level.saturating_add(1));
        nodes.push(Node {
            title: title.clone(),
            slug: slug.clone(),
            children,
        });
    }
    nodes
}

fn push_node(events: &mut Vec<Event<'static>>, node: &Node, file_ref: Option<&str>) {
    events.push(Event::Start(Tag::Item));
    match file_ref {
        Some(file_ref) => {
            events.push(Event::Start(Tag::Link {
                link_type: LinkType::Inline,
                dest_url: CowStr::from(format!("{file_ref}#{}", node.slug)),
                title: CowStr::Borrowed(""),
                id: CowStr::Borrowed(""),
            }));
            events.push(Event::Text(CowStr::from(node.title.clone())));
            events.push(Event::End(TagEnd::Link));
        }
        None => events.push(Event::Text(CowStr::from(node.title.clone()))),
    }
    if !node.children.is_empty() {
        events.push(Event::Start(Tag::List(Some(1))));
        for child in &node.children {
            push_node(events, child, file_ref);
        }
        events.push(Event::End(TagEnd::List(true)));
    }
    events.push(Event::End(TagEnd::Item));
}

/// Build the events for a table of contents, generated from the headings in `markdown`.
///
/// `options` must match the options used to parse `markdown` for the real render, so that
/// headings are extracted consistently (e.g. with the same Markdown extensions enabled).
///
/// If `file_ref` is given, wrap each entry in a link to `{file_ref}#{slug}`, letting terminals
/// with OSC 8 support open the source file at that heading in another application that resolves
/// GitHub-style anchors. Pass `None` (e.g. when reading from standard input, where there is no
/// file to link to) to render entries as plain text instead.
///
/// Returns an empty `Vec` if `markdown` has no headings.
pub fn build_toc_events(
    markdown: &str,
    options: Options,
    file_ref: Option<&str>,
) -> Vec<Event<'static>> {
    let headings = collect_headings(markdown, options);
    if headings.is_empty() {
        return Vec::new();
    }

    let mut used = HashMap::new();
    let entries: Vec<(u8, String, String)> = headings
        .into_iter()
        .map(|heading| {
            let slug = slugify(&heading.title, &mut used);
            (heading.level, heading.title, slug)
        })
        .collect();

    let mut idx = 0;
    let roots = build_tree(&entries, &mut idx, 0);

    let mut events = vec![
        Event::Start(Tag::Paragraph),
        Event::Start(Tag::Strong),
        Event::Text(CowStr::Borrowed("Table of Contents")),
        Event::End(TagEnd::Strong),
        Event::End(TagEnd::Paragraph),
        Event::Start(Tag::List(Some(1))),
    ];
    for root in &roots {
        push_node(&mut events, root, file_ref);
    }
    events.push(Event::End(TagEnd::List(true)));
    events.push(Event::Rule);
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use pulldown_cmark_mdcat::markdown_options;

    fn titles_and_slugs(events: &[Event<'_>]) -> Vec<(String, Option<String>)> {
        let mut result = Vec::new();
        let mut pending_href: Option<String> = None;
        for event in events {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    pending_href = Some(dest_url.to_string());
                }
                Event::Text(text) if text.as_ref() != "Table of Contents" => {
                    result.push((text.to_string(), pending_href.take()));
                }
                _ => {}
            }
        }
        result
    }

    #[test]
    fn no_headings_produces_no_events() {
        let events = build_toc_events("just a paragraph", markdown_options(false), None);
        assert!(events.is_empty());
    }

    #[test]
    fn flat_headings_become_a_flat_list() {
        let markdown = "# One\n\n# Two\n\n# Three\n";
        let events = build_toc_events(markdown, markdown_options(false), None);
        let titles = titles_and_slugs(&events);
        assert_eq!(
            titles,
            vec![
                ("One".to_string(), None),
                ("Two".to_string(), None),
                ("Three".to_string(), None),
            ]
        );
    }

    #[test]
    fn nested_headings_become_a_nested_list() {
        let markdown = "# Top\n\n## Child\n\n### Grandchild\n\n## Sibling\n";
        let events = build_toc_events(markdown, markdown_options(false), None);
        // Nesting is exercised structurally by checking that List start/end
        // markers balance and that all four titles appear in document order.
        let titles = titles_and_slugs(&events);
        assert_eq!(
            titles.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            vec!["Top", "Child", "Grandchild", "Sibling"]
        );
        let list_starts = events
            .iter()
            .filter(|e| matches!(e, Event::Start(Tag::List(_))))
            .count();
        let list_ends = events
            .iter()
            .filter(|e| matches!(e, Event::End(TagEnd::List(_))))
            .count();
        assert_eq!(list_starts, list_ends);
        // Top-level list, plus one nested list per of Top and Child.
        assert_eq!(list_starts, 3);
    }

    #[test]
    fn duplicate_titles_get_deduplicated_slugs() {
        let markdown = "# Foo\n\n# Foo\n\n# Foo\n";
        let events = build_toc_events(markdown, markdown_options(false), Some("doc.md"));
        let hrefs: Vec<_> = titles_and_slugs(&events)
            .into_iter()
            .map(|(_, href)| href.unwrap())
            .collect();
        assert_eq!(
            hrefs,
            vec![
                "doc.md#foo".to_string(),
                "doc.md#foo-1".to_string(),
                "doc.md#foo-2".to_string(),
            ]
        );
    }

    #[test]
    fn without_file_ref_entries_have_no_link() {
        let markdown = "# Foo\n";
        let events = build_toc_events(markdown, markdown_options(false), None);
        assert!(!events
            .iter()
            .any(|e| matches!(e, Event::Start(Tag::Link { .. }))));
    }
}
