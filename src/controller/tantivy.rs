use std::{collections::HashSet, sync::LazyLock};

use askama::Template;
use axum::{extract::Query, response::IntoResponse};
use axum_extra::{TypedHeader, headers::Cookie};
use bincode::config::standard;
use fjall::{KeyspaceCreateOptions, SingleWriterTxDatabase};
use indexmap::IndexSet;
use jieba_rs::{Jieba, TokenizeMode};
use rust_stemmers::{Algorithm, Stemmer};
use serde::Deserialize;
use tantivy::{
    Index, IndexReader, IndexWriter, TantivyDocument,
    collector::TopDocs,
    directory::MmapDirectory,
    query::QueryParser,
    schema::{
        FAST, Field, INDEXED, IndexRecordOption, STORED, STRING, Schema, SchemaBuilder,
        TextFieldIndexing, TextOptions, Value,
    },
    tokenizer::{Token, TokenStream, Tokenizer},
};
use tracing::{info, warn};
use unicode_segmentation::UnicodeSegmentation;
use whichlang::detect_language;

use crate::{DB, config::CONFIG, error::AppError};

use super::{
    Claim, Comment, InnType, Item, Post, PostStatus, SiteConfig, Solo, SoloType, User,
    db_utils::{get_one, u8_slice_to_u32, u32_to_ivec},
    filters,
    fmt::ts_to_date,
    meta_handler::{PageData, into_response},
};

struct OutSearch {
    url: String,
    title: String,
    date: String,
    uid: Option<u32>,
    ctype: String,
}

/// Page data: `search.html`
#[derive(Template)]
#[template(path = "search.html", escape = "none")]
struct PageSearch<'a> {
    page_data: PageData<'a>,
    outs: Vec<OutSearch>,
    search: String,
    offset: usize,
    ctype: String,
    uid: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsSearch {
    search: String,
    offset: Option<usize>,
    uid: Option<String>,
    ctype: Option<String>,
}

pub(crate) async fn search(
    Query(input): Query<ParamsSearch>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let offset = input.offset.unwrap_or_default();
    let search = input.search.trim();

    let mut query = search.to_owned();
    if let Some(ref uid) = input.uid
        && !uid.is_empty()
    {
        query.push_str(" uid:");
        query.push_str(uid);
    }
    if let Some(ref ctype) = input.ctype
        && ctype != "all"
    {
        query.push_str(" ctype:");
        query.push_str(ctype);
    };

    let mut ids = IndexSet::with_capacity(20);
    if !search.is_empty() {
        let (query, err) = SEARCHER.query_parser.parse_query_lenient(&query);
        if !err.is_empty() {
            warn!("search {search} contains err: {err:?}");
        }

        let searcher = SEARCHER.reader.searcher();
        let top_docs: Vec<(_, _)> = searcher
            .search(&query, &TopDocs::with_limit(20).and_offset(offset))
            .unwrap_or_default();

        for (_score, doc_address) in top_docs {
            let doc: TantivyDocument = searcher.doc(doc_address)?;
            let id = doc.get_first(FIELDS.id).unwrap().as_str().unwrap();
            ids.insert(id.to_owned());
        }
    }

    let mut out_searches = Vec::with_capacity(20);
    for id in ids {
        if let Some(out) = OutSearch::get(&id, &DB) {
            out_searches.push(out);
        }
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };

    let page_data = PageData::new("Search", &site_config, claim, has_unread);
    let page_search = PageSearch {
        page_data,
        outs: out_searches,
        search: input.search,
        offset,
        uid: input.uid,
        ctype: input.ctype.unwrap_or_else(|| "all".to_owned()),
    };

    Ok(into_response(&page_search))
}

pub(super) trait ToDoc {
    fn to_doc(&self, id: Option<u32>) -> TantivyDocument;
}

