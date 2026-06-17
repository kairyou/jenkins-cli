use scraper::{ElementRef, Html, Selector};
use std::collections::HashSet;

const DOWNSTREAM_HINTS: [&str; 2] = ["Scheduling project:", "Triggering a new build of"];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DownstreamJobLink {
    pub href: String,
    pub label: String,
}

impl DownstreamJobLink {
    pub fn key(&self) -> String {
        self.href.clone()
    }
}

pub fn render_console_html(html: &str) -> String {
    normalize_terminal_newlines(&Html::parse_fragment(html).root_element().text().collect::<String>())
}

fn normalize_terminal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

pub fn contains_downstream_hint(text: &str) -> bool {
    DOWNSTREAM_HINTS.iter().any(|hint| text.contains(hint))
}

pub fn extract_downstream_links(html: &str) -> Vec<DownstreamJobLink> {
    let fragment = Html::parse_fragment(html);
    let container_selector = Selector::parse("span, div, p, li").expect("valid selector");
    let link_selector = Selector::parse("a[href]").expect("valid selector");
    let mut seen = HashSet::new();
    let mut links = Vec::new();

    collect_downstream_link(&fragment.root_element(), &link_selector, &mut seen, &mut links);

    for container in fragment.select(&container_selector) {
        collect_downstream_link(&container, &link_selector, &mut seen, &mut links);
    }

    links
}

fn collect_downstream_link(
    container: &ElementRef<'_>,
    link_selector: &Selector,
    seen: &mut HashSet<String>,
    links: &mut Vec<DownstreamJobLink>,
) {
    let text = container.text().collect::<String>();
    if !contains_downstream_hint(&text) {
        return;
    }

    let Some(element) = first_link_after_downstream_hint(container, link_selector) else {
        return;
    };
    let Some(href) = element.value().attr("href") else {
        return;
    };
    if !href.contains("/job/") {
        return;
    }

    let label = element.text().collect::<String>().trim().to_string();
    if label.is_empty() {
        return;
    }

    let link = DownstreamJobLink {
        href: href.to_string(),
        label,
    };
    if seen.insert(link.key()) {
        links.push(link);
    }
}

fn first_link_after_downstream_hint<'a>(
    container: &'a ElementRef<'a>,
    link_selector: &Selector,
) -> Option<ElementRef<'a>> {
    let mut after_hint = false;

    for child in container.children() {
        if let Some(text) = child.value().as_text() {
            if DOWNSTREAM_HINTS.iter().any(|hint| text.text.contains(hint)) {
                after_hint = true;
            }
            continue;
        }

        let Some(element) = ElementRef::wrap(child) else {
            continue;
        };

        if element.value().name() == "a" && element.value().attr("href").is_some() {
            if after_hint {
                return Some(element);
            }
            continue;
        }

        if after_hint {
            if let Some(nested) = element.select(link_selector).next() {
                return Some(nested);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_console_html_as_plain_text() {
        let html = "</span><span class=\"pipeline-new-node\">[Pipeline] build\n</span><span>Scheduling project: <a href='/job/forder-test/job/test2/'>Folder Test » test2</a>\n";

        assert_eq!(
            render_console_html(html),
            "[Pipeline] build\r\nScheduling project: Folder Test » test2\r\n"
        );
    }

    #[test]
    fn extracts_pipeline_downstream_link() {
        let html = "</span><span>Scheduling project: <a href='/job/forder-test/job/test2/' class='jenkins-table__link model-link model-link--float'>Folder Test » test2</a>\n";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test2/".to_string(),
                label: "Folder Test » test2".to_string(),
            }]
        );
    }

    #[test]
    fn extracts_pipeline_downstream_link_from_progressive_html_node() {
        let html = "</span><span class=\"pipeline-new-node\" nodeId=\"13\" enclosingId=\"11\" label=\"Scheduling Folder Test » test2\">[Pipeline] build
</span><span class=\"pipeline-node-13\">Scheduling project: <a href='/job/forder-test/job/test2/' class='jenkins-table__link model-link model-link--float'>Folder Test » test2</a>
</span><span class=\"pipeline-new-node\" nodeId=\"14\" startId=\"11\">[Pipeline] }
";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test2/".to_string(),
                label: "Folder Test » test2".to_string(),
            }]
        );
    }

    #[test]
    fn extracts_freestyle_downstream_link() {
        let html = "<span>Triggering a new build of <a href='/job/forder-test/job/test/'>Folder Test » test</a>\n";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test/".to_string(),
                label: "Folder Test » test".to_string(),
            }]
        );
    }

    #[test]
    fn extracts_freestyle_downstream_link_from_root_fragment() {
        let html = "+ echo 'Build completed successfully!'\nBuild completed successfully!\nTriggering a new build of <a href='/job/forder-test/job/test/' class='jenkins-table__link model-link model-link--float'>Folder Test » test</a>\nFinished: SUCCESS\n";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test/".to_string(),
                label: "Folder Test » test".to_string(),
            }]
        );
    }

    #[test]
    fn ignores_job_links_without_downstream_hint() {
        let html = "<span>See <a href='/job/forder-test/job/test/'>Folder Test » test</a>\n";

        assert!(extract_downstream_links(html).is_empty());
    }

    #[test]
    fn ignores_unrelated_job_links_in_other_console_nodes() {
        let html = "\
<span>Started by upstream project <a href='/job/CLI-Test-Job/'>CLI-Test-Job</a> build number <a href='/job/CLI-Test-Job/34/'>34</a>\n</span>\
<span>Scheduling project: <a href='/job/forder-test/job/test2/'>Folder Test » test2</a>\n</span>";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test2/".to_string(),
                label: "Folder Test » test2".to_string(),
            }]
        );
    }

    #[test]
    fn extracts_only_the_first_job_link_after_downstream_hint() {
        let html = "\
<span><a href='/job/CLI-Test-Job/'>CLI-Test-Job</a> Scheduling project: <a href='/job/forder-test/job/test2/'>Folder Test » test2</a> <a href='/job/other/'>Other</a>\n</span>";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/forder-test/job/test2/".to_string(),
                label: "Folder Test » test2".to_string(),
            }]
        );
    }

    #[test]
    fn decodes_html_entities_from_text_and_href() {
        let html = "<span>Scheduling project: <a href='/job/a&amp;b/'>A &amp; B</a>\n";

        assert_eq!(
            extract_downstream_links(html),
            vec![DownstreamJobLink {
                href: "/job/a&b/".to_string(),
                label: "A & B".to_string(),
            }]
        );
    }
}
