//! Merge the tagged HF stream with task candidates from the unfiltered stream.
//! The raw stream's last sort key is a frontier: tagged rows above it can be emitted
//! without scanning all of HF. Buffered rows and separate cursors survive pagination.
use super::{parse_search_item, sort_key, split_cursor, HfSearchOptions, HfSearchPage, SORT_KEYS};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, VecDeque};
use std::future::Future;

#[derive(Default, Serialize, Deserialize)]
struct Stream {
    cursor: Option<String>,
    started: bool,
    rows: VecDeque<Value>,
    frontier: Option<String>,
}

impl Stream {
    fn exhausted(&self) -> bool {
        self.started && self.cursor.is_none()
    }

    fn accept(&mut self, page: (Vec<Value>, Option<String>), task: Option<&str>, sort: &str) {
        self.started = true;
        self.cursor = page.1;
        self.frontier = page.0.last().map(|v| sort_key(v, sort));
        self.rows.extend(page.0.into_iter().filter(|v| {
            parse_search_item(v).is_some_and(|m| {
                task.is_none_or(|t| {
                    m.pipeline_tag.is_none() && m.inferred_task.as_deref() == Some(t)
                })
            })
        }));
    }

    fn discard_seen(&mut self, seen: &BTreeSet<String>) {
        while self
            .rows
            .front()
            .is_some_and(|v| v["id"].as_str().is_some_and(|id| seen.contains(id)))
        {
            self.rows.pop_front();
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct State {
    tagged: Stream,
    untagged: Stream,
    seen: BTreeSet<String>,
}

impl State {
    fn decode(cursor: Option<&str>) -> Result<Self, String> {
        if let Some(encoded) = cursor.and_then(|c| c.strip_prefix("tasks1.")) {
            let bytes = URL_SAFE_NO_PAD.decode(encoded).map_err(|e| e.to_string())?;
            return serde_json::from_slice(&bytes).map_err(|e| e.to_string());
        }
        // Continue cursors issued before the buffered merge was introduced.
        let (tagged, untagged) = split_cursor(cursor);
        Ok(Self {
            tagged: Stream {
                cursor: tagged,
                started: cursor.is_some(),
                ..Default::default()
            },
            untagged: Stream {
                cursor: untagged,
                started: cursor.is_some(),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn encode(&self) -> Option<String> {
        (!(self.tagged.exhausted()
            && self.untagged.exhausted()
            && self.tagged.rows.is_empty()
            && self.untagged.rows.is_empty()))
        .then(|| {
            format!(
                "tasks1.{}",
                URL_SAFE_NO_PAD.encode(serde_json::to_vec(self).expect("listing state"))
            )
        })
    }
}

pub(super) async fn search<F, Fut>(
    options: &HfSearchOptions,
    task: &str,
    fetch: F,
) -> Result<HfSearchPage, String>
where
    F: Fn(HfSearchOptions) -> Fut,
    Fut: Future<Output = Result<(Vec<Value>, Option<String>), String>>,
{
    let mut state = State::decode(options.cursor.as_deref())?;
    let sort = options
        .sort
        .as_deref()
        .filter(|s| SORT_KEYS.contains(s))
        .unwrap_or("downloads");
    let limit = options.limit.unwrap_or(30).clamp(1, 100) as usize;
    let opts = |stream: &Stream, tagged: bool| HfSearchOptions {
        cursor: stream.cursor.clone(),
        pipeline_tag: tagged.then(|| task.to_string()),
        // Scan sparse candidates in larger raw pages; output still obeys the UI's limit.
        limit: Some(100),
        ..options.clone()
    };
    let mut items = Vec::new();
    loop {
        state.tagged.discard_seen(&state.seen);
        state.untagged.discard_seen(&state.seen);
        let need_tagged = state.tagged.rows.is_empty() && !state.tagged.exhausted();
        let tagged_key = state.tagged.rows.front().map(|v| sort_key(v, sort));
        let need_untagged = state.untagged.rows.is_empty()
            && !state.untagged.exhausted()
            && (!state.untagged.started
                || tagged_key.as_ref().is_none_or(|key| {
                    state
                        .untagged
                        .frontier
                        .as_ref()
                        .is_none_or(|frontier| frontier > key)
                }));
        if need_tagged || need_untagged {
            let (tagged, untagged) = tokio::join!(
                async {
                    if need_tagged {
                        fetch(opts(&state.tagged, true)).await.map(Some)
                    } else {
                        Ok(None)
                    }
                },
                async {
                    if need_untagged {
                        fetch(opts(&state.untagged, false)).await.map(Some)
                    } else {
                        Ok(None)
                    }
                }
            );
            // Do not silently exhaust a failed stream: retrying the same cursor must retain candidates.
            if let Some(page) = tagged? {
                state.tagged.accept(page, None, sort);
            }
            if let Some(page) = untagged? {
                state.untagged.accept(page, Some(task), sort);
            }
            continue;
        }
        let tagged_first = match (state.tagged.rows.front(), state.untagged.rows.front()) {
            (Some(a), Some(b)) => sort_key(a, sort) >= sort_key(b, sort),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };
        let stream = if tagged_first {
            &mut state.tagged
        } else {
            &mut state.untagged
        };
        let row = stream.rows.pop_front().expect("stream head");
        let item = parse_search_item(&row).expect("validated listing row");
        state.seen.insert(item.id.clone());
        items.push(item);
        if items.len() == limit {
            break;
        }
    }
    state.tagged.discard_seen(&state.seen);
    state.untagged.discard_seen(&state.seen);
    Ok(HfSearchPage {
        items,
        next_cursor: state.encode(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::Mutex;

    fn row(id: &str, downloads: u64, tagged: bool) -> Value {
        let mut v = json!({"id": id, "downloads": downloads, "tags": ["gguf"],
            "gguf": {"architecture": "bert", "causal": false}});
        if tagged {
            v["pipeline_tag"] = json!("feature-extraction");
        }
        v
    }

    #[tokio::test]
    async fn sparse_two_page_streams_keep_order_buffers_and_deduplicate() {
        let calls = Mutex::new(Vec::new());
        let fetch = |o: HfSearchOptions| {
            calls
                .lock()
                .unwrap()
                .push((o.pipeline_tag.clone(), o.cursor.clone()));
            async move {
                Ok(match (o.pipeline_tag.is_some(), o.cursor.as_deref()) {
                    (true, None) => (
                        vec![row("tagged/a", 90, true), row("tagged/b", 60, true)],
                        Some("tag2".into()),
                    ),
                    (true, Some("tag2")) => (
                        vec![row("tagged/b", 60, true), row("tagged/c", 20, true)],
                        None,
                    ),
                    (false, None) => (
                        vec![
                            json!({"id":"llm/a", "downloads":100,"pipeline_tag":"text-generation"}),
                            json!({"id":"llm/b", "downloads":80,"pipeline_tag":"text-generation"}),
                        ],
                        Some("raw2".into()),
                    ),
                    (false, Some("raw2")) => (
                        vec![
                            row("encoder/a", 70, false),
                            row("encoder/b", 40, false),
                            row("encoder/b", 40, false),
                        ],
                        None,
                    ),
                    _ => panic!("unexpected fetch"),
                })
            }
        };
        let opts = HfSearchOptions {
            pipeline_tag: Some("feature-extraction".into()),
            sort: Some("downloads".into()),
            limit: Some(2),
            ..Default::default()
        };
        let first = search(&opts, "feature-extraction", &fetch).await.unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["tagged/a", "encoder/a"]
        );
        let second = search(
            &HfSearchOptions {
                cursor: first.next_cursor,
                ..opts.clone()
            },
            "feature-extraction",
            &fetch,
        )
        .await
        .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["tagged/b", "encoder/b"]
        );
        let third = search(
            &HfSearchOptions {
                cursor: second.next_cursor,
                ..opts
            },
            "feature-extraction",
            &fetch,
        )
        .await
        .unwrap();
        assert_eq!(
            third
                .items
                .iter()
                .map(|m| m.id.as_str())
                .collect::<Vec<_>>(),
            ["tagged/c"]
        );
        assert!(third.next_cursor.is_none());
        assert_eq!(calls.lock().unwrap().len(), 4);
    }

    #[tokio::test]
    async fn ties_prefer_tagged_and_failed_candidate_fetch_is_not_lost() {
        let opts = HfSearchOptions {
            limit: Some(1),
            ..Default::default()
        };
        let fetch = |o: HfSearchOptions| async move {
            Ok((
                vec![row(
                    if o.pipeline_tag.is_some() {
                        "tagged/a"
                    } else {
                        "encoder/a"
                    },
                    42,
                    o.pipeline_tag.is_some(),
                )],
                None,
            ))
        };
        let first = search(&opts, "feature-extraction", &fetch).await.unwrap();
        assert_eq!(first.items[0].id, "tagged/a");
        let next = search(
            &HfSearchOptions {
                cursor: first.next_cursor,
                ..opts.clone()
            },
            "feature-extraction",
            &fetch,
        )
        .await
        .unwrap();
        assert_eq!(next.items[0].id, "encoder/a");
        assert!(next.next_cursor.is_none());
        let result = search(&opts, "feature-extraction", |o| async move {
            if o.pipeline_tag.is_some() {
                Ok((vec![row("tagged/a", 42, true)], None))
            } else {
                Err("rate limited".into())
            }
        })
        .await;
        assert_eq!(result.unwrap_err(), "rate limited");
        assert!(State::decode(Some("tasks1.invalid!")).is_err());
    }

    /// Real HF, two output pages per task; run explicitly with --ignored --nocapture.
    #[tokio::test]
    #[ignore]
    async fn test_hf_task_candidates_continue_on_second_page() {
        for (task, query) in [
            ("feature-extraction", "embed"),
            ("text-ranking", "rerank"),
            ("feature-extraction", ""),
            ("text-ranking", ""),
        ] {
            let mut opts = HfSearchOptions {
                query: query.into(),
                pipeline_tag: Some(task.into()),
                sort: Some("downloads".into()),
                limit: Some(5),
                ..Default::default()
            };
            let mut seen = HashSet::new();
            let mut previous = u64::MAX;
            for page_number in 1..=2 {
                let page = super::super::DownloadManager::search_models_page(&opts, None)
                    .await
                    .unwrap();
                assert_eq!(page.items.len(), 5);
                let inferred: Vec<_> = page
                    .items
                    .iter()
                    .filter(|m| m.pipeline_tag.is_none())
                    .map(|m| &m.id)
                    .collect();
                eprintln!(
                    "{task} query={query:?} page {page_number}: {} rows; inferred {inferred:?}; downloads {:?}",
                    page.items.len(),
                    page.items.iter().map(|m| m.downloads).collect::<Vec<_>>()
                );
                assert!(
                    !inferred.is_empty(),
                    "expected untagged candidates on page {page_number}"
                );
                for item in &page.items {
                    assert!(seen.insert(item.id.clone()), "duplicate {}", item.id);
                    assert!(item.downloads <= previous, "order changed across pages");
                    previous = item.downloads;
                    assert!(
                        item.pipeline_tag.as_deref() == Some(task)
                            || item.inferred_task.as_deref() == Some(task)
                    );
                }
                opts.cursor = Some(page.next_cursor.expect("continuation"));
            }
        }
    }
}
