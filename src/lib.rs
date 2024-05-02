//! This crate implements async-let as a `#[no_std]`, no-unsafe, no-panic, minimal-overhead library.
//!
//! Async-let is an experiment based on Conrad Ludgate's musings from <https://conradludgate.com/posts/async-let>,
//! which proposes a syntax that allows futures to be run "in the background" without spawning additional tasks.
//! The general behavior of async-let is that the following (fictitious) syntax
//!
//! ```ignore
//! async let metadata = client.request_metadata();
//! ```
//!
//! will mark a future as a background future. Any await point between the binding and `data.await` will poll
//! `data` as well as the future being awaited.
//!
//! ```ignore
//! // `metadata` is marked as a background future here
//! async let metadata = client.request_metadata();
//!
//! // both `data` and `metadata` are polled while awaiting `data`
//! let data = client.request_data().await;
//!
//! // unmark `metadata` as a background future and await it
//! let metadata = metadata.await;
//! ```
//!
//! Using this crate's API, the above can be written like this.
//!
//! ```
//! # use core::pin::pin;
//! # struct Client;
//! # impl Client {
//! #   async fn request_metadata(&self) {}
//! #   async fn request_data(&self) {}
//! # }
//! # let client = Client;
//! # pollster::block_on(async move {
//! // create a group that will drive the background futures
//! let group = async_let::Group::new();
//!
//! // mark `metadata` as a background future by attaching it to the group
//! let metadata = pin!(client.request_metadata());
//! let (metadata, mut group) = group.attach(metadata);
//!
//! // await `data` using the group, which polls the background futures
//! let data = group.wait_for(client.request_data()).await;
//!
//! // detach `metadata` from the group and await it
//! let (metadata, group) = group.detach_and_wait_for(metadata).await;
//! # });
//! ```
//!
//! ## API
//! The main types this crate offers are [`Group<List>`], a type that manages manages driving a statically typed
//! `List` of background futures; and [`Handle<Fut>`], an abstract handle that represents the capability to extract
//! a background future of type `Fut`. The [`attach`] method adds a future to the list of background futures, the
//! [`detach`] method removes a future, and the [`wait_for`] method produces a future that will poll the background
//! futures when it is polled.
//!
//! To minimize the overhead of tracking background futures, each `Group` is associated with a fixed set of futures.
//! Attaching a future to a group or detaching a future from a group consumes the group and produces a new group with
//! the future added to or removed from the set of background futures, respectively.
//!
//! ## Limitations
//! In the quest for minimal overhead, several tradeoffs were made.
//! - **Ergonomics**: while the API was made to be as ergonomic as possible, callers are still required to manually add and
//!   remove futures, thread the group through each operation, and locally pin futures explicitly.
//! - **Pinning**: Because a group stores its list of futures inline, the stored futures must be `Unpin`. Futures that require
//!   pinning must be stored behind an indirection, such as with `pin!` or `Box::pin`.
//! - **Lifetimes**: a group is necessarily moved when a future is attached or detached. If a future is attached in a nested
//!   scope, it must be detached before leaving that scope, or else the group (and all its attached futures) will be made
//!   inaccessible.
//! - **Branching**: because the set of futures is statically tracked, it is not possible to attach a future in only one branch
//!   of a condtional if one wishes the group to remain accessible after the conditional. Futures of different types may
//!   be attached to the same location in a group by erasing the type of the attached future to `dyn Future<Output = X>`,
//!   but this has its limitations.
//!
//! [`attach`]: Group::attach
//! [`detach`]: Group::detach
//! [`wait_for`]: Group::wait_for

#![no_std]
#![forbid(unsafe_code)]

use core::{future::Future, marker::PhantomData};

use list::{At, Detach, Empty, FutList};
use wait::WaitFor;

/// Types and traits for interacting with a group of futures.
pub mod list;
pub mod wait;

/// A typed handle representing a specific future type in an async let group. A handle can be redeemed for the future
/// it represents by passing it to [`Group::detach`].
#[derive(Debug)]
pub struct Handle<F> {
    /// A handle logically replaces a future.
    _ph: PhantomData<F>,
}

