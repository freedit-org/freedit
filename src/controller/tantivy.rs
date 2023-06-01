use std::collections::HashSet;

use bincode::config::standard;
use jieba_rs::{Jieba, TokenizeMode};
use once_cell::sync::Lazy;
use rust_stemmers::{Algorithm, Stemmer};
use sled::Db;
use tantivy::{
    directory::MmapDirectory,
    query::QueryParser,
    schema::{
        Field, IndexRecordOption, Schema, SchemaBuilder, TextFieldIndexing, TextOptions, FAST,
        INDEXED, STORED, STRING,
    },
    tokenizer::{BoxTokenStream, Token, TokenStream, Tokenizer},
    Document, Index, IndexReader, IndexWriter, Term,
};
use unicode_segmentation::UnicodeSegmentation;
use whichlang::detect_language;

use crate::{config::CONFIG, error::AppError};

use super::{
    db_utils::{get_one, u32_to_ivec},
    Comment, Item, Post, Solo,
};

pub(super) trait ToDoc {
    fn to_doc(&self, id: Option<u32>) -> Document;
}

pub(super) static SEARCHER: Lazy<Searcher> = Lazy::new(|| Tan::get_searcher().unwrap());
pub(super) static FIELDS: Lazy<Fields> = Lazy::new(|| Tan::set_schema().1);

pub struct Tan {
    writer: IndexWriter,
}

pub(super) struct Searcher {
    pub(super) reader: IndexReader,
    pub(super) query_parser: QueryParser,
    pub(super) schema: Schema,
}

pub(super) struct Fields {
    pub(super) id: Field,
    pub(super) title: Field,
    pub(super) time: Field,
    pub(super) uid: Field,
    pub(super) content: Field,
    pub(super) content_type: Field,
}

impl Tan {
    fn set_schema() -> (Schema, Fields) {
        let mut schema_builder = SchemaBuilder::default();

        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer(MULTI_LINGO_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options_stored = TextOptions::default()
            .set_indexing_options(text_indexing.clone())
            .set_stored();
        let text_options_nostored = TextOptions::default().set_indexing_options(text_indexing);

        let id = schema_builder.add_text_field("id", STRING | STORED);
        let title = schema_builder.add_text_field("title", text_options_stored);
        let time = schema_builder.add_text_field("time", STORED);
        let uid = schema_builder.add_u64_field("uid", STORED | INDEXED);
        let content = schema_builder.add_text_field("content", text_options_nostored);
        let content_type = schema_builder.add_text_field("content_type", FAST | STRING);

        let fields = Fields {
            id,
            title,
            time,
            uid,
            content,
            content_type,
        };
        let schema = schema_builder.build();

        (schema, fields)
    }

    fn get_index() -> tantivy::Result<Index> {
        let (schema, _) = Tan::set_schema();
        let index = tantivy::Index::open_or_create(
            MmapDirectory::open(&CONFIG.tantivy_path).unwrap(),
            schema,
        )?;
        let tokenizer = MultiLingoTokenizer {};
        index
            .tokenizers()
            .register(MULTI_LINGO_TOKENIZER, tokenizer);
        Ok(index)
    }

    pub fn init() -> tantivy::Result<Self> {
        let index = Tan::get_index()?;
        let writer = index.writer(50 * 1024 * 1024)?;
        Ok(Tan { writer })
    }

    fn get_searcher() -> tantivy::Result<Searcher> {
        let (schema, _) = Tan::set_schema();
        let index = Tan::get_index()?;
        let reader = index.reader().unwrap();
        let mut query_parser = QueryParser::for_index(&index, vec![FIELDS.title, FIELDS.content]);
        query_parser.set_conjunction_by_default();
        query_parser.set_field_boost(FIELDS.title, 2.);

        Ok(Searcher {
            reader,
            query_parser,
            schema,
        })
    }

    /// id should be `post123` `comt45#1` `solo123` or `item123`
    ///
    /// It just add doc to tantivy, not commit.
    pub fn add_doc(&mut self, id: String, db: Db) -> Result<(), AppError> {
        let content_type = &id[0..4];
        let ids: Vec<_> = id[4..].split('#').collect();
        let id1: u32 = ids[0].parse().unwrap();
        let doc = match content_type {
            "post" => {
                let post: Post = get_one(&db, "posts", id1)?;
                post.to_doc(None)
            }
            "comt" => {
                let id2: u32 = ids[1].parse().unwrap();
                let k = [&u32_to_ivec(id1), &u32_to_ivec(id2)].concat();
                let v = db
                    .open_tree("post_comments")?
                    .get(k)?
                    .ok_or(AppError::NotFound)?;
                let (comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                comment.to_doc(None)
            }
            "solo" => {
                let solo: Solo = get_one(&db, "solos", id1)?;
                solo.to_doc(None)
            }
            "item" => {
                let item: Item = get_one(&db, "items", id1)?;
                item.to_doc(Some(id1))
            }
            _ => unreachable!(),
        };

        self.writer.add_document(doc)?;

        Ok(())
    }

    pub fn del_index(&mut self, id: &str) -> tantivy::Result<()> {
        self.writer
            .delete_term(Term::from_field_text(FIELDS.id, id));
        Ok(())
    }

    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.writer.commit()?;
        Ok(())
    }
}

const MULTI_LINGO_TOKENIZER: &str = "multi_lingo_tokenizer";

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
    let mut tokens = Vec::with_capacity(text.len() / 4);
    match detect_language(text) {
        whichlang::Lang::Eng => {
            for (idx, (offset, word)) in text.unicode_word_indices().enumerate() {
                let word = word.to_lowercase();
                if !STOP_WORDS_ENG.contains(&word) && word.len() <= 30 {
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
                if !STOP_WORDS_CMN.contains(token.word) && token.word.len() <= 30 {
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

        _ => {
            for (idx, (offset, word)) in text.unicode_word_indices().enumerate() {
                let word = word.to_lowercase();
                if word.len() <= 30 {
                    tokens.push(Token {
                        offset_from: offset,
                        offset_to: offset + word.len(),
                        position: idx,
                        text: word,
                        position_length: 1,
                    });
                }
            }
        }
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