static SEARCHER: LazyLock<Searcher> = LazyLock::new(|| Tan::get_searcher().unwrap());
pub(super) static FIELDS: LazyLock<Fields> = LazyLock::new(|| Tan::set_schema().1);

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
    pub(super) ctype: Field,
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
    pub fn add_doc(&mut self, id: &str, db: &SingleWriterTxDatabase) -> Result<(), AppError> {
        let doc = extract_id(id, db)?;
        self.writer.add_document(doc)?;

        Ok(())
    }

    pub fn commit(&mut self) -> tantivy::Result<()> {
        self.writer.commit()?;
        Ok(())
    }

    pub fn rebuild_index(&mut self, db: &SingleWriterTxDatabase) -> Result<(), AppError> {
        let tan_tree = db.keyspace("tan", KeyspaceCreateOptions::default)?;
        for i in tan_tree.inner().iter() {
            let (k, _) = i.into_inner()?;
            tan_tree.remove(k)?;
        }

        for i in db
            .keyspace("user_posts", KeyspaceCreateOptions::default)?
            .inner()
            .iter()
        {
            let (k, v) = i.into_inner()?;
            let pid = u8_slice_to_u32(&k[4..8]);
            let inn_type = InnType::from(v[4]);
            if inn_type == InnType::Public || inn_type == InnType::Apply {
                let post: Post = get_one(db, "posts", pid)?;
                if post.status != PostStatus::HiddenByMod && post.status != PostStatus::HiddenByUser
                {
                    tan_tree.insert(format!("post{}", post.pid).as_bytes(), [])?;
                    for i in db
                        .keyspace("post_comments", KeyspaceCreateOptions::default)?
                        .inner()
                        .prefix(&k[4..8])
                    {
                        let (_, v) = i.into_inner()?;
                        let (comment, _): (Comment, usize) =
                            bincode::decode_from_slice(&v, standard())?;
                        if !comment.is_hidden {
                            tan_tree.insert(
                                format!("comt{}/{}", comment.pid, comment.cid).as_bytes(),
                                [],
                            )?;
                        }
                    }
                }
            }
        }

        for i in db
            .keyspace("solos", KeyspaceCreateOptions::default)?
            .inner()
            .iter()
        {
            let (_, v) = i.into_inner()?;
            let (solo, _): (Solo, usize) = bincode::decode_from_slice(&v, standard())?;
            if SoloType::from(solo.solo_type) == SoloType::Public {
                tan_tree.insert(format!("solo{}", solo.sid).as_bytes(), [])?;
            }
        }

        for i in db
            .keyspace("items", KeyspaceCreateOptions::default)?
            .inner()
            .iter()
        {
            let (k, _) = i.into_inner()?;
            let id = u8_slice_to_u32(&k);
            tan_tree.insert(format!("item{id}").as_bytes(), [])?;
        }

        self.writer.delete_all_documents()?;
        self.commit()?;
        info!("All search index deleted");

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
        let ctype = schema_builder.add_text_field("ctype", FAST | STRING);

        let fields = Fields {
            id,
            title,
            uid,
            content,
            ctype,
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

fn extract_id(id: &str, db: &SingleWriterTxDatabase) -> Result<TantivyDocument, AppError> {
    let ctype = &id[0..4];
    let ids: Vec<_> = id[4..].split('/').collect();
    let id1: u32 = ids[0].parse().unwrap();
    match ctype {
        "post" => {
            let post: Post = get_one(db, "posts", id1)?;
            Ok(post.to_doc(None))
        }
        "comt" => {
            let id2: u32 = ids[1].parse().unwrap();
            let k = [u32_to_ivec(id1), u32_to_ivec(id2)].concat();
            let v = db
                .keyspace("post_comments", KeyspaceCreateOptions::default)?
                .get(k)?
                .ok_or(AppError::NotFound)?;
            let (comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
            Ok(comment.to_doc(None))
        }
        "solo" => {
            let solo: Solo = get_one(db, "solos", id1)?;
            Ok(solo.to_doc(None))
        }
        "item" => {
            let item: Item = get_one(db, "items", id1)?;
            Ok(item.to_doc(Some(id1)))
        }
        _ => unreachable!(),
    }
}

impl OutSearch {
    fn get(id: &str, db: &SingleWriterTxDatabase) -> Option<Self> {
        let ctype = &id[0..4];
        let ids: Vec<_> = id[4..].split('/').collect();
        let id1: u32 = ids[0].parse().unwrap();

        match ctype {
            "post" => {
                let post: Post = get_one(db, "posts", id1).ok()?;
                Some(Self {
                    url: format!("/post/{}/{}", post.iid, post.pid),
                    title: post.title,
                    date: ts_to_date(post.created_at),
                    uid: Some(post.uid),
                    ctype: "post".to_string(),
                })
            }
            "comt" => {
                let id2: u32 = ids[1].parse().unwrap();
                let k = [u32_to_ivec(id1), u32_to_ivec(id2)].concat();
                let v = db
                    .keyspace("post_comments", KeyspaceCreateOptions::default)
                    .ok()?
                    .get(k)
                    .ok()??;
                let (comment, _): (Comment, usize) =
                    bincode::decode_from_slice(&v, standard()).ok()?;
                let post: Post = get_one(db, "posts", id1).ok()?;
                Some(Self {
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
                    ctype: "comment".to_string(),
                })
            }
            "solo" => {
                let solo: Solo = get_one(db, "solos", id1).ok()?;
                Some(Self {
                    url: format!("/solo/{}", solo.sid),
                    title: solo.content,
                    date: ts_to_date(solo.created_at),
                    uid: Some(solo.uid),
                    ctype: "solo".to_string(),
                })
            }
            "item" => {
                let item: Item = get_one(db, "items", id1).ok()?;
                Some(Self {
                    url: format!("/feed/read/{id1}"),
                    title: item.title,
                    date: ts_to_date(item.updated),
                    uid: None,
                    ctype: "item".to_string(),
                })
            }
            _ => unreachable!(),
        }
    }
}

const MULTI_LINGO_TOKENIZER: &str = "multi_lingo_tokenizer";

#[derive(Clone)]
struct MultiLingoTokenizer;

impl Tokenizer for MultiLingoTokenizer {
    type TokenStream<'a> = MultiLingoTokenStream;
    fn token_stream<'a>(&'a mut self, text: &'a str) -> MultiLingoTokenStream {
        if text.is_empty() {
            return MultiLingoTokenStream {
                tokens: vec![],
                index: 0,
            };
        }

        let tokens = pre_tokenize_text(text);
        MultiLingoTokenStream { tokens, index: 0 }
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

static JIEBA: LazyLock<Jieba> = LazyLock::new(Jieba::new);
static STEMMER_ENG: LazyLock<Stemmer> = LazyLock::new(|| Stemmer::create(Algorithm::English));

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

static STOP_WORDS_ENG: LazyLock<HashSet<String>> = LazyLock::new(|| {
    stop_words::get(stop_words::LANGUAGE::English)
        .iter()
        .map(|s| s.to_string())
        .collect()
});

static STOP_WORDS_CMN: LazyLock<HashSet<String>> = LazyLock::new(|| {
    let mut set: HashSet<_> = stop_words::get(stop_words::LANGUAGE::Chinese)
        .iter()
        .map(|s| s.to_string())
        .collect();
    set.insert(" ".to_string());
    set
});