/// This type holds a future that has been detached from a group.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadyOrNot<F: Future>
{
    /// If the future has run to completion, this variant holds the future's output.
    Ready(F::Output),
    /// If the future has not yet run to completion, this variant holds the future.
    Not(F),
}

impl<F: Future> ReadyOrNot<F>
{
    /// A convenience method for retrieving the output of the future, either by driving the contained future
    /// to completion or by unwrapping the output value.
    ///
    /// Note that this method does *not* drive the background futures of an async group. To drive the background
    /// futures, use [`Group::detach_and_wait_for`] instead of detaching the future separately.
    pub async fn output(self) -> F::Output {
        match self {
            ReadyOrNot::Ready(val) => val,
            ReadyOrNot::Not(fut) => fut.await,
        }
    }
}

/// This type defines a specific set of futures that are driven whenever a future is awaited through a group's cooperation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Group<List> {
    /// The type-safe list of futures held in this group.
    fut_list: List,
}

impl Group<Empty> {
    /// Constructs a new group with no attached futures.
    #[inline]
    pub const fn new() -> Self {
        Self {
            fut_list: Empty { _priv: () },
        }
    }
}

impl<List> Group<List> {
    /// Adds a future to this group's set of background futures. The future must be `Unpin`, or be pinned in external
    /// storage before being attached.
    ///
    /// This method consumes `self` and returns a *new* group with the future attached, as well as a handle that
    /// can be used to later detach the future.
    ///
    /// # Example
    /// The most common way to use this method is to locally pin a future before attaching it.
    /// ```
    /// # use core::pin::pin;
    /// # async fn some_future() {}
    /// let group = async_let::Group::new();
    ///
    /// let fut = pin!(some_future()); // locally pin before attaching
    /// let (handle, group) = group.attach(fut);
    /// ```
    /// However, any pinning pointer to a future can be used.
    /// ```
    /// # async fn some_future() {}
    /// let group = async_let::Group::new();
    ///
    /// let mut fut = Box::pin(some_future()); // pin to the heap
    /// let (handle, group) = group.attach(fut);
    /// ```
    pub fn attach<F: Future + Unpin>(self, fut: F) -> (Handle<F>, Group<At<F, List>>)
    {
        (
            Handle { _ph: PhantomData },
            Group {
                fut_list: At {
                    node: ReadyOrNot::Not(fut),
                    tail: self.fut_list,
                    _holds_output: PhantomData,
                },
            },
        )
    }

    /// Removes a future from this group's set of background futures. The future held by this group
    /// is relinquished and returned to the caller. The detached future may have been partially driven or even completed.
    /// If the future is already completed, then its output is saved and returned to the caller instead of the future.
    ///
    /// This method consumes `self` and returns a *new* group with the future detached. If you want to additionally
    /// await the detached future, you can use `Self::detach_and_wait_for` as a convenience. If you want to cancel the detached
    /// future, you can use `Self::detach_and_cancel` as a convenience.
    ///
    /// # Example
    /// ```
    /// # use core::pin::pin;
    /// # async fn some_future() {}
    /// # async fn some_other_future() {}
    /// # fn pass_elsewhere(_: impl core::future::Future) {}
    /// # pollster::block_on(async {
    /// let group = async_let::Group::new();
    /// let fut = pin!(some_future());
    /// let (handle, group) = group.attach(fut);
    ///     
    /// // ... do work with `fut` in the background ...
    ///
    /// let (fut, mut group) = group.detach(handle);
    /// pass_elsewhere(fut.output());
    ///
    /// // continue to use the group
    /// let val = group.wait_for(some_other_future()).await;
    /// # });
    /// ```
    /// # Type Inference
    /// Usually, the future being detached is inferred from the provided handle. However, if multiple futures of the same
    /// type are attached to the group, then type inference will not be able to determine *which* future should be detached.
    /// In these cases, explicit disambiguation must be provided.
    /// ```
    /// # use core::pin::pin;
    /// # async fn some_future() {}
    /// let group = async_let::Group::new();
    ///
    /// let fut1 = pin!(some_future());
    /// let (handle1, group) = group.attach(fut1);
    ///
    /// let fut2 = pin!(some_future());
    /// let (handle2, group) = group.attach(fut2);
    ///
    /// // error: type annotations needed
    /// // let (fut1, group) = group.detach(handle1);
    /// use async_let::list::{S, Z};
    /// let (fut1, group) = group.detach::<S<Z>, _>(handle1);
    ///
    /// // type inference *can* infer the index now that the other future is detached
    /// let (fut2, group) = group.detach(handle2);
    /// ```
    pub fn detach<I, F: Future>(self, handle: Handle<F>) -> (ReadyOrNot<F>, Group<List::Output>)
    where
        List: Detach<F, I>,
    {
        let _ = handle;
        let (fut, rest) = self.fut_list.detach();
        (fut, Group { fut_list: rest })
    }

