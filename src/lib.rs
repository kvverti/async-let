#![no_std]
#![forbid(unsafe_code)]

use core::{future::Future, marker::PhantomData, pin::Pin};

use list::{At, Detach, Empty};
use wait::WaitFor;

pub mod list;
pub mod wait;

/// A typed handle representing a specific future type in an async let group.
#[derive(Debug)]
pub struct Handle<'fut, F> {
    _ph: PhantomData<&'fut mut F>,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ReadyOrNot<'fut, F: Future> {
    Ready(F::Output),
    Not(Pin<&'fut mut F>),
}

/// This type defines an async let group - specific set of futures that are driven
/// whenever a future is awaited through a group's cooperation.
pub struct Group<List> {
    fut_list: List,
}

impl Group<Empty> {
    pub const fn new() -> Self {
        Self {
            fut_list: Empty { _priv: () },
        }
    }
}

impl<List> Group<List> {
    pub fn attach<F: Future>(self, fut: Pin<&mut F>) -> (Handle<'_, F>, Group<At<'_, F, List>>) {
        (
            Handle { _ph: PhantomData },
            Group {
                fut_list: At {
                    node: ReadyOrNot::Not(fut),
                    tail: self.fut_list,
                },
            },
        )
    }

    pub fn detach<'fut, I, F: Future>(
        self,
        handle: Handle<'fut, F>,
    ) -> (ReadyOrNot<'fut, F>, Group<List::Output>)
    where
        List: Detach<'fut, F, I>,
    {
        let _ = handle;
        let (fut, rest) = self.fut_list.detach();
        (fut, Group { fut_list: rest })
    }

    pub fn wait_for<F>(&mut self, fut: F) -> WaitFor<'_, F, List> {
        WaitFor {
            driving_fut: fut,
            async_let_group: &mut self.fut_list,
        }
    }

    pub async fn detach_and_wait_for<'fut, I, F: Future>(
        self,
        handle: Handle<'fut, F>,
    ) -> (F::Output, Group<<List as Detach<'fut, F, I>>::Output>)
    where
        List: Detach<'fut, F, I>,
    {
        let (ready_or_not, group) = self.detach(handle);
        let output = match ready_or_not {
            ReadyOrNot::Ready(val) => val,
            ReadyOrNot::Not(fut) => fut.await,
        };
        (output, group)
    }
}
