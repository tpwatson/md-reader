use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{html, CowStr, Event, Options, Parser, Tag, TagEnd};
use serde::Serialize;

const MAX_BYTES: usize = 20 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub path: String,
    pub name: String,
    pub directory: String,
    pub html: String,
}

pub struct Loaded {
    pub document: Document,
    pub directory: PathBuf,
    pub images: Vec<PathBuf>,
}

pub fn load(input: &str) -> Result<Loaded, String> {
    let path = prepare_path(input)?;
    let text = read_text(&path)?;
    let directory = path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let (html, images) = render(&directory, &text);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Untitled".to_string());

    Ok(Loaded {
        document: Document {
            path: path.to_string_lossy().into_owned(),
            name,
            directory: directory.to_string_lossy().into_owned(),
            html,
        },
        directory,
        images,
    })
}

pub fn resolve_href(current_file: &str, href: &str) -> Result<String, String> {
    let href = href.trim();
    let href = href.split(['#', '?']).next().unwrap_or(href);
    let href = percent_decode(href);
    if href.is_empty() {
        return Err("That link is empty.".into());
    }

    let current = prepare_path(current_file)?;
    let base = current.parent().unwrap_or(Path::new("."));
    let raw = PathBuf::from(&href);
    let joined = if raw.is_absolute() {
        raw
    } else {
        base.join(raw)
    };
    Ok(plain_path(&normalize(joined))
        .to_string_lossy()
        .into_owned())
}

pub fn prepare_path(input: &str) -> Result<PathBuf, String> {
    let input = input.trim().trim_matches('"');
    if input.is_empty() {
        return Err("No file was given.".into());
    }

    let path = absolute(PathBuf::from(input));
    if path.is_dir() {
        return Err("That is a folder. Choose a Markdown file.".into());
    }
    if !path.is_file() {
        return Err(format!("File not found:\n{}", path.display()));
    }

    let canonical = std::fs::canonicalize(&path).unwrap_or(path);
    Ok(plain_path(&canonical))
}

pub(crate) fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    let text = text
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| {
            text.strip_prefix(r"\\?\")
                .unwrap_or(text.as_ref())
                .to_string()
        });
    PathBuf::from(text)
}

fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read file: {error}"))?;
    if bytes.len() > MAX_BYTES {
        return Err("This file is larger than 20 MB.".into());
    }

    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    if !utf16 && bytes.iter().take(4096).any(|byte| *byte == 0) {
        return Err("This file does not look like a text document.".into());
    }
    Ok(decode_text(&bytes))
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect::<Vec<_>>();
        return String::from_utf16_lossy(&units);
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn render(base: &Path, markdown: &str) -> (String, Vec<PathBuf>) {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_SMART_PUNCTUATION
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_GFM;
    let events: Vec<_> = Parser::new_ext(markdown, options).collect();
    let images = collect_images(base, &events);
    let ids = heading_ids(&events);
    let mut html_out = String::new();
    html::push_html(
        &mut html_out,
        Render {
            events: events.into_iter(),
            ids,
            index: 0,
            in_meta: false,
            base,
        },
    );
    (html_out, images)
}

struct Render<'a> {
    events: std::vec::IntoIter<Event<'a>>,
    ids: Vec<String>,
    index: usize,
    in_meta: bool,
    base: &'a Path,
}

impl<'a> Iterator for Render<'a> {
    type Item = Event<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(event) = self.events.next() {
            if self.in_meta {
                if matches!(event, Event::End(TagEnd::MetadataBlock(_))) {
                    self.in_meta = false;
                }
                continue;
            }

            match event {
                Event::Start(Tag::MetadataBlock(_)) => {
                    self.in_meta = true;
                }
                Event::Html(content) | Event::InlineHtml(content) => {
                    return Some(Event::Text(content));
                }
                Event::Start(Tag::Heading {
                    level,
                    classes,
                    attrs,
                    ..
                }) => {
                    let id = self.ids.get(self.index).cloned().map(CowStr::from);
                    self.index += 1;
                    return Some(Event::Start(Tag::Heading {
                        level,
                        id,
                        classes,
                        attrs,
                    }));
                }
                Event::Start(Tag::Image {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => {
                    let dest_url = CowStr::from(image_src(self.base, &dest_url));
                    return Some(Event::Start(Tag::Image {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }));
                }
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    id,
                }) => {
                    let dest_url = CowStr::from(sanitize_link(&dest_url));
                    return Some(Event::Start(Tag::Link {
                        link_type,
                        dest_url,
                        title,
                        id,
                    }));
                }
                other => return Some(other),
            }
        }
        None
    }
}

