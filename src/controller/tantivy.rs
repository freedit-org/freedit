use std::collections::HashSet;

use askama::Template;
use axum::{
    extract::{Query, State},
    headers::Cookie,
    response::IntoResponse,
    TypedHeader,
};
use bincode::config::standard;
use jieba_rs::{Jieba, TokenizeMode};
use once_cell::sync::Lazy;
use rust_stemmers::{Algorithm, Stemmer};
use serde::Deserialize;
use sled::Db;
use tantivy::{
    collector::TopDocs,
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
    fmt::ts_to_date,
    meta_handler::{into_response, PageData},
    Claim, Comment, Item, Post, SiteConfig, Solo, User,
};

struct OutSearch {
    url: String,
    title: String,
    date: String,
    uid: Option<u32>,
    content_type: String,
}

/// Page data: `search.html`
#[derive(Template)]
#[template(path = "search.html", escape = "none")]
struct PageSearch<'a> {
    page_data: PageData<'a>,
    outs: Vec<OutSearch>,
    search: String,
    offset: usize,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsSearch {
    search: String,
    offset: Option<usize>,
}

pub(crate) async fn search(
    Query(input): Query<ParamsSearch>,
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let offset = input.offset.unwrap_or_default();
    let search = input.search.trim();

    let mut out_searchs = Vec::with_capacity(20);
    if !search.is_empty() {
        let Ok(query) = SEARCHER.query_parser.parse_query(search)else{
            return Err(AppError::Custom("Please remove special chars".to_owned()));
        };

        let searcher = SEARCHER.reader.searcher();
        let top_docs: Vec<(_, _)> = searcher
            .search(&query, &TopDocs::with_limit(20).and_offset(offset))
            .unwrap_or_default();

        for (_score, doc_address) in top_docs {
            let doc = searcher.doc(doc_address).unwrap();
            let id = doc.get_first(FIELDS.id).unwrap().as_text().unwrap();
            let out = OutSearch::get(&id, &db)?;
            out_searchs.push(out);
        }
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&db, claim.uid)?
    } else {
        false
    };

    let page_data = PageData::new("Search", &site_config, claim, has_unread);
    let page_search = PageSearch {
        page_data,
        outs: out_searchs,
        search: input.search,
        offset,
    };

    Ok(into_response(&page_search))
}

pub(super) trait ToDoc {
    fn to_doc(&self, id: Option<u32>) -> Document;
}

static SEARCHER: Lazy<Searcher> = Lazy::new(|| Tan::get_searcher().unwrap());
pub(super) static FIELDS: Lazy<Fields> = Lazy::new(|| Tan::set_schema().1);

pub struct Tan {
    writer: IndexWriter,
}

struct Searcher {
    reader: IndexReader,
    query_parser: QueryParser,
}

pub(super) struct Fields {
    pub(super) id: Field,
    pub(super) title: Field,
    pub(super) uid: Field,
    pub(super) content: Field,
    pub(super) content_type: Field,
}

impl Tan {
    pub fn init() -> tantivy::Result<Self> {
        let index = Tan::get_index()?;
        let writer = index.writer(50 * 1024 * 1024)?;
        Ok(Tan { writer })
    }

    /// id should be `post123` `comt45/1` `solo123` or `item123`
    ///
    /// It just add doc to tantivy, not commit.
    pub fn add_doc(&mut self, id: String, db: Db) -> Result<(), AppError> {
        let doc = extract_id(&id, db)?;
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

    fn set_schema() -> (Schema, Fields) {
        let mut schema_builder = SchemaBuilder::default();

        let text_indexing = TextFieldIndexing::default()
            .set_tokenizer(MULTI_LINGO_TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let text_options_nostored = TextOptions::default().set_indexing_options(text_indexing);

        let id = schema_builder.add_text_field("id", STORED);
        let title = schema_builder.add_text_field("title", text_options_nostored.clone());
        let uid = schema_builder.add_u64_field("uid", INDEXED);
        let content = schema_builder.add_text_field("content", text_options_nostored);
        let content_type = schema_builder.add_text_field("content_type", FAST | STRING);

        let fields = Fields {
            id,
            title,
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

    fn get_searcher() -> tantivy::Result<Searcher> {
        let index = Tan::get_index()?;
        let reader = index.reader().unwrap();
        let mut query_parser = QueryParser::for_index(&index, vec![FIELDS.title, FIELDS.content]);
        query_parser.set_conjunction_by_default();
        query_parser.set_field_boost(FIELDS.title, 2.);

        Ok(Searcher {
            reader,
            query_parser,
        })
    }
}

fn extract_id(id: &str, db: Db) -> Result<Document, AppError> {
    let content_type = &id[0..4];
    let ids: Vec<_> = id[4..].split('/').collect();
    let id1: u32 = ids[0].parse().unwrap();
    match content_type {
        "post" => {
            let post: Post = get_one(&db, "posts", id1)?;
            Ok(post.to_doc(None))
        }
        "comt" => {
            let id2: u32 = ids[1].parse().unwrap();
            let k = [&u32_to_ivec(id1), &u32_to_ivec(id2)].concat();
            let v = db
                .open_tree("post_comments")?
                .get(k)?
                .ok_or(AppError::NotFound)?;
            let (comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
            Ok(comment.to_doc(None))
        }
        "solo" => {
            let solo: Solo = get_one(&db, "solos", id1)?;
            Ok(solo.to_doc(None))
        }
        "item" => {
            let item: Item = get_one(&db, "items", id1)?;
            Ok(item.to_doc(Some(id1)))
        }
        _ => unreachable!(),
    }
}

impl OutSearch {
    fn get(id: &str, db: &Db) -> Result<Self, AppError> {
        let content_type = &id[0..4];
        let ids: Vec<_> = id[4..].split('/').collect();
        let id1: u32 = ids[0].parse().unwrap();
        let out = match content_type {
            "post" => {
                let post: Post = get_one(&db, "posts", id1)?;
                Self {
                    url: format!("/post/{}/{}", post.iid, post.pid),
                    title: post.title,
                    date: ts_to_date(post.created_at),
                    uid: Some(post.uid),
                    content_type: "post".to_string(),
                }
            }
            "comt" => {
                let id2: u32 = ids[1].parse().unwrap();
                let k = [&u32_to_ivec(id1), &u32_to_ivec(id2)].concat();
                let v = db
                    .open_tree("post_comments")?
                    .get(k)?
                    .ok_or(AppError::NotFound)?;
                let (comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                let post: Post = get_one(&db, "posts", id1)?;
                Self {
                    url: format!(
                        "/post/{}/{}?anchor={}&is_desc=false#{}",
                        post.iid,
                        comment.pid,
                        comment.cid - 1,
                        comment.cid
                    ),
                    title: comment.content,
                    date: ts_to_date(comment.created_at),
                    uid: Some(comment.uid),
                    content_type: "comment".to_string(),
                }
            }
            "solo" => {
                let solo: Solo = get_one(&db, "solos", id1)?;
                Self {
                    url: format!("/solo/{}", solo.sid),
                    title: solo.content,
                    date: ts_to_date(solo.created_at),
                    uid: Some(solo.uid),
                    content_type: "solo".to_string(),
                }
            }
            "item" => {
                let item: Item = get_one(&db, "items", id1)?;
                Self {
                    url: format!("/feed/read/{}", id1),
                    title: item.title,
                    date: ts_to_date(item.updated),
                    uid: None,
                    content_type: "item".to_string(),
                }
            }
            _ => unreachable!(),
        };

        Ok(out)
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
