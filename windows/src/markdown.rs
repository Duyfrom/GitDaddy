//! Cleans up README markdown before rendering: GitHub-style HTML blocks become markdown,
//! and images the viewer cannot load (remote or SVG) are reduced to their alt text.

const MAX_LINES: usize = 400;
const LINK_START: char = '\u{1}';

pub fn prepare_readme(markdown: &str, file_name: Option<&str>) -> String {
    prepare(markdown, file_name, true)
}

/// For markdown fetched from GitHub: there are no local files to show, so every image
/// becomes its alt text.
pub fn prepare_remote(markdown: &str) -> String {
    prepare(markdown, Some("README.md"), false)
}

fn prepare(markdown: &str, file_name: Option<&str>, local_images: bool) -> String {
    let is_markdown = file_name.is_none_or(|name| {
        let lower = name.to_lowercase();
        lower.ends_with(".md") || lower.ends_with(".markdown")
    });
    if !is_markdown {
        // Plain-text READMEs render verbatim.
        let body: Vec<&str> = markdown.lines().take(MAX_LINES).collect();
        return format!("```text\n{}\n```", body.join("\n"));
    }

    let mut out = Vec::new();
    let mut in_fence = false;
    let mut in_comment = false;
    for line in markdown.lines().take(MAX_LINES) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            out.push(line.to_string());
            continue;
        }
        if in_fence {
            out.push(line.to_string());
            continue;
        }
        let mut line = line.to_string();
        if in_comment {
            match line.find("-->") {
                Some(end) => line = line[end + 3..].to_string(),
                None => continue,
            }
        }
        let (without_comments, open) = strip_comments(&line);
        in_comment = open;
        line = without_comments;

        if line.trim_start().starts_with('<') {
            line = convert_html(&line);
        }
        out.push(rewrite_images(&line, local_images));
    }
    if in_fence {
        out.push("```".into());
    }
    out.join("\n")
}

fn strip_comments(line: &str) -> (String, bool) {
    let mut result = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("<!--") {
        result.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + 3..],
            None => return (result, true),
        }
    }
    result.push_str(rest);
    (result, false)
}

/// Turns a line of HTML into the closest markdown: headings, images, links, and line breaks.
fn convert_html(line: &str) -> String {
    let mut out = String::new();
    let mut heading: Option<usize> = None;
    let mut link_href: Option<String> = None;
    let mut rest = line;

    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(len) = rest[start..].find('>') else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + len];
        rest = &rest[start + len + 1..];

        let lower = tag.trim().to_lowercase();
        let name: String = lower.trim_start_matches('/').chars().take_while(char::is_ascii_alphanumeric).collect();
        let closing = lower.starts_with('/');
        match name.as_str() {
            "img" => {
                if let Some(src) = attribute(tag, "src") {
                    let alt = attribute(tag, "alt").unwrap_or_default();
                    out.push_str(&format!("![{alt}]({})", src.replace(' ', "%20")));
                }
            }
            "br" => out.push_str("  \n"),
            "a" if !closing => {
                // Mark where the link text starts; the closing tag turns it into `[text](href)`.
                link_href = attribute(tag, "href");
                if link_href.is_some() {
                    out.push(LINK_START);
                }
            }
            "a" => {
                if let (Some(href), Some(idx)) = (link_href.take(), out.rfind(LINK_START)) {
                    out.replace_range(idx..idx + LINK_START.len_utf8(), "[");
                    out.push_str(&format!("]({href})"));
                }
            }
            h if !closing
                && h.len() == 2
                && h.starts_with('h')
                && h[1..].parse::<usize>().is_ok_and(|n| (1..=6).contains(&n)) =>
            {
                heading = h[1..].parse().ok();
            }
            _ => {}
        }
    }
    out.push_str(rest);
    // Link openers whose closing tag is on another line just disappear.
    let mut out = out.replace(LINK_START, "");
    out = out.trim().to_string();
    if let Some(level) = heading
        && !out.is_empty()
    {
        out = format!("{} {out}", "#".repeat(level));
    }
    out
}

fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_lowercase();
    let mut search_from = 0;
    while let Some(found) = lower[search_from..].find(name) {
        let idx = search_from + found;
        search_from = idx + name.len();
        let preceded_ok = idx == 0 || lower.as_bytes()[idx - 1].is_ascii_whitespace();
        let after = lower[idx + name.len()..].trim_start();
        if !preceded_ok || !after.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - after.len() + 1;
        let value = tag[value_start..].trim_start();
        let quote = value.chars().next()?;
        if quote == '"' || quote == '\'' {
            let inner = &value[1..];
            return inner.find(quote).map(|end| inner[..end].to_string());
        }
        return Some(value.split_whitespace().next().unwrap_or("").trim_end_matches('/').to_string());
    }
    None
}

