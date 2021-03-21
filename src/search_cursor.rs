//! Like [`eggmode::cursor`], but for search endpoints.
//!
//! Unfortunately (but for good reasons) the Twitter V2 Search API has a
//! slightly [different pagination scheme][page] than other V1 endpoints.
//! Specifically, it is token based instead of ID based and the names of the
//! fields used are slightly different (`max_results` instead of `count`,
//! `next_token` instead of `next_cursor`/`cursor`).
//!
//! The code for [`eggmode::cursor::CursorIter`] and [`eggmode::cursor::Cursor`]
//! is largely duplicated here. If this code is ever to be merged upstream, it'd
//! be worth looking into unifying this with `CursorIter` and `Cursor`, though
//! I don't know of a good way to do so.
//!
//! [page]: https://developer.twitter.com/en/docs/twitter-api/tweets/search/integrate/paginate

use eggmode::auth;
use serde::{de::DeserializeOwned, Deserialize};

// We currently only have a single wrapper search result type so we don't
// _really_ need this trait but I figured we might as well.
pub trait SearchCursor {
    type Item;

    fn next_token(&self) -> Option<&str>;

    fn into_inner(self) -> Vec<Self::Item>;
}

#[derive(Debug, Deserialize)]
struct SearchResultsMeta {
    newest_id: u64,
    oldest_id: u64,
    result_count: usize,
    next_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResults<I: Deserialize> {
    data: Vec<I>,
    meta: SearchResultsMeta,
}

#[must_use = "cursor iterators are lazy and do nothing unless consumed"]
pub struct SearchCursorIter<T>
where
    T: Cursor + DeserializeOwned,
{
    link: &'static str,
    token: auth::Token,
    params_base: Option<ParamList>,

    pub next_token: i64,

    loader: Option<FutureResponse<T>>,
    iter: Option<Box<dyn Iterator<Item = Response<T::Item>>>>,
}

impl<T> CursorIter<T>
where
    T: Cursor + DeserializeOwned,
{
    ///Sets the number of results returned in a single network call.
    ///
    ///Certain calls set their own minimums and maximums for what this value can be. Furthermore,
    ///some calls don't allow you to set the size of the pages at all. Refer to the individual
    ///methods' documentation for specifics. If this method is called for a response that does not
    ///accept changing the page size, no change to the underlying struct will occur.
    ///
    ///Calling this function will invalidate any current results, if any were previously loaded.
    pub fn with_page_size(self, page_size: i32) -> CursorIter<T> {
        if self.page_size.is_some() {
            CursorIter {
                page_size: Some(page_size),
                previous_cursor: -1,
                next_cursor: -1,
                loader: None,
                iter: None,
                ..self
            }
        } else {
            self
        }
    }

    ///Loads the next page of results.
    ///
    ///This is intended to be used as part of this struct's Iterator implementation. It is provided
    ///as a convenience for those who wish to manage network calls and pagination manually.
    pub fn call(&self) -> impl Future<Output = Result<Response<T>>> {
        let params = ParamList::from(self.params_base.as_ref().cloned().unwrap_or_default())
            .add_param("cursor", self.next_cursor.to_string())
            .add_opt_param("count", self.page_size.map_string());

        let req = get(self.link, &self.token, Some(&params));
        request_with_json_response(req)
    }

    ///Creates a new instance of CursorIter, with the given parameters and empty initial results.
    ///
    ///This is essentially an internal infrastructure function, not meant to be used from consumer
    ///code.
    pub(crate) fn new(
        link: &'static str,
        token: &auth::Token,
        params_base: Option<ParamList>,
        page_size: Option<i32>,
    ) -> CursorIter<T> {
        CursorIter {
            link: link,
            token: token.clone(),
            params_base: params_base,
            page_size: page_size,
            previous_cursor: -1,
            next_cursor: -1,
            loader: None,
            iter: None,
        }
    }
}

impl<T> Stream for CursorIter<T>
where
    T: Cursor + DeserializeOwned + 'static,
    T::Item: Unpin,
{
    type Item = Result<Response<T::Item>>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Option<Self::Item>> {
        if let Some(mut fut) = self.loader.take() {
            match Pin::new(&mut fut).poll(cx) {
                Poll::Pending => {
                    self.loader = Some(fut);
                    return Poll::Pending;
                }
                Poll::Ready(Ok(resp)) => {
                    self.previous_cursor = resp.previous_cursor_id();
                    self.next_cursor = resp.next_cursor_id();

                    let resp = Response::map(resp, |r| r.into_inner());
                    let rate = resp.rate_limit_status;

                    let mut iter = Box::new(resp.response.into_iter().map(move |item| Response {
                        rate_limit_status: rate,
                        response: item,
                    }));
                    let first = iter.next();
                    self.iter = Some(iter);

                    match first {
                        Some(item) => return Poll::Ready(Some(Ok(item))),
                        None => return Poll::Ready(None),
                    }
                }
                Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
            }
        }

        if let Some(ref mut results) = self.iter {
            if let Some(item) = results.next() {
                return Poll::Ready(Some(Ok(item)));
            } else if self.next_cursor == 0 {
                return Poll::Ready(None);
            }
        }

        self.loader = Some(Box::pin(self.call()));
        self.poll_next(cx)
    }
}
