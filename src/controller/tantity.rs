use std::collections::HashSet;

use jieba_rs::{Jieba, TokenizeMode};
use once_cell::sync::Lazy;
use rust_stemmers::{Algorithm, Stemmer};
use tantivy::tokenizer::{
    BoxTokenStream, RemoveLongFilter, TextAnalyzer, Token, TokenStream, Tokenizer,
};
use unicode_segmentation::UnicodeSegmentation;
use whichlang::detect_language;

const MULTI_LINGO_TOKENIZER: &str = "multi_lingo_tokenizer";

fn get_tokenizer() -> TextAnalyzer {
    TextAnalyzer::from(MultiLingoTokenizer).filter(RemoveLongFilter::limit(30))
}

#[derive(Clone)]
struct MultiLingoTokenizer;

impl Tokenizer for MultiLingoTokenizer {
    fn token_stream<'a>(&self, text: &'a str) -> BoxTokenStream<'a> {
        if text.is_empty() {
            return BoxTokenStream::from(MultiLingoTokenStream {
                tokens: vec![],
                index: 0,
            });
        }

        let tokens = pre_tokenize_text(text);
        BoxTokenStream::from(MultiLingoTokenStream { tokens, index: 0 })
    }
}

struct MultiLingoTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for MultiLingoTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);
static STEMMER_ENG: Lazy<Stemmer> = Lazy::new(|| Stemmer::create(Algorithm::English));

fn pre_tokenize_text(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    match detect_language(text) {
        whichlang::Lang::Eng => {
            for (idx, (offset, word)) in text.unicode_word_indices().enumerate() {
                let word = word.to_lowercase();
                if !STOP_WORDS_ENG.contains(&word) {
                    tokens.push(Token {
                        offset_from: offset,
                        offset_to: offset + word.len(),
                        position: idx,
                        text: STEMMER_ENG.stem(&word).to_string(),
                        position_length: 1,
                    });
                }
            }
        }

        whichlang::Lang::Cmn => {
            let text = fast2s::convert(text);
            let orig_tokens = JIEBA.tokenize(&text, TokenizeMode::Search, true);
            let mut indices = text.char_indices().collect::<Vec<_>>();
            indices.push((text.len(), '\0'));

            for token in orig_tokens {
                if !STOP_WORDS_CMN.contains(token.word) {
                    tokens.push(Token {
                        offset_from: indices[token.start].0,
                        offset_to: indices[token.end].0,
                        position: token.start,
                        text: token.word.to_lowercase(),
                        position_length: 1,
                    });
                }
            }
        }

        _ => todo!(),
    }

    tokens
}

static STOP_WORDS_ENG: Lazy<HashSet<String>> = Lazy::new(|| {
    stop_words::get(stop_words::LANGUAGE::English)
        .into_iter()
        .collect()
});

static STOP_WORDS_CMN: Lazy<HashSet<String>> = Lazy::new(|| {
    let mut set: HashSet<_> = stop_words::get(stop_words::LANGUAGE::Chinese)
        .into_iter()
        .collect();
    set.insert(" ".to_string());
    set
});