/// Replaces `![alt](url)` images that cannot be shown locally with their alt text.
fn rewrite_images(line: &str, local_images: bool) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(start) = rest.find("![") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let parsed = after.find("](").and_then(|alt_end| {
            let url_part = &after[alt_end + 2..];
            url_part.find(')').map(|url_end| (alt_end, url_end, url_part))
        });
        let Some((alt_end, url_end, url_part)) = parsed else {
            out.push_str("![");
            rest = after;
            continue;
        };
        let alt = &after[..alt_end];
        let url = url_part[..url_end].split_whitespace().next().unwrap_or("");
        if local_images && is_displayable_image(url) {
            out.push_str(&format!("![{alt}]({url})"));
        } else {
            out.push_str(alt);
        }
        rest = &url_part[url_end + 1..];
    }
    out.push_str(rest);
    // Drop links left with no text, such as badges without alt text.
    out.replace("[](", "\u{2}(")
        .split('\u{2}')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else {
                part.find(')').map_or_else(|| part.to_string(), |end| part[end + 1..].to_string())
            }
        })
        .collect()
}

fn is_displayable_image(url: &str) -> bool {
    let lower = url.to_lowercase();
    let remote = lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("//");
    let supported = [".png", ".jpg", ".jpeg", ".gif"].iter().any(|ext| lower.ends_with(ext));
    !remote && supported
}

/// A run of README markdown. Tables are kept apart because they cannot wrap and need
/// their own horizontal scroll area in the narrow inspector.
#[derive(Debug, PartialEq, Eq)]
pub enum Segment {
    Text(String),
    Table(String),
}

fn table_cells(line: &str) -> Vec<&str> {
    line.trim().trim_start_matches('|').trim_end_matches('|').split('|').map(str::trim).collect()
}

/// `| --- | :-: |` style separator row that marks the second line of a GFM table.
fn is_table_delimiter(line: &str) -> bool {
    line.contains('|')
        && table_cells(line).iter().all(|c| {
            let inner = c.trim_start_matches(':').trim_end_matches(':');
            !inner.is_empty() && inner.chars().all(|ch| ch == '-')
        })
}

/// A GFM table starts with a header row followed by a delimiter row with as many cells.
fn starts_table(header: &str, delimiter: &str) -> bool {
    header.contains('|') && is_table_delimiter(delimiter) && table_cells(header).len() == table_cells(delimiter).len()
}

/// Splits prepared markdown into text and table segments, leaving code fences intact.
pub fn split_tables(markdown: &str) -> Vec<Segment> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut segments = Vec::new();
    let mut text: Vec<&str> = Vec::new();
    let mut in_fence = false;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
        }
        let table_here = !in_fence && lines.get(i + 1).is_some_and(|next| starts_table(line, next));
        if !table_here {
            text.push(line);
            i += 1;
            continue;
        }
        if !text.is_empty() {
            segments.push(Segment::Text(text.join("\n")));
            text.clear();
        }
        let start = i;
        i += 2;
        while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
            i += 1;
        }
        segments.push(Segment::Table(lines[start..i].join("\n")));
    }
    if !text.is_empty() {
        segments.push(Segment::Text(text.join("\n")));
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_split_out() {
        let md = "Intro\n\n| A | B |\n| --- | :-: |\n| 1 | 2 |\n\nAfter\n```\n| not | a |\n| --- | --- |\n```";
        let segments = split_tables(md);
        assert_eq!(segments.len(), 3, "{segments:?}");
        assert_eq!(segments[0], Segment::Text("Intro\n".into()));
        assert_eq!(segments[1], Segment::Table("| A | B |\n| --- | :-: |\n| 1 | 2 |".into()));
        assert!(matches!(&segments[2], Segment::Text(t) if t.contains("| not | a |")));
        assert!(!is_table_delimiter("just - text"));
        assert!(!is_table_delimiter("---"));
        assert!(is_table_delimiter("|:---|---:|"));
        // A setext heading underline is not a table.
        assert_eq!(split_tables("a | b\n---\ntext").len(), 1);
    }

    #[test]
    fn converts_centered_logo_block() {
        let md = "<p align=\"center\">\n  <img src=\"Assets/logo.png\" width=\"200\" alt=\"Logo\" />\n</p>\n\n<h1 align=\"center\">GitDaddy</h1>";
        let out = prepare_readme(md, Some("README.md"));
        assert!(out.contains("![Logo](Assets/logo.png)"), "{out}");
        assert!(out.contains("# GitDaddy"), "{out}");
        assert!(!out.contains("<p"), "{out}");
    }

    #[test]
    fn remote_badges_become_text_links() {
        let md = "[![CI](https://img.shields.io/ci.svg)](https://ci.example) [![](https://x/y.svg)](https://z)";
        let out = prepare_readme(md, Some("README.md"));
        assert_eq!(out.trim(), "[CI](https://ci.example)");
    }

    #[test]
    fn keeps_code_fences_untouched() {
        let md = "```html\n<img src=\"a.png\">\n```";
        assert_eq!(prepare_readme(md, Some("README.md")), md);
    }

    #[test]
    fn html_links_and_comments() {
        let md = "<!-- hidden -->\n<a href=\"https://example.com\">Docs</a>";
        let out = prepare_readme(md, Some("README.md"));
        assert_eq!(out.trim(), "[Docs](https://example.com)");
    }

    #[test]
    fn plain_text_readme_is_fenced() {
        let out = prepare_readme("hello\nworld", Some("README.txt"));
        assert!(out.starts_with("```text\nhello"));
    }
}
