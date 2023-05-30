use jieba_rs::{Jieba, TokenizeMode};
use once_cell::sync::Lazy;
use tantivy::tokenizer::Token;
use unicode_segmentation::UnicodeSegmentation;
use whichlang::detect_language;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

fn pre_tokenize_text(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    match detect_language(text) {
        whichlang::Lang::Cmn => {
            let orig_tokens = JIEBA.tokenize(text, TokenizeMode::Search, true);
            let mut indices = text.char_indices().collect::<Vec<_>>();
            indices.push((text.len(), '\0'));

            for i in 0..orig_tokens.len() {
                let token = &orig_tokens[i];
                tokens.push(Token {
                    offset_from: indices[token.start].0,
                    offset_to: indices[token.end].0,
                    position: token.start,
                    text: token.word.to_lowercase(),
                    position_length: 1,
                });
            }
        }

        whichlang::Lang::Jpn | whichlang::Lang::Kor => todo!(),

        _ => {
            for (idx, (offset, word)) in text.unicode_word_indices().enumerate() {
                tokens.push(Token {
                    offset_from: offset,
                    offset_to: offset + word.len(),
                    position: idx,
                    text: word.to_lowercase(),
                    position_length: 1,
                });
            }
        }
    }

    tokens
}
