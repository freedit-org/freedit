use axum::{extract::Query, response::IntoResponse};
use serde::Deserialize;
use tantivy::{collector::TopDocs, DocAddress, Score};

use crate::error::AppError;

use super::tantivy::SEARCHER;

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsSearch {
    search: String,
    offset: Option<usize>,
}

pub(crate) async fn search(Query(input): Query<ParamsSearch>) -> impl IntoResponse {
    let offset = input.offset.unwrap_or_default();
    let search = input.search.trim();

    if !search.is_empty() {
        let Ok(query) = SEARCHER.query_parser.parse_query(&search)else{
            return AppError::Custom("Please remove special chars".to_owned()).into_response();
        };

        let searcher = SEARCHER.reader.searcher();
        let top_docs: Vec<(Score, DocAddress)> = searcher
            .search(&query, &TopDocs::with_limit(20).and_offset(offset))
            .unwrap_or_default();

        for (_score, doc_address) in top_docs {
            let retrieved_doc = searcher.doc(doc_address).unwrap();
            println!("{}", SEARCHER.schema.to_json(&retrieved_doc));
        }
    }

    return ().into_response();
}
