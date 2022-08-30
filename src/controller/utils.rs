use crate::error::AppError;
use ::time::OffsetDateTime;
use data_encoding::HEXLOWER;
use once_cell::sync::Lazy;
use pulldown_cmark::{html, CodeBlockKind, Event, Options, Tag};
use ring::digest::{Context, Digest, SHA256};
use sled::Db;
use std::{
    env,
    fs::File,
    io::{BufReader, Read},
};
use syntect::{highlighting::ThemeSet, html::highlighted_html_for_string, parsing::SyntaxSet};
use tokio::time;
use tracing::{debug, instrument};

/// Returns SHA256 of the current running executable.
/// Cookbook: [Calculate the SHA-256 digest of a file](https://rust-lang-nursery.github.io/rust-cookbook/cryptography/hashing.html)
pub(crate) static CURRENT_SHA256: Lazy<String> = Lazy::new(|| {
    let file = env::current_exe().unwrap();
    let input = File::open(file).unwrap();

    fn sha256_digest<R: Read>(mut reader: R) -> Digest {
        let mut context = Context::new(&SHA256);
        let mut buffer = [0; 1024];

        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }
        context.finish()
    }

    let reader = BufReader::new(input);
    let digest = sha256_digest(reader);

    HEXLOWER.encode(digest.as_ref())
});

/// Cron job: Scan all the keys in the `Tree` regularly and remove the expired ones.
///
/// The keys must be the format of `timestamp_id`. See [generate_nanoid_expire](../controller/fn.generate_nanoid_expire.html).
#[instrument(skip(db))]
pub(crate) async fn clear_invalid(db: &Db, tree_name: &str, interval: u64) -> Result<(), AppError> {
    let tree = db.open_tree(tree_name)?;
    for i in tree.iter() {
        let (k, _) = i?;
        let k_str = std::str::from_utf8(&k)?;
        let time_stamp = k_str
            .split_once('_')
            .and_then(|s| i64::from_str_radix(s.0, 16).ok());
        if let Some(time_stamp) = time_stamp {
            if time_stamp < OffsetDateTime::now_utc().unix_timestamp() {
                debug!("remove expired {}: {}", tree_name, k_str);
                tree.remove(k)?;
            }
        }
    }
    time::sleep(time::Duration::from_secs(interval)).await;
    Ok(())
}

struct SyntaxPreprocessor<'a, I: Iterator<Item = Event<'a>>> {
    parent: I,
}

impl<'a, I: Iterator<Item = Event<'a>>> SyntaxPreprocessor<'a, I> {
    /// Create a new syntax preprocessor from `parent`.
    fn new(parent: I) -> Self {
        Self { parent }
    }
}

#[inline]
fn is_inline_latex(s: &str) -> bool {
    let s = s.as_bytes();
    s.len() > 1 && [s[0], s[s.len() - 1]] == [b'$', b'$']
}

static THEME_SET: Lazy<syntect::highlighting::ThemeSet> = Lazy::new(ThemeSet::load_defaults);
static SYNTAX_SET: Lazy<SyntaxSet> = Lazy::new(SyntaxSet::load_defaults_newlines);

impl<'a, I: Iterator<Item = Event<'a>>> Iterator for SyntaxPreprocessor<'a, I> {
    type Item = Event<'a>;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let lang = match self.parent.next()? {
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(lang))) => lang,
            Event::Code(c) if is_inline_latex(&c) => {
                return Some(Event::Html(
                    latex2mathml::latex_to_mathml(
                        &c[1..c.len() - 1],
                        latex2mathml::DisplayStyle::Inline,
                    )
                    .unwrap_or_else(|e| e.to_string())
                    .into(),
                ));
            }
            other => return Some(other),
        };

        let mut code = String::with_capacity(16);
        loop {
            match self.parent.next() {
                Some(Event::End(Tag::CodeBlock(_))) => break,
                Some(Event::Text(text)) => code.push_str(&text),
                None => return Some(Event::Text("CodeBlock end tag not found".into())),
                event => return Some(Event::Text(format!("Unexpected event {:?}", event).into())),
            };
        }

        if lang.as_ref() == "math" {
            return Some(Event::Html(
                latex2mathml::latex_to_mathml(&code, latex2mathml::DisplayStyle::Block)
                    .unwrap_or_else(|e| e.to_string())
                    .into(),
            ));
        }

        let syntax = if let Some(syntax) = SYNTAX_SET.find_syntax_by_name(lang.as_ref()) {
            syntax
        } else {
            SYNTAX_SET.find_syntax_by_extension("rs").unwrap()
        };

        let res = highlighted_html_for_string(
            &code,
            &SYNTAX_SET,
            syntax,
            &THEME_SET.themes["InspiredGitHub"],
        )
        .unwrap_or_else(|e| e.to_string());

        Some(Event::Html(res.into()))
    }
}

static OPTIONS: Options = Options::all();

// TODO: markdown may need to be escaped
/// convert latex and markdown to html
pub(super) fn md2html(md: &str) -> String {
    let parser = pulldown_cmark::Parser::new_ext(md, OPTIONS);
    let processed = SyntaxPreprocessor::new(parser);
    let mut html_output = String::with_capacity(md.len() * 2);
    html::push_html(&mut html_output, processed);
    html_output
}