fn heading_ids(events: &[Event<'_>]) -> Vec<String> {
    let mut ids = Vec::new();
    let mut used = HashSet::new();
    let mut index = 0;
    let mut in_meta = false;

    while index < events.len() {
        match &events[index] {
            Event::Start(Tag::MetadataBlock(_)) => in_meta = true,
            Event::End(TagEnd::MetadataBlock(_)) => in_meta = false,
            Event::Start(Tag::Heading { .. }) if !in_meta => {
                let mut text = String::new();
                let mut cursor = index + 1;
                while cursor < events.len() {
                    match &events[cursor] {
                        Event::End(TagEnd::Heading(_)) => break,
                        Event::Text(value) | Event::Code(value) => text.push_str(value),
                        _ => {}
                    }
                    cursor += 1;
                }
                ids.push(unique_slug(&text, ids.len(), &mut used));
                index = cursor;
            }
            _ => {}
        }
        index += 1;
    }

    ids
}

fn unique_slug(text: &str, index: usize, used: &mut HashSet<String>) -> String {
    let mut base = slugify(text);
    if base.is_empty() {
        base = format!("section-{}", index + 1);
    }
    if used.insert(base.clone()) {
        return base;
    }
    let mut suffix = 1;
    loop {
        let candidate = format!("{base}-{suffix}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
        suffix += 1;
    }
}

fn slugify(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn collect_images(base: &Path, events: &[Event<'_>]) -> Vec<PathBuf> {
    let mut images = Vec::new();
    for event in events {
        if let Event::Start(Tag::Image { dest_url, .. }) = event {
            if let Some(path) = local_image(base, dest_url) {
                images.push(path);
            }
        }
    }
    images
}

fn image_src(base: &Path, url: &str) -> String {
    if let Some(path) = local_image(base, url) {
        return format!("md-local:{}", percent_encode(&path.to_string_lossy()));
    }
    let url = url.trim();
    if is_remote(url) || url.starts_with("data:") {
        return url.to_string();
    }
    String::new()
}

fn local_image(base: &Path, url: &str) -> Option<PathBuf> {
    let url = url.trim();
    if url.is_empty() || is_remote(url) || url.to_ascii_lowercase().starts_with("data:") {
        return None;
    }
    let url = url.split(['?', '#']).next().unwrap_or(url);
    let path = if let Some(rest) = url.strip_prefix("file:") {
        file_url_to_path(rest)?
    } else {
        let decoded = percent_decode(url);
        let raw = PathBuf::from(decoded);
        if raw.is_absolute() {
            raw
        } else {
            base.join(raw)
        }
    };
    Some(plain_path(&normalize(path)))
}

fn file_url_to_path(rest: &str) -> Option<PathBuf> {
    let rest = rest.trim_start_matches('/');
    let decoded = percent_decode(rest);
    let decoded = decoded
        .strip_prefix("localhost/")
        .unwrap_or(decoded.as_str())
        .to_string();
    let mut text = decoded.as_str();
    if let Some(stripped) = text.strip_prefix('/') {
        let bytes = stripped.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            text = stripped;
        }
    }
    if text.is_empty() {
        None
    } else {
        Some(PathBuf::from(text))
    }
}

fn sanitize_link(url: &str) -> String {
    let url = url.trim();
    if url.is_empty() || url.starts_with('#') || is_remote(url) {
        return url.to_string();
    }
    if url.to_ascii_lowercase().starts_with("mailto:") {
        return url.to_string();
    }
    if scheme(url).is_some() && !is_windows_path(url) {
        return "#".into();
    }
    url.to_string()
}

fn is_remote(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_windows_path(url: &str) -> bool {
    let bytes = url.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
}

fn scheme(url: &str) -> Option<&str> {
    let bytes = url.as_bytes();
    if !bytes.first().is_some_and(|byte| byte.is_ascii_alphabetic()) {
        return None;
    }
    let mut index = 0;
    while index < bytes.len()
        && (bytes[index].is_ascii_alphanumeric()
            || bytes[index] == b'+'
            || bytes[index] == b'.'
            || bytes[index] == b'-')
    {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b':' {
        Some(&url[..index])
    } else {
        None
    }
}

fn absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        normalize(path)
    } else {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        normalize(cwd.join(path))
    }
}

fn normalize(path: PathBuf) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_a_real_markdown_file() {
        let dir = std::env::temp_dir().join(format!("md-reader-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("note.md");
        std::fs::write(
            &file,
            "---\ntitle: Hidden\n---\n\n# Hello world\n\nSee [next](other.md) and [bad](javascript:alert(1)).\n\n![pic](pic.png)\n\n<script>alert(1)</script>\n\n- [x] done\n\n| a | b |\n| - | - |\n| 1 | 2 |\n",
        )
        .unwrap();

        let loaded = load(&file.to_string_lossy()).unwrap();
        let html = &loaded.document.html;
        assert!(html.contains("id=\"hello-world\""));
        assert!(!html.contains("<script"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("href=\"other.md\""));
        assert!(html.contains("href=\"#\""));
        assert!(html.contains("md-local:"));
        assert!(html.contains("<table>"));
        assert!(html.contains("type=\"checkbox\""));
        assert!(!html.contains("title: Hidden"));
        assert_eq!(loaded.document.name, "note.md");

        let resolved = resolve_href(&loaded.document.path, "other.md").unwrap();
        assert!(resolved.ends_with("other.md"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