    /// Await a future while concurrently driving this group's background futures. The background futures are
    /// not polled if the future being awaited does not suspend. The background futures share a context with
    /// the future being awaited, so the enclosing task will be awoken if any one of the background futures makes progress.
    ///
    /// This method is used to await a future that is not in the set of background futures attached to this group.
    /// To await a future that *is* attached to this group, use [`Self::detach_and_wait_for`].
    ///
    /// # Example
    /// ```
    /// # use core::pin::pin;
    /// # pollster::block_on(async {
    /// let group = async_let::Group::new();
    ///
    /// // ... attach futures to `group` ...
    /// # let f1 = pin!(async {});
    /// # let (h1, group) = group.attach(f1);
    /// # let f2 = pin!(async {});
    /// # let (h2, mut group) = group.attach(f2);
    ///
    /// let output = group.wait_for(async { 3 + 7 }).await;
    /// assert_eq!(output, 10);
    /// # });
    /// ```
    pub fn wait_for<F>(&mut self, fut: F) -> WaitFor<'_, F, List> {
        WaitFor {
            driving_fut: fut,
            async_let_group: &mut self.fut_list,
        }
    }

    /// A convenience method for [`detach`]ing a future followed by [`wait_for`] to get its output. If you do not
    /// wish to drive the background futures while awaiting the output, use [`Group::detach`] followed by
    /// [`ReadyOrNot::output`].
    ///
    /// # Example
    /// ```
    /// # use core::pin::pin;
    /// # pollster::block_on(async {
    /// # let group = async_let::Group::new();
    /// let future = pin!(async { 3 + 7 });
    /// let (handle, group) = group.attach(future);
    ///
    /// // ... do other work with `future` in the background ...
    ///
    /// let (output, group) = group.detach_and_wait_for(handle).await;
    /// assert_eq!(output, 10);
    /// # });
    /// ```
    /// [`detach`]: Self::detach
    /// [`wait_for`]: Self::wait_for
    pub async fn detach_and_wait_for<I, F: Future>(
        self,
        handle: Handle<F>,
    ) -> (
        F::Output,
        Group<<List as Detach<F, I>>::Output>,
    )
    where
        List: Detach<F, I>,
        List::Output: FutList,
    {
        let (ready_or_not, mut group) = self.detach(handle);
        let output = group.wait_for(ready_or_not.output()).await;
        (output, group)
    }

    /// A convenience method for [`detach`]ing a future and immediately canceling it. This is useful
    /// if you need to detach a future that is about to go out of scope, in order to continue using the group.
    ///
    /// # Example
    /// ```
    /// # use core::pin::pin;
    /// # async fn some_future() {}
    /// # async fn some_other_future() {}
    /// # pollster::block_on(async {
    /// let group = async_let::Group::new();
    /// let mut group = {
    ///     // attach a short-lived future
    ///     let fut = pin!(some_future());
    ///     let (handle, group) = group.attach(fut);
    ///     
    ///     // ... do work with `fut` in the background ...
    ///
    ///     // "error: temporary value dropped while borrowed" if commented
    ///     group.detach_and_cancel(handle)
    /// };
    /// // continue to use the group
    /// let val = group.wait_for(some_other_future()).await;
    /// # });
    /// ```
    /// [`detach`]: Self::detach
    pub fn detach_and_cancel<I, F: Future>(self, handle: Handle<F>) -> Group<List::Output>
    where
        List: Detach<F, I>,
    {
        let (_, group) = self.detach(handle);
        group
    }
}
