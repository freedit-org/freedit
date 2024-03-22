use chrono::DateTime;
use once_cell::sync::Lazy;
use pulldown_cmark::{html, CodeBlockKind, Event, Options, Tag};
use syntect::{highlighting::ThemeSet, html::highlighted_html_for_string, parsing::SyntaxSet};

/// convert a `i64` timestamp to a date [`String`]
pub(super) fn ts_to_date(timestamp: i64) -> String {
    DateTime::from_timestamp(timestamp, 0)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
}

const MATHML_TAGS: [&str; 31] = [
    "maction",
    "math",
    "menclose",
    "merror",
    "mfenced",
    "mfrac",
    "mglyph",
    "mi",
    "mlabeledtr",
    "mmultiscripts",
    "mn",
    "mo",
    "mover",
    "mpadded",
    "mphantom",
    "mroot",
    "mrow",
    "ms",
    "mspace",
    "msqrt",
    "mstyle",
    "msub",
    "msubsup",
    "msup",
    "mtable",
    "mtd",
    "mtext",
    "mtr",
    "munder",
    "munderover",
    "semantics",
];

/// convert latex and markdown to html.
/// Inspired by [cmark-syntax](https://github.com/grego/cmark-syntax/blob/master/src/lib.rs)

// This file is part of cmark-syntax. This program comes with ABSOLUTELY NO WARRANTY;
// This is free software, and you are welcome to redistribute it under the
// conditions of the GNU General Public License version 3.0.
//
// You should have received a copy of the GNU General Public License
// along with cmark-syntax. If not, see <http://www.gnu.org/licenses/>
pub(super) fn md2html(md: &str) -> String {
    // list of mathml tags obtained from
    // <https://www.tutorialspoint.com/mathml/mathml_all_elements.htm>

    let parser = pulldown_cmark::Parser::new_ext(md, OPTIONS);
    let processed = SyntaxPreprocessor::new(parser);
    let mut html_output = String::with_capacity(md.len() * 2);
    html::push_html(&mut html_output, processed);
    clean_html(&html_output)
}

pub(super) fn clean_html(raw: &str) -> String {
    ammonia::Builder::default()
        .add_tags(&MATHML_TAGS)
        .add_allowed_classes("span", &["replytag"])
        .add_tag_attributes("pre", &["style"])
        .add_tag_attributes("span", &["style"])
        .clean(raw)
        .to_string()
}

const OPTIONS: Options = Options::all();

struct SyntaxPreprocessor<'a, I: Iterator<Item = Event<'a>>> {
    parent: I,
}

impl<'a, I: Iterator<Item = Event<'a>>> SyntaxPreprocessor<'a, I> {
    /// Create a new syntax preprocessor from `parent`.
    const fn new(parent: I) -> Self {
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
        let mut code = String::with_capacity(64);
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

        while let Some(Event::Text(text) | Event::Html(text)) = self.parent.next() {
            code.push_str(&text);
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
